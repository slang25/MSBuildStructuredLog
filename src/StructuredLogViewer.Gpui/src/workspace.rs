//! The window: title bar strip under the transparent system title bar,
//! then sidebar | build tree | inspector with draggable dividers. Owns the
//! open build and the load lifecycle.
//!
//! Either side panel collapses, three ways: the panel buttons at the ends
//! of the title bar, ⌘B / ⌘⌥I, or the divider itself — double-click it, or
//! drag the panel below its minimum width. A collapsed panel keeps its
//! width for when it comes back, and its divider stays against the window
//! edge so the mouse has a way to drag it out again.
//!
//! The sidebar carries the same five panes as the WPF and Avalonia viewers,
//! on a tab strip along its bottom edge: Search Log, Properties and items,
//! Files, Find in Files and Favorites. The two file panes only exist when
//! the log embedded a source archive, and every pane but Search Log is
//! built the first time it is shown.

use crate::engine::{Host, OpenSource, Session};
use crate::favorites::{Favorites, FavoritesEvent, FavoritesView};
use crate::files_view::{FilesEvent, FilesView, FindInFilesView};
use crate::inspector::Inspector;
use crate::model::{BuildInfo, format_bytes, format_duration};
use crate::search_view::{SearchEvent, SearchMode, SearchView};
use crate::source_view::{SourceEvent, SourceWell};
use crate::theme::Theme;
use crate::timeline_view::{TimelineEvent, TimelineView};
use crate::tree_view::{TreeEvent, TreeView};
#[cfg(not(target_family = "wasm"))]
use gpui::PathPromptOptions;
use gpui::{
    App, ClickEvent, Context, CursorStyle, Entity, ExternalPaths, FocusHandle, Focusable, FontWeight,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Window, actions, div, prelude::*, px,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

actions!(
    viewer,
    [
        OpenFile,
        CloseBuild,
        FocusSearch,
        FocusTree,
        ToggleSidebar,
        ToggleInspector,
        Quit,
        ShowTree,
        ShowTimeline,
        ShowProperties,
        ShowFiles,
        ShowFindInFiles,
        ShowFavorites
    ]
);

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding as K;
    vec![
        K::new("cmd-o", OpenFile, None),
        K::new("cmd-w", CloseBuild, None),
        // Inside the source well ⌘F finds in the file instead; a bare
        // binding would outrank the well's (gpui gives it maximum depth).
        K::new("cmd-f", FocusSearch, Some("!SourceWell")),
        K::new("cmd-shift-t", FocusTree, None),
        // ⌘B is the sidebar everywhere else; the inspector keeps ⌘⌥I.
        K::new("cmd-b", ToggleSidebar, None),
        K::new("cmd-alt-i", ToggleInspector, None),
        K::new("cmd-q", Quit, None),
        K::new("cmd-1", ShowTree, None),
        K::new("cmd-2", ShowTimeline, None),
        K::new("cmd-shift-p", ShowProperties, None),
        K::new("cmd-shift-e", ShowFiles, None),
        K::new("cmd-shift-f", ShowFindInFiles, None),
        K::new("cmd-shift-d", ShowFavorites, None),
    ]
}

const TITLEBAR_HEIGHT: f32 = 38.;

/// Debug-only launch arguments, mirroring the Mac app's -reveal/-source.
#[derive(Default, Clone)]
pub struct Launch {
    pub search: Option<String>,
    pub reveal: Option<String>,
    pub source: Option<String>,
    pub line: Option<usize>,
    pub timeline: bool,
    /// Sidebar pane to open: search | properties | files | find | favorites.
    pub pane: Option<String>,
    /// Query for the Properties and items pane (needs --reveal for scope).
    pub props: Option<String>,
    /// Term for the Find in Files pane.
    pub find: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailMode {
    Tree,
    Timeline,
}

/// The sidebar panes, in tab-strip order (the WPF `leftPaneTabControl`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    SearchLog,
    PropertiesAndItems,
    Files,
    FindInFiles,
    Favorites,
}

impl SidebarTab {
    fn parse(name: &str) -> Option<SidebarTab> {
        match name {
            "search" | "log" => Some(SidebarTab::SearchLog),
            "properties" | "props" => Some(SidebarTab::PropertiesAndItems),
            "files" => Some(SidebarTab::Files),
            "find" | "findinfiles" => Some(SidebarTab::FindInFiles),
            "favorites" => Some(SidebarTab::Favorites),
            _ => None,
        }
    }

    /// The strip is narrow; the full names live in the View menu.
    fn label(self) -> &'static str {
        match self {
            SidebarTab::SearchLog => "Search",
            SidebarTab::PropertiesAndItems => "Properties",
            SidebarTab::Files => "Files",
            SidebarTab::FindInFiles => "Find",
            SidebarTab::Favorites => "Favorites",
        }
    }

    fn element_id(self) -> &'static str {
        match self {
            SidebarTab::SearchLog => "tab-search",
            SidebarTab::PropertiesAndItems => "tab-properties",
            SidebarTab::Files => "tab-files",
            SidebarTab::FindInFiles => "tab-find-in-files",
            SidebarTab::Favorites => "tab-favorites",
        }
    }

    /// Files and Find in Files need the log to carry a source archive.
    fn needs_archive(self) -> bool {
        matches!(self, SidebarTab::Files | SidebarTab::FindInFiles)
    }
}

