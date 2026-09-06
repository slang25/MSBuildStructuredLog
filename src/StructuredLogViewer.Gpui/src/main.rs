#![allow(dead_code)]
//! MSBuild Structured Log Viewer on Zed's GPUI.
//!
//! Native: the .NET engine behind the NativeAOT bridge (`libmslog.dylib`).
//! Web: the same engine as browser-wasm in a Web Worker; gpui renders to a
//! canvas over WebGPU.
//!
//! Usage (native): structured-log-viewer-gpui [path.binlog] [--search <query>] [--reveal <nodeId>] [--source <path>] [--line N] [--timeline]
//!                 [--pane search|properties|files|find|favorites] [--props <query>] [--find <term>]
//! Usage (web):    index.html?binlog=<url>&search=<query>&reveal=<nodeId>&source=<path>&line=N&timeline

mod engine;
mod favorites;
mod files_view;
mod icons;
mod inspector;
mod model;
mod msbuild;
mod scrollbar;
mod search_view;
mod source_view;
mod styling;
mod text_input;
mod theme;
mod timeline_view;
mod tree;
mod tree_view;
#[cfg(target_family = "wasm")]
mod web;
mod workspace;

use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use theme::Theme;
use workspace::{Launch, Quit, Workspace};

fn configure(cx: &mut App) {
    // gpui_web only embeds IBM Plex Sans and Lilex; the row glyphs (chevrons,
    // kind icons) need a symbol-rich fallback. Native uses system fonts.
    #[cfg(target_family = "wasm")]
    if let Err(err) = cx
        .text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!("../web/fonts/DejaVuSans.ttf").as_slice())])
    {
        web_sys::console::warn_1(&format!("symbol font: {err:#}").into());
    }
    cx.set_global(Theme::for_appearance(cx.window_appearance()));
    cx.bind_keys(workspace::key_bindings());
    cx.bind_keys(tree_view::key_bindings());
    cx.bind_keys(text_input::key_bindings());
    cx.bind_keys(source_view::key_bindings());
    cx.on_action(|_: &Quit, cx| cx.quit());
}

fn open_main_window(cx: &mut App, host: engine::Host, launch: Launch) -> gpui::WindowHandle<Workspace> {
    let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
    #[cfg(not(target_family = "wasm"))]
    let titlebar = Some(gpui::TitlebarOptions {
        title: Some("Structured Log Viewer".into()),
        appears_transparent: true,
        traffic_light_position: Some(gpui::point(px(12.), px(12.))),
    });
    #[cfg(target_family = "wasm")]
    let titlebar = None;

    cx.open_window(
        WindowOptions {
            titlebar,
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.), px(500.))),
            ..Default::default()
        },
        move |window, cx| {
            window
                .observe_window_appearance(|window, cx| {
                    cx.set_global(Theme::for_appearance(window.appearance()));
                    window.refresh();
                })
                .detach();
            cx.set_global(Theme::for_appearance(window.appearance()));
            cx.new(|cx| Workspace::new(host, launch, cx))
        },
    )
    .expect("open window")
}

// ---------------------------------------------------------------- native

#[cfg(not(target_family = "wasm"))]
fn main() {
    use gpui::{Menu, MenuItem};
    use std::path::PathBuf;
    use workspace::{
        CloseBuild, FocusSearch, FocusTree, OpenFile, ShowFavorites, ShowFiles, ShowFindInFiles,
        ShowProperties, ShowTimeline, ShowTree, ToggleInspector,
    };

    let mut launch = Launch::default();
    let mut path: Option<PathBuf> = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--search" => launch.search = iter.next(),
            "--reveal" => launch.reveal = iter.next(),
            "--source" => launch.source = iter.next(),
            "--line" => launch.line = iter.next().and_then(|l| l.parse().ok()),
            "--timeline" => launch.timeline = true,
            "--pane" => launch.pane = iter.next(),
            "--props" => launch.props = iter.next(),
            "--find" => launch.find = iter.next(),
            _ if arg.starts_with("--") => {}
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    let engine = match engine::Engine::locate().and_then(|p| engine::Engine::load(&p)) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("error: {err:#}");
            std::process::exit(1);
        }
    };
    eprintln!("libmslog {}", engine.version());

    gpui_platform::application().run(move |cx: &mut App| {
        configure(cx);
        cx.set_menus(vec![
            Menu::new("Structured Log Viewer").items([MenuItem::action("Quit", Quit)]),
            Menu::new("File").items([
                MenuItem::action("Open…", OpenFile),
                MenuItem::separator(),
                MenuItem::action("Close Build", CloseBuild),
            ]),
            Menu::new("View").items([
                MenuItem::action("Search Log", FocusSearch),
                MenuItem::action("Properties and items", ShowProperties),
                MenuItem::action("Files", ShowFiles),
                MenuItem::action("Find in Files", ShowFindInFiles),
                MenuItem::action("Favorites", ShowFavorites),
                MenuItem::separator(),
                MenuItem::action("Build Tree", FocusTree),
                MenuItem::action("Show Log Tree", ShowTree),
                MenuItem::action("Show Timeline", ShowTimeline),
                MenuItem::separator(),
                MenuItem::action("Toggle Inspector", ToggleInspector),
            ]),
        ]);

        let host = engine::Host { engine: engine.clone(), executor: cx.background_executor().clone() };
        let window = open_main_window(cx, host, launch);
        window
            .update(cx, |workspace, window, cx| {
                if let Some(path) = path {
                    let path = std::path::absolute(&path).unwrap_or(path);
                    workspace.open(engine::OpenSource::Path(path), cx);
                }
                window.focus(&gpui::Focusable::focus_handle(workspace, cx), cx);
                cx.activate(true);
            })
            .ok();
    });
}

// ---------------------------------------------------------------- web

#[cfg(target_family = "wasm")]
fn main() {
    use gpui::Application;
    use std::rc::Rc;

    console_error_panic_hook::set_once();
    gpui_web::init_logging();

    let query = web::query();
    let client = match engine::WorkerClient::new("engine-worker.js") {
        Ok(client) => client,
        Err(err) => {
            web_sys::console::error_1(&format!("{err:#}").into());
            return;
        }
    };

    // Single-threaded web platform; `Application::run` would return at once
    // and drop the App, so run embedded and keep the handle for the tab's
    // lifetime.
    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    let app = Application::with_platform(platform).with_http_client(http_client);
    let handle = app.run_embedded(move |cx: &mut App| {
        configure(cx);
        let host = engine::Host { client: client.clone() };
        let window = open_main_window(cx, host, query.launch);
        let workspace = window.update(cx, |_, _, cx| cx.entity()).expect("workspace");
        web::install_file_input(workspace, cx);
        window
            .update(cx, |workspace, window, cx| {
                if let Some(url) = query.binlog {
                    workspace.open(engine::OpenSource::Url(url), cx);
                }
                window.focus(&gpui::Focusable::focus_handle(workspace, cx), cx);
                cx.activate(true);
            })
            .ok();
    });
    std::mem::forget(handle);
}
