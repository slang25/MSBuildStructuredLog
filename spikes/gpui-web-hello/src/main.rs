//! gpui-in-the-browser feasibility spike. Same gpui revision as the native
//! viewer, single-threaded web platform, no gpui_platform (see Cargo.toml).
//!
//! Layout: sidebar | uniform_list of ROW_COUNT rows | detail pane with a
//! monospace block. One button mutates state (appends rows) and a row can be
//! selected by click or arrow keys.

use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, ClickEvent, Context, FocusHandle, Focusable, KeyBinding,
    ScrollStrategy, SharedString, UniformListScrollHandle, Window, WindowBounds, WindowOptions,
    actions, div, px, rgb, size, uniform_list,
};
use std::ops::Range;
use std::rc::Rc;

const ROW_COUNT: usize = 10_000;
const ROW_HEIGHT: f32 = 22.;

const BG_BASE: u32 = 0x1e1e2e;
const BG_SIDEBAR: u32 = 0x181825;
const BG_SURFACE: u32 = 0x313244;
const BG_SELECTED: u32 = 0x3b5bdb;
const BG_HOVER: u32 = 0x2a2b3d;
const TEXT: u32 = 0xcdd6f4;
const TEXT_DIM: u32 = 0x6c7086;
const ACCENT: u32 = 0x89b4fa;
const GREEN: u32 = 0xa6e3a1;
const RED: u32 = 0xf38ba8;
const YELLOW: u32 = 0xf9e2af;

actions!(viewer, [SelectNext, SelectPrevious]);

#[derive(Clone, Copy)]
enum Kind {
    Project,
    Target,
    Task,
    Message,
    Warning,
    Error,
}

impl Kind {
    fn for_index(ix: usize) -> Kind {
        match ix % 37 {
            0 => Kind::Project,
            1 | 9 | 20 => Kind::Target,
            2 | 5 | 11 | 24 => Kind::Task,
            17 => Kind::Warning,
            33 => Kind::Error,
            _ => Kind::Message,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Project => "Project",
            Kind::Target => "Target",
            Kind::Task => "Task",
            Kind::Message => "Message",
            Kind::Warning => "Warning",
            Kind::Error => "Error",
        }
    }
    fn color(self) -> u32 {
        match self {
            Kind::Project => ACCENT,
            Kind::Target => GREEN,
            Kind::Task => TEXT,
            Kind::Message => TEXT_DIM,
            Kind::Warning => YELLOW,
            Kind::Error => RED,
        }
    }
    fn depth(self) -> usize {
        match self {
            Kind::Project => 0,
            Kind::Target => 1,
            Kind::Task => 2,
            _ => 3,
        }
    }
}

struct Viewer {
    row_count: usize,
    selected: Option<usize>,
    clicks: u32,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    started: web_time::Instant,
}

impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Viewer {
    fn row_text(ix: usize) -> String {
        format!("{} #{ix}: {}", Kind::for_index(ix).label(), match ix % 4 {
            0 => "Build started for Foo.csproj (net10.0)",
            1 => "CoreCompile → csc.dll /noconfig /nowarn:1701",
            2 => "Copying obj/Debug/Foo.dll to bin/Debug/",
            _ => "Property reassignment: $(Configuration)=Debug",
        })
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.row_count {
            self.selected = Some(ix);
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.map_or(0, |ix| ix + 1), cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.map_or(0, |ix| ix.saturating_sub(1)), cx);
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::Stateful<gpui::Div>> {
        range
            .map(|ix| {
                let kind = Kind::for_index(ix);
                let selected = self.selected == Some(ix);
                div()
                    .id(ix)
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .flex()
                    .items_center()
                    .pl(px(8. + 16. * kind.depth() as f32))
                    .pr_2()
                    .text_sm()
                    .text_color(rgb(if selected { TEXT } else { kind.color() }))
                    .when(selected, |d| d.bg(rgb(BG_SELECTED)))
                    .when(!selected, |d| d.hover(|s| s.bg(rgb(BG_HOVER))))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(ix, cx)))
                    .child(SharedString::from(Self::row_text(ix)))
            })
            .collect()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let stat = |label: &'static str, value: String| {
            div()
                .flex()
                .justify_between()
                .text_sm()
                .child(div().text_color(rgb(TEXT_DIM)).child(label))
                .child(div().text_color(rgb(TEXT)).child(SharedString::from(value)))
        };
        div()
            .w(px(240.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(BG_SIDEBAR))
            .child(div().text_lg().text_color(rgb(TEXT)).child("gpui web spike"))
            .child(div().text_sm().text_color(rgb(TEXT_DIM)).child("zed rev f66ed399 · wasm32 · WebGPU/WebGL"))
            .child(stat("rows", self.row_count.to_string()))
            .child(stat("selected", self.selected.map_or("none".into(), |ix| format!("#{ix}"))))
            .child(stat("button clicks", self.clicks.to_string()))
            .child(stat("uptime", format!("{:.1}s", self.started.elapsed().as_secs_f64())))
            .child(
                div()
                    .id("append")
                    .mt_2()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(GREEN))
                    .text_color(rgb(BG_BASE))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0xc6f3c1)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.row_count += 1_000;
                        this.clicks += 1;
                        cx.notify();
                    }))
                    .child("Append 1,000 rows"),
            )
            .child(div().text_sm().text_color(rgb(TEXT_DIM)).child("Click a row, then use up/down arrows."))
    }

    fn detail_pane(&self) -> impl IntoElement {
        let (title, body) = match self.selected {
            Some(ix) => (
                Self::row_text(ix),
                format!(
                    "<{} Id=\"{ix}\"\n         Depth=\"{}\"\n         Kind=\"{}\">\n  <Message>{}</Message>\n</{}>",
                    Kind::for_index(ix).label(),
                    Kind::for_index(ix).depth(),
                    Kind::for_index(ix).label(),
                    Self::row_text(ix),
                    Kind::for_index(ix).label(),
                ),
            ),
            None => ("Nothing selected".to_string(), "// select a row on the left\n// to see its source here".to_string()),
        };
        div()
            .w(px(360.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(BG_SIDEBAR))
            .child(div().text_color(rgb(TEXT)).child(SharedString::from(title)))
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(BG_SURFACE))
                    .font_family("Lilex")
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .whitespace_nowrap()
                    .children(body.lines().map(|l| div().child(SharedString::from(l.to_string())))),
            )
    }
}

impl Render for Viewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("root")
            .key_context("Viewer")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(BG_BASE))
            .text_color(rgb(TEXT))
            .child(self.sidebar(cx))
            .child(
                div().flex_1().h_full().min_w_0().child(
                    uniform_list(
                        "rows",
                        self.row_count,
                        cx.processor(|this, range: Range<usize>, _, cx| this.render_rows(range, cx)),
                    )
                    .size_full()
                    .track_scroll(&self.scroll),
                ),
            )
            .child(self.detail_pane())
    }
}

fn main() {
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    // Mirrors gpui_platform::single_threaded_web(), which we cannot use
    // because gpui_platform hard-enables gpui_web's `multithreaded` feature.
    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    // `Application::run` returns immediately on web (the browser owns the run
    // loop) and would drop the App right after launch, so use the embedded
    // form and leak the handle for the lifetime of the tab.
    let app = Application::with_platform(platform).with_http_client(http_client);
    let handle = app.run_embedded(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("down", SelectNext, Some("Viewer")),
            KeyBinding::new("up", SelectPrevious, Some("Viewer")),
        ]);
        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |window, cx| {
                let view = cx.new(|cx| Viewer {
                    row_count: ROW_COUNT,
                    selected: None,
                    clicks: 0,
                    scroll: UniformListScrollHandle::new(),
                    focus: cx.focus_handle(),
                    started: web_time::Instant::now(),
                });
                let focus = view.read(cx).focus.clone();
                window.focus(&focus, cx);
                view
            },
        )
        .expect("failed to open window");
        cx.activate(true);
    });
    std::mem::forget(handle);
}