struct Loaded {
    session: Arc<Session>,
    info: BuildInfo,
    tree: Entity<TreeView>,
    search: Entity<SearchView>,
    properties: Entity<SearchView>,
    favorites: Entity<Favorites>,
    favorites_view: Entity<FavoritesView>,
    files: Option<Entity<FilesView>>,
    find_in_files: Option<Entity<FindInFilesView>>,
    inspector: Entity<Inspector>,
    well: Entity<SourceWell>,
    timeline: Option<Entity<TimelineView>>,
}

enum Phase {
    Welcome,
    Loading { name: String, progress: Arc<AtomicU64> },
    Failed { name: String, message: String },
    Loaded(Loaded),
}

/// A divider, and by extension the panel it sizes.
#[derive(Clone, Copy, PartialEq)]
enum Divider {
    Sidebar,
    Inspector,
}

impl Divider {
    /// How narrow and how wide the panel may be dragged.
    fn range(self) -> (f32, f32) {
        match self {
            Divider::Sidebar => (220., 700.),
            Divider::Inspector => (240., 1100.),
        }
    }

    /// Dragged narrower than this, the panel collapses instead of
    /// clamping; dragging back out past it brings it back at its minimum.
    fn collapse_at(self) -> f32 {
        self.range().0 * 0.7
    }

    fn element_id(self) -> &'static str {
        match self {
            Divider::Sidebar => "divider-sidebar",
            Divider::Inspector => "divider-inspector",
        }
    }

    fn toggle_id(self) -> &'static str {
        match self {
            Divider::Sidebar => "toggle-sidebar",
            Divider::Inspector => "toggle-inspector",
        }
    }
}

struct Drag {
    divider: Divider,
    start_x: Pixels,
    start_width: f32,
}

