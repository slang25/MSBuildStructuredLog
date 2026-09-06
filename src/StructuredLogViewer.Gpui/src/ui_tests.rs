//! Headless UI tests: the real views in a gpui test window, driven with
//! real keystrokes and mouse events, against the real engine and the
//! checked-in binlog. No screen, no OS event queue, deterministic executor.
//!
//! They need `libmslog.dylib` (build it with the bridge's build-dylib.sh);
//! without it each test says so and passes vacuously.

use crate::automation;
use crate::engine::{Engine, Host, OpenSource, Session};
use crate::files_view::FilesView;
use crate::inspector::Inspector;
use crate::source_view::SourceWell;
use crate::theme::Theme;
use crate::workspace::{Launch, Workspace};
use gpui::{AppContext as _, Modifiers, TestAppContext, point, px};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// The checked-in log, or whatever `MSLOG_TEST_BINLOG` points at, to
/// reproduce something seen on a real log headlessly.
fn binlog() -> PathBuf {
    match std::env::var_os("MSLOG_TEST_BINLOG") {
        Some(path) => PathBuf::from(path),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../StructuredLogger.Tests/msbuild.binlog"),
    }
}

/// One engine for the whole test process: the NativeAOT runtime inside
/// the dylib must be initialised once, not by every test thread at once.
fn engine() -> Option<Arc<Engine>> {
    static ENGINE: OnceLock<Option<Arc<Engine>>> = OnceLock::new();
    ENGINE
        .get_or_init(|| match Engine::locate() {
            Ok(path) => Some(Engine::load(&path).expect("load libmslog")),
            Err(err) => {
                eprintln!("skipping UI tests: {err:#}");
                None
            }
        })
        .clone()
}

/// UI tests run one at a time: two sessions opening on the engine from
/// two threads has crashed the bridge (SIGSEGV) in parallel runs.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A session on the checked-in binlog, or `None` where the bridge has not
/// been built.
fn open_session(cx: &mut TestAppContext) -> Option<Arc<Session>> {
    let engine = engine()?;
    let host = Host { engine, executor: cx.executor() };
    // The engine answers on real threads the deterministic executor
    // cannot see; let block_on wait for them.
    cx.executor().allow_parking();
    let progress = Arc::new(AtomicU64::new(0));
    let session = cx.foreground_executor().block_test(Session::open(host, OpenSource::Path(binlog()), progress)).expect("open the test binlog");
    Some(Arc::new(session))
}

/// What `main` does before the first window: keymaps, theme, probes.
fn configure(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Theme::for_appearance(cx.window_appearance()));
        cx.bind_keys(crate::workspace::key_bindings());
        cx.bind_keys(crate::tree_view::key_bindings());
        cx.bind_keys(crate::text_input::key_bindings());
        cx.bind_keys(crate::source_view::key_bindings());
    });
    automation::enable();
}

fn source_files(cx: &mut TestAppContext, session: &Arc<Session>) -> Vec<String> {
    let list = cx.foreground_executor().block_test(session.files_list()).expect("files list");
    list.files.into_iter().map(|f| f.path).collect()
}

