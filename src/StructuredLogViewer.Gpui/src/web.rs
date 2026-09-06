//! Browser glue: the hidden `<input type=file>` in web/index.html stands in
//! for the native open panel, and URL query parameters stand in for the
//! command line (`?binlog=sample.binlog&search=$warning&reveal=124`).

use crate::engine::OpenSource;
use crate::workspace::{Launch, Workspace};
use gpui::{App, Entity};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlInputElement;

const FILE_INPUT_ID: &str = "binlog-file";

pub struct Query {
    pub binlog: Option<String>,
    pub launch: Launch,
}

pub fn query() -> Query {
    let mut q = Query { binlog: None, launch: Launch::default() };
    let Some(window) = web_sys::window() else { return q };
    let Ok(search) = window.location().search() else { return q };
    let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) else { return q };
    q.binlog = params.get("binlog");
    q.launch.search = params.get("search");
    q.launch.reveal = params.get("reveal");
    q.launch.source = params.get("source");
    q.launch.line = params.get("line").and_then(|l| l.parse().ok());
    q.launch.timeline = params.get("timeline").is_some();
    q
}

fn file_input() -> Option<HtmlInputElement> {
    web_sys::window()?.document()?.get_element_by_id(FILE_INPUT_ID)?.dyn_into().ok()
}

/// The "Open…" affordance on the web: click the hidden file input.
pub fn click_file_input() {
    if let Some(input) = file_input() {
        input.click();
    }
}

/// Routes a chosen file to the workspace. The listener owns an AsyncApp so
/// it can re-enter gpui from a DOM event.
pub fn install_file_input(workspace: Entity<Workspace>, cx: &mut App) {
    let Some(input) = file_input() else { return };
    let async_cx = cx.to_async();
    let closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        let Some(input) = file_input() else { return };
        let Some(file) = input.files().and_then(|files| files.get(0)) else { return };
        // Allow re-selecting the same file later.
        input.set_value("");
        let workspace = workspace.clone();
        async_cx.update(|cx| {
            workspace.update(cx, |ws, cx| ws.open(OpenSource::File(file), cx));
        });
    }) as Box<dyn FnMut(web_sys::Event)>);
    input.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref()).ok();
    closure.forget();
}