pub struct Workspace {
    host: Host,
    phase: Phase,
    focus_handle: FocusHandle,
    sidebar_width: f32,
    inspector_width: f32,
    sidebar_visible: bool,
    inspector_visible: bool,
    drag: Option<Drag>,
    generation: u64,
    launch: Launch,
    mode: DetailMode,
    sidebar_tab: SidebarTab,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Workspace {
    pub fn new(host: Host, launch: Launch, cx: &mut Context<Self>) -> Self {
        let mode = if launch.timeline { DetailMode::Timeline } else { DetailMode::Tree };
        Workspace {
            host,
            phase: Phase::Welcome,
            focus_handle: cx.focus_handle(),
            sidebar_width: 340.,
            inspector_width: 520.,
            sidebar_visible: true,
            inspector_visible: true,
            drag: None,
            generation: 0,
            launch,
            mode,
            sidebar_tab: SidebarTab::SearchLog,
        }
    }

    /// Everything an `--automation` dump or a test wants to know.
    pub fn describe(&self, window: &Window, cx: &App) -> serde_json::Value {
        let focus = if let Phase::Loaded(loaded) = &self.phase {
            let focused = |handle: FocusHandle| handle.is_focused(window);
            loaded
                .well
                .read(cx)
                .focus_kind(window, cx)
                .map(str::to_string)
                .or_else(|| focused(loaded.tree.read(cx).focus_handle(cx)).then(|| "tree".to_string()))
                .or_else(|| focused(loaded.search.read(cx).focus_handle(cx)).then(|| "search-input".to_string()))
                .or_else(|| focused(loaded.properties.read(cx).focus_handle(cx)).then(|| "properties-input".to_string()))
                .or_else(|| loaded.files.as_ref().filter(|f| focused(f.read(cx).focus_handle(cx))).map(|_| "files-filter".to_string()))
                .or_else(|| loaded.find_in_files.as_ref().filter(|f| focused(f.read(cx).focus_handle(cx))).map(|_| "find-in-files-input".to_string()))
                .or_else(|| self.focus_handle.is_focused(window).then(|| "workspace".to_string()))
                .unwrap_or_else(|| "other".to_string())
        } else {
            "n/a".to_string()
        };
        let phase = match &self.phase {
            Phase::Welcome => "welcome",
            Phase::Loading { .. } => "loading",
            Phase::Failed { .. } => "failed",
            Phase::Loaded(_) => "loaded",
        };
        let mut value = serde_json::json!({
            "phase": phase,
            "focus": focus,
            "sidebar": self.sidebar_tab.element_id(),
            "sidebarVisible": self.sidebar_visible,
            "mode": match self.mode { DetailMode::Tree => "tree", DetailMode::Timeline => "timeline" },
            "inspectorVisible": self.inspector_visible,
        });
        if let Phase::Failed { message, .. } = &self.phase {
            value["error"] = serde_json::Value::String(message.clone());
        }
        if let Phase::Loaded(loaded) = &self.phase {
            value["well"] = loaded.well.read(cx).describe(cx);
            value["tree"] = loaded.tree.read(cx).describe();
            value["search"] = loaded.search.read(cx).describe(cx);
            value["properties"] = loaded.properties.read(cx).describe(cx);
            value["files"] = loaded.files.as_ref().map(|f| f.read(cx).describe(cx)).unwrap_or(serde_json::Value::Null);
        }
        value
    }

    // ----- lifecycle -----

    pub fn open(&mut self, source: OpenSource, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let progress = Arc::new(AtomicU64::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let name = source.display_name();
        self.phase = Phase::Loading { name: name.clone(), progress: progress.clone() };
        cx.notify();

        // Repaint while the bridge reads; the callback fires on its thread
        // and only touches the atomic.
        let poll_done = done.clone();
        cx.spawn(async move |this, cx| {
            while !poll_done.load(Ordering::Relaxed) {
                cx.background_executor().timer(Duration::from_millis(60)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let session = Session::open(host, source, progress).await?;
                let info = session.info().await?;
                let root = session.node(&info.root_id).await?.node;
                anyhow::Ok((session, info, root))
            }
            .await;
            done.store(true, Ordering::Relaxed);
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok((session, info, root)) => this.attach(Arc::new(session), info, root, cx),
                    Err(err) => {
                        this.phase = Phase::Failed { name, message: format!("{err:#}") };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn attach(&mut self, session: Arc<Session>, info: BuildInfo, root: crate::model::NodeSummary, cx: &mut Context<Self>) {
        let favorites = cx.new(|_| Favorites::default());
        let tree = cx.new(|cx| TreeView::new(session.clone(), root, favorites.clone(), cx));
        let search = cx.new(|cx| SearchView::new(session.clone(), SearchMode::Log, cx));
        let properties = cx.new(|cx| SearchView::new(session.clone(), SearchMode::PropertiesAndItems, cx));
        let favorites_view = cx.new(|cx| FavoritesView::new(favorites.clone(), cx));
        let inspector = cx.new(|_| Inspector::new(session.clone()));
        let well = cx.new(|cx| SourceWell::new(session.clone(), inspector.clone(), cx));

        cx.subscribe(&tree, |this, _tree, event, cx| match event {
            TreeEvent::Selected(node) => {
                if let Phase::Loaded(loaded) = &this.phase {
                    let id = node.id.clone();
                    loaded.inspector.update(cx, |inspector, cx| inspector.show(id, cx));
                }
            }
            TreeEvent::ViewSource(node) => {
                if let Phase::Loaded(loaded) = &this.phase {
                    let id = node.id.clone();
                    this.inspector_visible = true;
                    loaded.well.update(cx, |well, cx| well.open_node_source(id, cx));
                    cx.notify();
                }
            }
            TreeEvent::ProjectContext(context) => {
                if let Phase::Loaded(loaded) = &this.phase {
                    let context = context.clone();
                    loaded.properties.update(cx, |properties, cx| properties.set_context(context, cx));
                }
            }
            TreeEvent::Preprocess(node) => {
                if let Phase::Loaded(loaded) = &this.phase {
                    let id = node.id.clone();
                    let title = node
                        .prop("projectFile")
                        .map(crate::msbuild::file_name)
                        .or_else(|| node.name.clone())
                        .unwrap_or_else(|| node.title.clone());
                    this.inspector_visible = true;
                    loaded.well.update(cx, |well, cx| well.open_preprocessed(id, title, cx));
                    cx.notify();
                }
            }
            TreeEvent::AppendSearch(clause) => this.search_from_tree(Some(clause.clone()), None, cx),
            TreeEvent::SetSearch(query) => this.search_from_tree(None, Some(query.clone()), cx),
            TreeEvent::ShowInTimeline(_) => {
                this.mode = DetailMode::Timeline;
                this.ensure_timeline(cx);
                cx.notify();
            }
        })
        .detach();

        for pane in [&search, &properties] {
            cx.subscribe(pane, |this, _search, event, cx| match event {
                SearchEvent::Reveal(id) => this.reveal(id.clone(), cx),
            })
            .detach();
        }

        cx.subscribe(&favorites_view, |this, _view, event, cx| match event {
            FavoritesEvent::Reveal(id) => this.reveal(id.clone(), cx),
        })
        .detach();

        cx.subscribe(&well, |this, _well, event, cx| match event {
            SourceEvent::Reveal(id) => this.reveal(id.clone(), cx),
        })
        .detach();

        let launch = std::mem::take(&mut self.launch);
        if let Some(query) = launch.search {
            search.update(cx, |search, cx| search.run_query(&query, cx));
        }
        if let Some(id) = launch.reveal {
            tree.update(cx, |tree, cx| tree.reveal(id, cx));
        }
        if let Some(path) = launch.source {
            let line = launch.line;
            well.update(cx, |well, cx| well.open_file(path, line, None, cx));
        }
        if let Some(query) = &launch.props {
            // The context arrives with --reveal; set_context re-runs this.
            properties.update(cx, |properties, cx| properties.run_query(query, cx));
        }

        self.phase = Phase::Loaded(Loaded {
            session: session.clone(),
            info,
            tree,
            search,
            properties,
            favorites,
            favorites_view,
            files: None,
            find_in_files: None,
            inspector,
            well,
            timeline: None,
        });
        if self.mode == DetailMode::Timeline {
            self.ensure_timeline(cx);
        }
        if let Some(tab) = launch.pane.as_deref().and_then(SidebarTab::parse) {
            self.select_pane(tab, cx);
        } else if launch.props.is_some() {
            self.select_pane(SidebarTab::PropertiesAndItems, cx);
        } else if launch.find.is_some() {
            self.select_pane(SidebarTab::FindInFiles, cx);
        }
        if let Some(term) = &launch.find {
            self.ensure_find_in_files(cx);
            if let Phase::Loaded(loaded) = &self.phase
                && let Some(find) = loaded.find_in_files.clone()
            {
                find.update(cx, |find, cx| find.run_query(term, cx));
            }
        }
    }

    /// The tree's context menu composing a query: `append` adds a clause to
    /// whatever is in the box, `replace` starts over. Either way the Search
    /// Log pane comes forward.
    fn search_from_tree(&mut self, append: Option<String>, replace: Option<String>, cx: &mut Context<Self>) {
        self.select_pane(SidebarTab::SearchLog, cx);
        let Phase::Loaded(loaded) = &self.phase else { return };
        loaded.search.update(cx, |search, cx| match (&append, &replace) {
            (Some(clause), _) => search.append_query(clause, cx),
            (_, Some(query)) => search.run_query(query, cx),
            _ => {}
        });
    }

    /// The session behind the loaded build, for tests.
    #[cfg(test)]
    pub(crate) fn session(&self) -> Option<Arc<Session>> {
        match &self.phase {
            Phase::Loaded(loaded) => Some(loaded.session.clone()),
            _ => None,
        }
    }

    pub(crate) fn reveal(&mut self, id: String, cx: &mut Context<Self>) {
        self.mode = DetailMode::Tree;
        if let Phase::Loaded(loaded) = &self.phase {
            loaded.tree.update(cx, |tree, cx| tree.reveal(id, cx));
        }
        cx.notify();
    }

    fn ensure_timeline(&mut self, cx: &mut Context<Self>) {
        let Phase::Loaded(loaded) = &mut self.phase else { return };
        if loaded.timeline.is_some() {
            return;
        }
        let timeline = cx.new(|cx| TimelineView::new(loaded.session.clone(), cx));
        cx.subscribe(&timeline, |this, _timeline, event, cx| match event {
            TimelineEvent::Reveal(id) => this.reveal(id.clone(), cx),
        })
        .detach();
        loaded.timeline = Some(timeline);
    }

    // ----- sidebar panes -----

    fn has_archive(&self) -> bool {
        matches!(&self.phase, Phase::Loaded(loaded) if loaded.info.has_source_archive)
    }

    fn visible_tabs(&self) -> Vec<SidebarTab> {
        use SidebarTab::*;
        let archive = self.has_archive();
        [SearchLog, PropertiesAndItems, Files, FindInFiles, Favorites]
            .into_iter()
            .filter(|tab| archive || !tab.needs_archive())
            .collect()
    }

    /// Switches panes, building the file panes on first use — `files_list`
    /// and the search index behind Find in Files are worth deferring.
    fn select_pane(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        if tab.needs_archive() && !self.has_archive() {
            return;
        }
        self.sidebar_tab = tab;
        // Asking for a pane asks for the sidebar it lives in.
        self.sidebar_visible = true;
        match tab {
            SidebarTab::Files => self.ensure_files(cx),
            SidebarTab::FindInFiles => self.ensure_find_in_files(cx),
            _ => {}
        }
        cx.notify();
    }

    fn show_pane(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_pane(tab, cx);
        self.focus_pane(self.sidebar_tab, window, cx);
    }

    fn focus_pane(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        let Phase::Loaded(loaded) = &self.phase else { return };
        let handle = match tab {
            SidebarTab::SearchLog => Some(loaded.search.read(cx).focus_handle(cx)),
            SidebarTab::PropertiesAndItems => Some(loaded.properties.read(cx).focus_handle(cx)),
            SidebarTab::Files => loaded.files.as_ref().map(|f| f.read(cx).focus_handle(cx)),
            SidebarTab::FindInFiles => loaded.find_in_files.as_ref().map(|f| f.read(cx).focus_handle(cx)),
            SidebarTab::Favorites => None,
        };
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }

    fn ensure_files(&mut self, cx: &mut Context<Self>) {
        let Phase::Loaded(loaded) = &self.phase else { return };
        if loaded.files.is_some() {
            return;
        }
        let session = loaded.session.clone();
        let files = cx.new(|cx| FilesView::new(session, cx));
        cx.subscribe(&files, Self::on_files_event).detach();
        if let Phase::Loaded(loaded) = &mut self.phase {
            loaded.files = Some(files);
        }
    }

    fn ensure_find_in_files(&mut self, cx: &mut Context<Self>) {
        let Phase::Loaded(loaded) = &self.phase else { return };
        if loaded.find_in_files.is_some() {
            return;
        }
        let session = loaded.session.clone();
        let find = cx.new(|cx| FindInFilesView::new(session, cx));
        cx.subscribe(&find, Self::on_files_event).detach();
        if let Phase::Loaded(loaded) = &mut self.phase {
            loaded.find_in_files = Some(find);
        }
    }

    fn on_files_event<T: 'static>(&mut self, _view: Entity<T>, event: &FilesEvent, cx: &mut Context<Self>) {
        let FilesEvent::Open { path, line } = event;
        self.open_source(path.clone(), *line, cx);
    }

    /// Show `path` in the source well, as a sidebar row does.
    pub(crate) fn open_source(&mut self, path: String, line: Option<usize>, cx: &mut Context<Self>) {
        let Phase::Loaded(loaded) = &self.phase else { return };
        self.inspector_visible = true;
        loaded.well.update(cx, |well, cx| well.open_file(path, line, None, cx));
        cx.notify();
    }

    fn show_properties(&mut self, _: &ShowProperties, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pane(SidebarTab::PropertiesAndItems, window, cx);
    }

    fn show_files(&mut self, _: &ShowFiles, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pane(SidebarTab::Files, window, cx);
    }

    fn show_find_in_files(&mut self, _: &ShowFindInFiles, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pane(SidebarTab::FindInFiles, window, cx);
    }

    fn show_favorites(&mut self, _: &ShowFavorites, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pane(SidebarTab::Favorites, window, cx);
    }

    fn show_tree(&mut self, _: &ShowTree, _window: &mut Window, cx: &mut Context<Self>) {
        self.mode = DetailMode::Tree;
        cx.notify();
    }

    fn show_timeline(&mut self, _: &ShowTimeline, _window: &mut Window, cx: &mut Context<Self>) {
        self.mode = DetailMode::Timeline;
        self.ensure_timeline(cx);
        cx.notify();
    }

    #[cfg(not(target_family = "wasm"))]
    fn open_file(&mut self, _: &OpenFile, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.open(OpenSource::Path(path), cx)).ok();
            }
        })
        .detach();
    }

    #[cfg(target_family = "wasm")]
    fn open_file(&mut self, _: &OpenFile, _window: &mut Window, _cx: &mut Context<Self>) {
        crate::web::click_file_input();
    }

    fn close_build(&mut self, _: &CloseBuild, _window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        // Dropping the phase is not enough on web: the worker has no drop
        // hook, so without this it keeps the build graph and the staged
        // binlog until another file is opened. Say it explicitly on both.
        if let Phase::Loaded(loaded) = &self.phase {
            let session = loaded.session.clone();
            cx.spawn(async move |_, _| session.close().await).detach();
        }
        self.phase = Phase::Welcome;
        cx.notify();
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_pane(SidebarTab::SearchLog, window, cx);
    }

    fn focus_tree(&mut self, _: &FocusTree, window: &mut Window, cx: &mut Context<Self>) {
        if let Phase::Loaded(loaded) = &self.phase {
            let handle = loaded.tree.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.set_panel_visible(Divider::Sidebar, !self.sidebar_visible, window, cx);
    }

    fn toggle_inspector(&mut self, _: &ToggleInspector, window: &mut Window, cx: &mut Context<Self>) {
        self.set_panel_visible(Divider::Inspector, !self.inspector_visible, window, cx);
    }

    fn panel_visible(&self, which: Divider) -> bool {
        match which {
            Divider::Sidebar => self.sidebar_visible,
            Divider::Inspector => self.inspector_visible,
        }
    }

    fn set_panel_visible(&mut self, which: Divider, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let hiding = !visible && self.panel_visible(which);
        match which {
            Divider::Sidebar => self.sidebar_visible = visible,
            Divider::Inspector => self.inspector_visible = visible,
        }
        // Focus left on an element that is no longer rendered is a dead
        // end: keystrokes dispatch along its (empty) path and every
        // shortcut stops working. Hand it to the tree.
        if hiding
            && self.panel_holds_focus(which, window, cx)
            && let Phase::Loaded(loaded) = &self.phase
        {
            let handle = loaded.tree.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        cx.notify();
    }

    /// Whether the keyboard focus is inside the panel. Each sidebar pane
    /// tracks its own input's handle, so one check per pane covers all of
    /// it — a click on a pane's rows focuses that same handle.
    fn panel_holds_focus(&self, which: Divider, window: &Window, cx: &App) -> bool {
        let Phase::Loaded(loaded) = &self.phase else { return false };
        let focused = |handle: FocusHandle| handle.is_focused(window);
        match which {
            Divider::Inspector => loaded.well.read(cx).focus_kind(window, cx).is_some(),
            Divider::Sidebar => {
                focused(loaded.search.read(cx).focus_handle(cx))
                    || focused(loaded.properties.read(cx).focus_handle(cx))
                    || loaded.files.as_ref().is_some_and(|f| focused(f.read(cx).focus_handle(cx)))
                    || loaded.find_in_files.as_ref().is_some_and(|f| focused(f.read(cx).focus_handle(cx)))
            }
        }
    }

    fn on_drop(&mut self, paths: &ExternalPaths, _window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(not(target_family = "wasm"))]
        if let Some(path) = paths
            .paths()
            .iter()
            .find(|p| p.extension().map_or(false, |e| e.eq_ignore_ascii_case("binlog")))
        {
            self.open(OpenSource::Path(path.clone()), cx);
        }
        #[cfg(target_family = "wasm")]
        let _ = (paths, cx);
    }

    // ----- dividers -----

    fn start_drag(&mut self, divider: Divider, event: &MouseDownEvent, cx: &mut Context<Self>) {
        // A collapsed panel drags out from nothing, so the gesture that
        // collapsed it is also the one that brings it back.
        let start_width = if self.panel_visible(divider) {
            match divider {
                Divider::Sidebar => self.sidebar_width,
                Divider::Inspector => self.inspector_width,
            }
        } else {
            0.
        };
        self.drag = Some(Drag { divider, start_x: event.position.x, start_width });
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        let divider = drag.divider;
        let delta: f32 = (event.position.x - drag.start_x).into();
        let width = match divider {
            Divider::Sidebar => drag.start_width + delta,
            Divider::Inspector => drag.start_width - delta,
        };
        if width < divider.collapse_at() {
            self.set_panel_visible(divider, false, window, cx);
            return;
        }
        let (min, max) = divider.range();
        match divider {
            Divider::Sidebar => self.sidebar_width = width.clamp(min, max),
            Divider::Inspector => self.inspector_width = width.clamp(min, max),
        }
        self.set_panel_visible(divider, true, window, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    /// The grab strip between two panels: drag to resize, drag past the
    /// panel's minimum to collapse it, double-click to collapse or restore.
    /// It is rendered whether or not its panel is showing, so a collapsed
    /// panel always has an edge to pull back out.
    fn divider(&self, which: Divider, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = self.drag.as_ref().map_or(false, |d| d.divider == which);
        div()
            .id(which.element_id())
            .relative()
            .child(crate::automation::probe(which.element_id()))
            .w(px(5.))
            .h_full()
            .flex_none()
            .flex()
            .justify_center()
            .cursor(CursorStyle::ResizeLeftRight)
            .hover(|s| s.bg(theme.hover))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event, _window, cx| this.start_drag(which, event, cx)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if let ClickEvent::Mouse(mouse) = event
                    && mouse.down.click_count == 2
                {
                    this.set_panel_visible(which, !this.panel_visible(which), window, cx);
                }
            }))
            .child(div().w(px(1.)).h_full().bg(if active { theme.accent } else { theme.border }))
            .into_any_element()
    }

    // ----- rendering -----

    fn render_mode_switch(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let segment = |id: &'static str, label: &'static str, mode: DetailMode, current: DetailMode| {
            let on = mode == current;
            div()
                .id(id)
                .px(px(10.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .cursor(CursorStyle::PointingHand)
                .when(on, |d| d.bg(theme.window_background).text_color(theme.text).shadow_sm())
                .when(!on, |d| d.text_color(theme.text_secondary).hover(|s| s.text_color(theme.text)))
                .child(label)
        };
        div()
            .flex()
            .items_center()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(7.))
            .bg(theme.selection_inactive)
            .text_size(px(11.))
            .child(segment("mode-tree", "Log", DetailMode::Tree, self.mode).on_click(cx.listener(|this, _: &ClickEvent, w, cx| {
                cx.stop_propagation();
                this.show_tree(&ShowTree, w, cx);
            })))
            .child(segment("mode-timeline", "Timeline", DetailMode::Timeline, self.mode).on_click(cx.listener(|this, _: &ClickEvent, w, cx| {
                cx.stop_propagation();
                this.show_timeline(&ShowTimeline, w, cx);
            })))
    }

    fn render_titlebar(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = window.is_window_active();
        let mut bar = div()
            // Double-clicking the title bar does whatever the system's
            // "double-click a window's title bar to" setting says — zoom,
            // minimise or nothing. The traffic lights are native subviews
            // above this one, so they keep taking their own clicks.
            .id("titlebar")
            .on_click(cx.listener(|_, event: &ClickEvent, window, _| {
                if let ClickEvent::Mouse(mouse) = event
                    && mouse.down.click_count == 2
                {
                    window.titlebar_double_click();
                }
            }))
            .flex()
            .items_center()
            .h(px(TITLEBAR_HEIGHT))
            .w_full()
            .flex_none()
            .pl(px(if cfg!(target_family = "wasm") { 12. } else { 80. }))
            .pr(px(12.))
            .gap(px(10.))
            .bg(theme.titlebar_background)
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(12.));

        match &self.phase {
            Phase::Loaded(loaded) => {
                let info = &loaded.info;
                let file = std::path::Path::new(&info.file_path)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| info.file_path.clone());
                bar = bar
                    .child(self.panel_toggle(Divider::Sidebar, theme, cx))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(13.))
                            .text_color(if active { theme.text } else { theme.text_tertiary })
                            .child(file),
                    )
                    .child(
                        div()
                            .text_color(if info.succeeded { theme.success } else { theme.error })
                            .child(if info.succeeded { "Build succeeded" } else { "Build failed" }),
                    )
                    .child(self.diagnostic_pill(
                        "pill-errors",
                        format!("{} error{}", info.error_count, if info.error_count == 1 { "" } else { "s" }),
                        "$error",
                        info.error_count,
                        theme.error,
                        theme,
                        cx,
                    ))
                    .child(self.diagnostic_pill(
                        "pill-warnings",
                        format!("{} warning{}", info.warning_count, if info.warning_count == 1 { "" } else { "s" }),
                        "$warning",
                        info.warning_count,
                        theme.warning,
                        theme,
                        cx,
                    ))
                    .child(div().font_family(crate::theme::MONO).text_color(theme.text_secondary).child(format_duration(info.duration_ms)))
                    .child(div().flex_1())
                    .child(self.render_mode_switch(theme, cx))
                    .child(div().flex_1())
                    .child(div().text_color(theme.text_tertiary).text_size(px(11.)).child(format!(
                        "{} · {} nodes · MSBuild {}",
                        format_bytes(info.file_size),
                        info.node_count,
                        info.msbuild_version.clone().unwrap_or_else(|| "?".into())
                    )))
                    .child(self.panel_toggle(Divider::Inspector, theme, cx));
            }
            _ => {
                bar = bar.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(13.))
                        .text_color(if active { theme.text } else { theme.text_tertiary })
                        .child("Structured Log Viewer"),
                );
            }
        }
        bar.into_any_element()
    }

    fn render_body(&mut self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.phase {
            Phase::Welcome => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child("MSBuild Structured Log Viewer"))
                        .child(div().text_size(px(13.)).text_color(theme.text_secondary).child(if cfg!(target_family = "wasm") {
                            "Open a .binlog from your computer, or add ?binlog=<url> to the address."
                        } else {
                            "Drop a .binlog here, press Cmd-O, or pass a path on the command line."
                        }))
                        .child(
                            div()
                                .id("welcome-open")
                                .mt(px(8.))
                                .px(px(14.))
                                .py(px(6.))
                                .rounded(px(6.))
                                .bg(theme.accent)
                                .text_color(theme.text_on_accent)
                                .text_size(px(13.))
                                .cursor(CursorStyle::PointingHand)
                                .hover(|s| s.opacity(0.9))
                                .on_click(cx.listener(|this, _, window, cx| this.open_file(&OpenFile, window, cx)))
                                .child("Open…"),
                        ),
                )
                .into_any_element(),
            Phase::Loading { name, progress } => {
                let ratio = f64::from_bits(progress.load(Ordering::Relaxed)).clamp(0., 1.) as f32;
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(10.))
                            .child(div().text_size(px(13.)).text_color(theme.text).child(format!("Loading {name}…")))
                            .child(
                                div()
                                    .w(px(320.))
                                    .h(px(6.))
                                    .rounded(px(3.))
                                    .bg(theme.selection_inactive)
                                    .child(div().w(px(320. * ratio)).h_full().rounded(px(3.)).bg(theme.accent)),
                            )
                            .child(div().text_size(px(11.)).text_color(theme.text_secondary).child("Reading, analyzing and indexing the build log")),
                    )
                    .into_any_element()
            }
            Phase::Failed { name, message } => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .max_w(px(560.))
                        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.error).child("Could not open build log"))
                        .child(div().text_size(px(12.)).text_color(theme.text_secondary).child(name.clone()))
                        .child(div().text_size(px(12.)).text_color(theme.text).child(message.clone())),
                )
                .into_any_element(),
            Phase::Loaded(loaded) => {
                let tree = loaded.tree.clone();
                let well = loaded.well.clone();
                let center: gpui::AnyElement = match (self.mode, &loaded.timeline) {
                    (DetailMode::Timeline, Some(timeline)) => timeline.clone().into_any_element(),
                    _ => tree.into_any_element(),
                };
                let mut body = div().flex().size_full().min_h_0();
                if self.sidebar_visible {
                    let sidebar = self.render_sidebar(loaded, theme, cx);
                    body = body.child(div().w(px(self.sidebar_width)).h_full().flex_none().child(sidebar));
                }
                body = body
                    .child(self.divider(Divider::Sidebar, theme, cx))
                    .child(div().flex_1().min_w_0().h_full().child(center))
                    .child(self.divider(Divider::Inspector, theme, cx));
                if self.inspector_visible {
                    body = body.child(div().w(px(self.inspector_width)).h_full().flex_none().child(well));
                }
                body.into_any_element()
            }
        }
    }
}