#[gpui::test]
fn find_bar_opens_on_cmd_f_and_steps_through_matches(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some(session) = open_session(cx) else { return };
    configure(cx);
    // A file that says "import" more than once, so stepping has somewhere
    // to go.
    let path = source_files(cx, &session)
        .into_iter()
        .filter(|p| p.ends_with(".props") || p.ends_with(".targets"))
        .find(|p| {
            cx.foreground_executor()
                .block_test(session.read_file(p))
                .map(|text| text.to_ascii_lowercase().matches("import").count() >= 2)
                .unwrap_or(false)
        })
        .expect("the test binlog embeds a build file mentioning imports");

    let (well, cx) = cx.add_window_view(|_, cx| {
        let inspector = cx.new(|_| Inspector::new(session.clone()));
        SourceWell::new(session.clone(), inspector, cx)
    });
    well.update_in(cx, |well, _, cx| well.open_file(path.clone(), None, None, cx));
    cx.run_until_parked();
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    assert_eq!(state["tabs"].as_array().map(Vec::len), Some(1), "{state}");
    assert!(state["tabs"][0]["lines"].as_u64().unwrap() > 0, "file text loaded: {state}");

    // ⌘F from the editor opens the well's find bar, not the log search,
    // even though the workspace binds the same chord globally.
    well.update_in(cx, |well, window, cx| well.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-f");
    cx.simulate_input("import");
    cx.run_until_parked();
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    let find = &state["find"];
    assert_eq!(find["query"], "import", "{state}");
    let matches = find["matches"].as_u64().expect("match count");
    assert!(matches > 0, "a build file mentions imports: {state}");
    assert_eq!(state["selection"]["text"].as_str().map(str::to_ascii_lowercase).as_deref(), Some("import"));

    // Enter steps forward, wrapping; Escape closes and clears the wash.
    let before = find["current"].as_u64().unwrap();
    cx.simulate_keystrokes("enter");
    let after = well.read_with(cx, |well, cx| well.describe(cx))["find"]["current"].as_u64().unwrap();
    assert_eq!(after, (before + 1) % matches);

    cx.simulate_keystrokes("escape");
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    assert!(state["find"].is_null(), "escape closes the find bar: {state}");
}

#[gpui::test]
fn clicking_a_condition_pill_flips_it_to_the_evaluated_form(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some(session) = open_session(cx) else { return };
    configure(cx);

    // Any file with an import MSBuild skipped on a false condition.
    let mut target = None;
    for path in source_files(cx, &session).into_iter().take(80) {
        let Ok(file) = cx.foreground_executor().block_test(session.semantic_file(&path, None)) else { continue };
        if let Some(skipped) = file.skipped_imports.iter().find(|s| s.evaluated_condition.is_some()) {
            target = Some((path, skipped.line));
            break;
        }
    }
    let Some((path, line)) = target else {
        eprintln!("skipping: the test binlog has no skipped conditional import");
        return;
    };

    let (well, cx) = cx.add_window_view(|_, cx| {
        let inspector = cx.new(|_| Inspector::new(session.clone()));
        SourceWell::new(session.clone(), inspector, cx)
    });
    well.update_in(cx, |well, _, cx| well.open_file(path.clone(), Some(line), None, cx));
    cx.run_until_parked();
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    let inlays = state["tabs"][0]["inlayLines"].as_array().expect("inlay lines").clone();
    assert!(!inlays.is_empty(), "the skipped import produced a pill: {state}");
    let pill_line = inlays[0].as_u64().unwrap() as usize;

    let bounds = cx.debug_bounds(Box::leak(format!("pill-{pill_line}").into_boxed_str())).expect("the pill is laid out");
    // The left padding: never a token, so the click reaches the pill.
    cx.simulate_click(point(bounds.origin.x + px(3.), bounds.center().y), Modifiers::default());
    cx.run_until_parked();
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    assert_eq!(state["tabs"][0]["evaluatedPillLines"], serde_json::json!([pill_line]), "{state}");

    cx.simulate_click(point(bounds.origin.x + px(3.), bounds.center().y), Modifiers::default());
    cx.run_until_parked();
    let state = well.read_with(cx, |well, cx| well.describe(cx));
    assert_eq!(state["tabs"][0]["evaluatedPillLines"], serde_json::json!([]), "a second click restores the source: {state}");
}

#[gpui::test]
fn files_tree_starts_fully_expanded(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some(session) = open_session(cx) else { return };
    configure(cx);
    let (files, cx) = cx.add_window_view(|_, cx| FilesView::new(session.clone(), cx));
    cx.run_until_parked();
    let state = files.read_with(cx, |files, cx| files.describe(cx));
    assert!(state["total"].as_u64().unwrap() > 0, "the test binlog embeds sources: {state}");
    let rows = state["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["depth"].as_u64().unwrap() > 0), "nested rows are visible: {state}");
    assert!(
        rows.iter().filter(|r| r["file"] == false).all(|r| r["expanded"] == true),
        "every folder starts open: {state}"
    );
}

/// The whole workspace, loaded, with the log search focused as at launch.
fn open_workspace(cx: &mut TestAppContext) -> Option<(gpui::Entity<Workspace>, &mut gpui::VisualTestContext)> {
    let engine = engine()?;
    configure(cx);
    cx.executor().allow_parking();
    let host = Host { engine, executor: cx.executor() };
    let (workspace, cx) = cx.add_window_view(|_, cx| Workspace::new(host, Launch::default(), cx));
    workspace.update_in(cx, |workspace, _, cx| workspace.open(OpenSource::Path(binlog()), cx));
    cx.run_until_parked();
    let phase = workspace.update_in(cx, |w, window, cx| w.describe(window, cx))["phase"].clone();
    assert_eq!(phase, "loaded", "the test binlog opens");
    Some((workspace, cx))
}

#[gpui::test]
fn cmd_f_reaches_the_log_search_from_the_workspace_root(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some((workspace, cx)) = open_workspace(cx) else { return };
    // Focus on the bare root, as right after launch: no pane, no editor.
    workspace.update_in(cx, |workspace, window, cx| window.focus(&gpui::Focusable::focus_handle(workspace, cx), cx));
    cx.simulate_keystrokes("cmd-f");
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["focus"], "search-input", "{state}");

    cx.simulate_input("csc");
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["search"]["query"], "csc", "{state}");
}

