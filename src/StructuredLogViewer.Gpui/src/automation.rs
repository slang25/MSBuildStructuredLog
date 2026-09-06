//! In-process driving of the viewer for agents and scripts.
//!
//! `--automation` turns stdin into a command channel: one JSON object per
//! line in, one JSON reply per line out. Commands act inside gpui — real
//! keystrokes, real mouse events, real actions — so nothing depends on
//! screen coordinates, window focus, or timing of the OS event queue.
//! Interactive elements register their window-relative bounds through
//! [`probe`], so a click can be aimed at an element by name.
//!
//! ```text
//! {"cmd":"keys","keys":"cmd-f"}            // space-separated keystrokes
//! {"cmd":"type","text":"import"}           // typed characters
//! {"cmd":"action","name":"source_editor::FindNext"}
//! {"cmd":"click","id":"find-next"}         // or {"cmd":"click","x":..,"y":..}
//! {"cmd":"move","id":"pill-50"}            // hover, also by x/y
//! {"cmd":"scroll","id":"source-well","dx":-300,"dy":0}
//! {"cmd":"scroll","id":"tree-row-0","dy":120,"phase":"started"}  // a
//!                                          // trackpad gesture, not a wheel
//! {"cmd":"bounds","id":"tab-files"}
//! {"cmd":"probes"}                         // every id currently laid out
//! {"cmd":"dump"}                           // the workspace's state as JSON
//! {"cmd":"screenshot","path":"/tmp/x.png"}
//! {"cmd":"sleep","ms":300}
//! {"cmd":"quit"}
//! ```
//!
//! `scripts/drive.py` wraps this for the command line.