impl Workspace {
    /// The active pane over the tab strip along the sidebar's bottom edge,
    /// where the WPF viewer puts it (`TabStripPlacement="Bottom"`).
    fn render_sidebar(&self, loaded: &Loaded, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tabs = self.visible_tabs();
        let active = if tabs.contains(&self.sidebar_tab) { self.sidebar_tab } else { SidebarTab::SearchLog };

        let pane: gpui::AnyElement = match active {
            SidebarTab::SearchLog => loaded.search.clone().into_any_element(),
            SidebarTab::PropertiesAndItems => loaded.properties.clone().into_any_element(),
            SidebarTab::Favorites => loaded.favorites_view.clone().into_any_element(),
            SidebarTab::Files => match &loaded.files {
                Some(files) => files.clone().into_any_element(),
                None => pane_placeholder(theme),
            },
            SidebarTab::FindInFiles => match &loaded.find_in_files {
                Some(find) => find.clone().into_any_element(),
                None => pane_placeholder(theme),
            },
        };

        let favorite_count = loaded.favorites.read(cx).nodes().len();

        let strip = div()
            .flex()
            .items_center()
            .h(px(26.))
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.titlebar_background)
            .text_size(px(11.))
            .justify_around()
            .overflow_hidden()
            .children(tabs.into_iter().map(|tab| {
                let on = tab == active;
                div()
                    .id(tab.element_id())
                    .relative()
                    .child(crate::automation::probe(tab.element_id()))
                    .flex()
                    .flex_shrink(1.)
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .cursor(CursorStyle::PointingHand)
                    .when(on, |d| d.bg(theme.sidebar_background).text_color(theme.text))
                    .when(!on, |d| d.text_color(theme.text_secondary).hover(|s| s.text_color(theme.text)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.show_pane(tab, window, cx)))
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(tab.label()))
                    .when(tab == SidebarTab::Favorites && favorite_count > 0, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .px(px(4.))
                                .rounded(px(6.))
                                .bg(theme.badge_background)
                                .text_color(theme.badge_text)
                                .text_size(px(10.))
                                .child(favorite_count.to_string()),
                        )
                    })
            }));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.sidebar_background)
            .child(div().flex_1().min_h_0().child(pane))
            .child(strip)
            .into_any_element()
    }
}