#[gpui::test]
fn clicking_a_source_tab_keeps_focus_in_the_well(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some(session) = open_session(cx) else { return };
    configure(cx);
    let files = source_files(cx, &session);
    let (well, cx) = cx.add_window_view(|_, cx| {
        let inspector = cx.new(|_| Inspector::new(session.clone()));
        SourceWell::new(session.clone(), inspector, cx)
    });
    for path in files.iter().take(2) {
        well.update_in(cx, |well, _, cx| well.open_file(path.clone(), None, None, cx));
        cx.run_until_parked();
    }
    well.update_in(cx, |well, window, cx| well.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-f");
    cx.simulate_input("a");
    cx.run_until_parked();

    let tab = automation::bounds("source-tab-0").expect("first tab laid out");
    cx.simulate_click(tab.center(), Modifiers::default());
    cx.run_until_parked();
    let focused = well.update_in(cx, |well, window, cx| well.focus_kind(window, cx));
    assert!(focused.is_some(), "focus stays in the well after a tab click");

    // ⌘G still steps the find because the well's context is on the path.
    let before = well.read_with(cx, |well, cx| well.describe(cx))["find"]["current"].clone();
    cx.simulate_keystrokes("cmd-g");
    let after = well.read_with(cx, |well, cx| well.describe(cx))["find"]["current"].clone();
    assert!(before.is_null() || before != after, "cmd-g stepped: {before} -> {after}");
}

#[gpui::test]
fn revealing_an_evaluation_selects_it_in_the_tree(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some((workspace, cx)) = open_workspace(cx) else { return };
    let session = workspace.read_with(cx, |w, _| w.session()).expect("loaded");
    let tree = workspace.update_in(cx, |w, window, cx| w.describe(window, cx))["tree"].clone();
    let folder = tree["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["title"] == "Evaluation")
        .expect("an Evaluation folder row")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let evaluation = cx
        .foreground_executor()
        .block_test(session.all_children(&folder, 0))
        .expect("evaluations")
        .into_iter()
        .find(|n| n.kind == "ProjectEvaluation")
        .expect("an evaluation")
        .id;
    let chain = cx.foreground_executor().block_test(session.ancestors(&evaluation)).expect("ancestors").chain;
    let chain_ids: Vec<String> = chain.iter().map(|n| n.id.clone()).collect();

    workspace.update_in(cx, |w, _, cx| w.reveal(evaluation.clone(), cx));
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    let tree = &state["tree"];
    let selected = tree["selected"].as_u64().map(|ix| tree["rows"][ix as usize]["id"].clone());
    assert_eq!(selected.as_ref().and_then(|v| v.as_str()), Some(evaluation.as_str()), "chain {chain_ids:?}: {tree}");
    assert_eq!(state["focus"], "tree", "{state}");
}

#[gpui::test]
fn hiding_the_inspector_moves_focus_out_of_it(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some((workspace, cx)) = open_workspace(cx) else { return };
    let session = workspace.read_with(cx, |w, _| w.session()).expect("loaded");
    let path = source_files(cx, &session).into_iter().next().expect("a source file");
    // Open a file the way a sidebar does, then take the editor's focus.
    workspace.update_in(cx, |w, window, cx| {
        w.open_source(path, None, cx);
        w.describe(window, cx)
    });
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["focus"], "editor", "{state}");

    cx.simulate_keystrokes("cmd-alt-i");
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["inspectorVisible"], false, "{state}");
    assert_ne!(state["focus"], "editor", "focus must leave the hidden well: {state}");

    // And the shortcut keeps working from wherever focus went.
    cx.simulate_keystrokes("cmd-alt-i");
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["inspectorVisible"], true, "{state}");
}

#[gpui::test]
fn the_picker_menu_reveals_the_evaluation_in_the_tree(cx: &mut TestAppContext) {
    let _serial = serial();
    let Some((workspace, cx)) = open_workspace(cx) else { return };
    let session = workspace.read_with(cx, |w, _| w.session()).expect("loaded");
    // The most widely imported file: its menu is the one that has to
    // scroll to reach the reveal item.
    let path = source_files(cx, &session)
        .into_iter()
        .filter(|p| p.ends_with(".props") || p.ends_with(".targets"))
        .take(60)
        .map(|p| {
            let contexts = cx.foreground_executor().block_test(session.semantic_file(&p, None)).map(|f| f.contexts_total).unwrap_or(0);
            (contexts, p)
        })
        .max()
        .map(|(_, p)| p)
        .expect("a build file");
    workspace.update_in(cx, |w, _, cx| w.open_source(path, None, cx));
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    let evaluation = state["well"]["tabs"][0]["evaluation"].as_str().expect("an evaluation context").to_string();

    let picker = automation::bounds("context-picker").expect("picker laid out");
    cx.simulate_click(picker.center(), Modifiers::default());
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    assert_eq!(state["well"]["contextMenuOpen"], true, "{state}");

    let item = automation::bounds("reveal-context").expect("menu item laid out");
    cx.simulate_click(item.center(), Modifiers::default());
    cx.run_until_parked();
    let state = workspace.update_in(cx, |w, window, cx| w.describe(window, cx));
    let tree = &state["tree"];
    let selected = tree["selected"].as_u64().map(|ix| tree["rows"][ix as usize]["id"].clone());
    assert_eq!(state["well"]["contextMenuOpen"], false, "{state}");
    assert_eq!(selected.as_ref().and_then(|v| v.as_str()), Some(evaluation.as_str()), "{tree}");
}