use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{
    App, Bounds, IntoElement, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, WindowHandle, canvas, div, point, px,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(false);

fn probes() -> &'static Mutex<HashMap<String, Bounds<Pixels>>> {
    static PROBES: OnceLock<Mutex<HashMap<String, Bounds<Pixels>>>> = OnceLock::new();
    PROBES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start recording element bounds. Cheap to leave off: `probe` then
/// renders nothing.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// A zero-size child that reports its parent's bounds under `id` every
/// frame. The parent must be `.relative()`.
pub fn probe(id: impl Into<String>) -> gpui::AnyElement {
    if !enabled() {
        return div().into_any_element();
    }
    let id = id.into();
    canvas(
        move |bounds, _window, _cx| {
            probes().lock().unwrap().insert(id.clone(), bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// Where `id` was last laid out, window-relative.
pub fn bounds(id: &str) -> Option<Bounds<Pixels>> {
    probes().lock().unwrap().get(id).copied()
}

/// Forget every recorded bounds, draw a frame, and let it land. Rows a
/// list no longer renders would otherwise keep reporting where they were.
async fn fresh_frame(window: WindowHandle<Workspace>, cx: &mut gpui::AsyncApp) -> anyhow::Result<()> {
    probes().lock().unwrap().clear();
    cx.update_window(window.into(), |_, window, _| window.refresh())?;
    cx.background_executor().timer(Duration::from_millis(60)).await;
    Ok(())
}

pub fn probe_ids() -> Vec<String> {
    let mut ids: Vec<String> = probes().lock().unwrap().keys().cloned().collect();
    ids.sort();
    ids
}

// ----- the command channel -----

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
enum Command {
    Keys { keys: String },
    Type { text: String },
    Action { name: String },
    Click { id: Option<String>, x: Option<f32>, y: Option<f32>, #[serde(default)] button: Button, #[serde(default)] modifiers: Mods },
    Move { id: Option<String>, x: Option<f32>, y: Option<f32> },
    Scroll {
        id: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
        #[serde(default)]
        dx: f32,
        #[serde(default)]
        dy: f32,
        /// "started" (fingers land) / "moved" (the default) / "ended"
        /// (fingers lift), for anything that tells a trackpad gesture from
        /// a wheel notch.
        phase: Option<String>,
    },
    Bounds { id: String },
    Probes,
    Dump,
    Screenshot { path: String },
    Sleep { ms: u64 },
    Quit,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Deserialize, Default, Clone, Copy)]
struct Mods {
    #[serde(default)]
    cmd: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    ctrl: bool,
}

impl Mods {
    fn modifiers(self) -> Modifiers {
        Modifiers { control: self.ctrl, alt: self.alt, shift: self.shift, platform: self.cmd, function: false }
    }
}

/// Reads commands from stdin on a thread and applies them on the UI
/// thread, one at a time, replying after each.
pub fn serve(cx: &mut App, window: WindowHandle<Workspace>) {
    enable();
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
    std::thread::Builder::new()
        .name("automation-stdin".into())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if tx.unbounded_send(line).is_err() {
                    break;
                }
            }
        })
        .expect("automation stdin thread");

    cx.spawn(async move |cx| {
        use futures::StreamExt;
        use std::io::Write;
        while let Some(line) = rx.next().await {
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Command>(&line) {
                Ok(command) => match apply(command, window, cx).await {
                    Ok(Some(result)) => json!({ "ok": true, "result": result }),
                    Ok(None) => json!({ "ok": true }),
                    Err(err) => json!({ "ok": false, "error": format!("{err:#}") }),
                },
                Err(err) => json!({ "ok": false, "error": format!("bad command: {err}") }),
            };
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    })
    .detach();
}

async fn apply(command: Command, window: WindowHandle<Workspace>, cx: &mut gpui::AsyncApp) -> anyhow::Result<Option<Value>> {
    match command {
        Command::Keys { keys } => {
            let keystrokes: Vec<Keystroke> = keys
                .split_whitespace()
                .map(|k| Keystroke::parse(k).map_err(|e| anyhow::anyhow!("{e:?}")))
                .collect::<anyhow::Result<_>>()?;
            cx.update_window(window.into(), |_, window, cx| {
                for keystroke in keystrokes {
                    window.dispatch_keystroke(keystroke, cx);
                }
            })?;
            Ok(None)
        }
        Command::Type { text } => {
            let keystrokes: Vec<Keystroke> = text
                .chars()
                .map(|c| Keystroke::parse(&c.to_string()).map_err(|e| anyhow::anyhow!("{e:?}")))
                .collect::<anyhow::Result<_>>()?;
            cx.update_window(window.into(), |_, window, cx| {
                for keystroke in keystrokes {
                    window.dispatch_keystroke(keystroke, cx);
                }
            })?;
            Ok(None)
        }
        Command::Action { name } => {
            cx.update_window(window.into(), |_, window, cx| -> anyhow::Result<()> {
                let action = cx.build_action(&name, None).map_err(|e| anyhow::anyhow!("{e:?}"))?;
                window.dispatch_action(action, cx);
                Ok(())
            })??;
            Ok(None)
        }
        Command::Click { id, x, y, button, modifiers } => {
            fresh_frame(window, cx).await?;
            let position = target(id.as_deref(), x, y)?;
            on_screen(window, position, cx)?;
            let button = match button {
                Button::Left => MouseButton::Left,
                Button::Right => MouseButton::Right,
                Button::Middle => MouseButton::Middle,
            };
            let modifiers = modifiers.modifiers();
            // Real input has frames between these; hover state and the
            // text element's click tracking are computed at paint.
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }),
                    cx,
                );
            })?;
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers, click_count: 1, first_mouse: false }),
                    cx,
                );
            })?;
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent { button, position, modifiers, click_count: 1 }),
                    cx,
                );
            })?;
            Ok(Some(json!({ "x": f32::from(position.x), "y": f32::from(position.y) })))
        }
        Command::Move { id, x, y } => {
            fresh_frame(window, cx).await?;
            let position = target(id.as_deref(), x, y)?;
            on_screen(window, position, cx)?;
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() }),
                    cx,
                );
            })?;
            Ok(None)
        }
        Command::Scroll { id, x, y, dx, dy, phase } => {
            let phase = match phase.as_deref() {
                None | Some("moved") => TouchPhase::Moved,
                Some("started") => TouchPhase::Started,
                Some("ended") => TouchPhase::Ended,
                Some(other) => anyhow::bail!("unknown touch phase {other:?}"),
            };
            fresh_frame(window, cx).await?;
            let position = target(id.as_deref(), x, y)?;
            on_screen(window, position, cx)?;
            cx.update_window(window.into(), |_, window, cx| {
                // Hit-testing follows the last mouse position.
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position,
                        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                        modifiers: Modifiers::default(),
                        touch_phase: phase,
                    }),
                    cx,
                );
            })?;
            Ok(None)
        }
        Command::Bounds { id } => {
            fresh_frame(window, cx).await?;
            let b = bounds(&id).ok_or_else(|| anyhow::anyhow!("no element laid out as {id:?}"))?;
            Ok(Some(bounds_json(b)))
        }
        Command::Probes => {
            fresh_frame(window, cx).await?;
            Ok(Some(json!(probe_ids())))
        }
        Command::Dump => {
            let value = cx.update_window(window.into(), |root, window, cx| {
                root.downcast::<Workspace>().ok().map(|workspace| workspace.read(cx).describe(window, cx))
            })?;
            Ok(value)
        }
        Command::Screenshot { path } => {
            let bounds = cx.update_window(window.into(), |_, window, cx| {
                cx.activate(true);
                window.bounds()
            })?;
            // Let the activation and a frame land before the capture.
            cx.background_executor().timer(Duration::from_millis(250)).await;
            // By window id when the OS will tell us it (nothing else can
            // bleed into the capture); by screen region otherwise.
            let region = format!(
                "{},{},{},{}",
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height)
            );
            let (how, args): (Value, Vec<String>) = match window_number(std::process::id()) {
                Some(id) => (json!({ "window": id }), vec!["-x".into(), "-l".into(), id.to_string(), path.clone()]),
                None => (json!({ "region": region }), vec!["-x".into(), "-R".into(), region.clone(), path.clone()]),
            };
            let status = std::process::Command::new("screencapture").args(&args).status()?;
            anyhow::ensure!(status.success(), "screencapture exited with {status}");
            Ok(Some(json!({ "path": path, "capture": how })))
        }
        Command::Sleep { ms } => {
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            Ok(None)
        }
        Command::Quit => {
            cx.update(|cx| cx.quit());
            Ok(None)
        }
    }
}