/// A window seen from above, with the panel in question filled in while it
/// is showing: a hairline frame over a wash, as the node icons are drawn.
fn panel_glyph(which: Divider, open: bool, color: gpui::Hsla) -> impl IntoElement {
    let bar = div()
        .absolute()
        .top(px(0.))
        .bottom(px(0.))
        .w(px(4.))
        .bg(color.opacity(if open { 0.85 } else { 0.14 }));
    let bar = match which {
        Divider::Sidebar => bar.left(px(0.)).rounded_l(px(1.5)),
        Divider::Inspector => bar.right(px(0.)).rounded_r(px(1.5)),
    };
    div()
        .w(px(15.))
        .h(px(12.))
        .relative()
        .rounded(px(3.))
        .border(px(1.))
        .border_color(color)
        .child(bar)
}

fn pane_placeholder(theme: &Theme) -> gpui::AnyElement {
    div()
        .size_full()
        .bg(theme.sidebar_background)
        .p(px(12.))
        .text_size(px(12.))
        .text_color(theme.text_secondary)
        .child("Opening…")
        .into_any_element()
}

impl Workspace {
    /// A panel toggle at one end of the title bar, on the side of the panel
    /// it hides. The glyph loses its fill while the panel is collapsed.
    fn panel_toggle(&self, which: Divider, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let open = self.panel_visible(which);
        let color = if open { theme.text_secondary } else { theme.text_tertiary };
        div()
            .id(which.toggle_id())
            .relative()
            .child(crate::automation::probe(which.toggle_id()))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .w(px(24.))
            .h(px(22.))
            .rounded(px(5.))
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.bg(theme.hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                // Otherwise this also counts towards a title-bar double-click.
                cx.stop_propagation();
                this.set_panel_visible(which, !this.panel_visible(which), window, cx);
            }))
            .child(panel_glyph(which, open, color))
            .into_any_element()
    }

    /// The error and warning counts in the title bar. When there is anything
    /// to see, the pill runs the matching search — the quickest route from
    /// "8 warnings" to the warnings themselves.
    fn diagnostic_pill(
        &self,
        id: &'static str,
        text: String,
        query: &'static str,
        count: usize,
        color: gpui::Hsla,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let color = if count > 0 { color } else { theme.text_secondary };
        let pill = div()
            .id(id)
            .px(px(7.))
            .py(px(1.))
            .rounded(px(9.))
            .text_size(px(11.))
            .bg(color.opacity(if theme.dark { 0.18 } else { 0.12 }))
            .text_color(color)
            .child(text);
        if count == 0 {
            return pill.into_any_element();
        }
        pill.cursor(CursorStyle::PointingHand)
            .hover(|s| s.bg(color.opacity(if theme.dark { 0.3 } else { 0.22 })))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                // Otherwise this also counts towards a title-bar double-click.
                cx.stop_propagation();
                this.search_from_tree(None, Some(query.to_string()), cx);
            }))
            .into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let titlebar = self.render_titlebar(&theme, window, cx);
        let body = self.render_body(&theme, cx);

        div()
            .id("workspace")
            // Always at least one key context on the focus path: gpui
            // matches no context predicate (even a negated one) against an
            // empty stack, which would drop ⌘F when only the root is focused.
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::close_build))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::focus_tree))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_inspector))
            .on_action(cx.listener(Self::show_tree))
            .on_action(cx.listener(Self::show_timeline))
            .on_action(cx.listener(Self::show_properties))
            .on_action(cx.listener(Self::show_files))
            .on_action(cx.listener(Self::show_find_in_files))
            .on_action(cx.listener(Self::show_favorites))
            .on_drop(cx.listener(Self::on_drop))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.window_background)
            .text_color(theme.text)
            .text_size(px(13.))
            .child(titlebar)
            .child(div().flex_1().min_h_0().w_full().child(body))
    }
}