/// The on-screen, layer-0 window owned by `pid`, as `screencapture -l`
/// wants it.
#[cfg(target_os = "macos")]
fn window_number(pid: u32) -> Option<u32> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{copy_window_info, kCGNullWindowID, kCGWindowListOptionOnScreenOnly};

    let windows = copy_window_info(kCGWindowListOptionOnScreenOnly, kCGNullWindowID)?;
    for ix in 0..windows.len() {
        let item = windows.get(ix)?;
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef) };
        let number = |key: &str| -> Option<i64> {
            dict.find(&CFString::new(key)).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64())
        };
        if number("kCGWindowOwnerPID") == Some(pid as i64) && number("kCGWindowLayer") == Some(0) {
            return number("kCGWindowNumber").map(|n| n as u32);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn window_number(_pid: u32) -> Option<u32> {
    None
}

/// Refuse targets outside the viewport: a stale or clipped element must
/// not turn into a click on whatever happens to be there.
fn on_screen(window: WindowHandle<Workspace>, position: Point<Pixels>, cx: &mut gpui::AsyncApp) -> anyhow::Result<()> {
    let size = cx.update_window(window.into(), |_, window, _| window.viewport_size())?;
    anyhow::ensure!(
        position.x >= px(0.) && position.y >= px(0.) && position.x < size.width && position.y < size.height,
        "target ({}, {}) is outside the {}×{} window",
        f32::from(position.x),
        f32::from(position.y),
        f32::from(size.width),
        f32::from(size.height)
    );
    Ok(())
}

/// A point from an element id (its centre) or explicit coordinates.
fn target(id: Option<&str>, x: Option<f32>, y: Option<f32>) -> anyhow::Result<Point<Pixels>> {
    match (id, x, y) {
        (Some(id), _, _) => {
            let b = bounds(id).ok_or_else(|| anyhow::anyhow!("no element laid out as {id:?}"))?;
            Ok(b.center())
        }
        (None, Some(x), Some(y)) => Ok(point(px(x), px(y))),
        _ => anyhow::bail!("give an element id, or x and y"),
    }
}

fn bounds_json(b: Bounds<Pixels>) -> Value {
    json!({
        "x": f32::from(b.origin.x),
        "y": f32::from(b.origin.y),
        "width": f32::from(b.size.width),
        "height": f32::from(b.size.height),
    })
}

/// Shared helper for the state dumps.
pub fn selection_json(start: usize, end: usize, text: &str) -> Value {
    json!({ "start": start, "end": end, "text": text })
}

// Keep Arc in scope for the type used by callers' signatures.
#[allow(dead_code)]
type Shared<T> = Arc<T>;
