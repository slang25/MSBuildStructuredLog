//! The document well: a Details tab plus closable tabs of embedded source
//! files and preprocessed projects, over a read-only editor with MSBuild
//! highlighting, a line gutter, end-of-element inlays for skipped imports,
//! ⌘-hover underlines, ⌘-click navigation and a quick-info popover — the
//! Mac viewer's DocumentWell / SourceEditorView / SemanticTextView /
//! QuickInfoView, on gpui.

use crate::engine::Session;
use crate::inspector::{Inspector, InspectorEvent};
use crate::model::{SemanticContext, SemanticLocation, SemanticSkippedImport, SemanticSymbol};
use crate::msbuild::{
    self, HighlightKind, Highlights, Navigation, Preference, SemanticIndex, Token, TokenKind,
};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use gpui::{
    App, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, HighlightStyle, InteractiveText, ListHorizontalSizingBehavior,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollStrategy, ScrollWheelEvent, StyledText, Task, UnderlineStyle, UniformListScrollHandle,
    Window, actions, anchored, canvas, deferred, div, point, prelude::*, px, uniform_list,
};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

actions!(source_editor, [CopySelection, SelectAll, FindInFile, FindNext, FindPrevious, CloseFind]);

pub const KEY_CONTEXT: &str = "SourceEditor";
/// The whole well, editor and find bar alike: find chords work from either.
pub const WELL_CONTEXT: &str = "SourceWell";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding as K;
    let c = Some(KEY_CONTEXT);
    let w = Some(WELL_CONTEXT);
    vec![
        K::new("cmd-c", CopySelection, c),
        K::new("cmd-a", SelectAll, c),
        K::new("cmd-f", FindInFile, w),
        K::new("cmd-g", FindNext, w),
        K::new("cmd-shift-g", FindPrevious, w),
        K::new("shift-enter", FindPrevious, w),
        K::new("escape", CloseFind, w),
    ]
}

pub const LINE_HEIGHT: f32 = 18.;
const FONT_SIZE: f32 = 12.;
const QUICK_INFO_WIDTH: f32 = 460.;
const MAX_LOCATIONS: usize = 20;

pub enum SourceEvent {
    Reveal(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabKind {
    File,
    Preprocessed,
}

struct Tab {
    id: String,
    kind: TabKind,
    title: String,
    path: String,
    text: Arc<String>,
    lines: Arc<Vec<Range<usize>>>,
    /// `uniform_list` sizes its scrollable width from *one* item, so it has
    /// to be told which one — otherwise horizontal scrolling stops at the
    /// width of line 1.
    widest_line: usize,
    highlights: Arc<Highlights>,
    semantics: Option<Arc<SemanticIndex>>,
    /// Skipped-import badges keyed by 0-based line, laid out inline with
    /// the text (so they scroll with it and push the element right).
    inlays: Arc<HashMap<usize, msbuild::Inlay>>,
    /// Lines whose pill currently shows the evaluated expression.
    evaluated_pills: HashSet<usize>,
    evaluation_id: Option<String>,
    contexts: Vec<SemanticContext>,
    contexts_total: usize,
    highlight_line: Option<usize>,
    scroll: UniformListScrollHandle,
    semantics_generation: u64,
}

impl Tab {
    fn selected_context(&self) -> Option<&SemanticContext> {
        self.contexts.iter().find(|c| Some(&c.evaluation_id) == self.evaluation_id.as_ref())
    }

    fn line_text(&self, ix: usize) -> &str {
        let range = &self.lines[ix];
        self.text[range.clone()].trim_end_matches('\r')
    }
}

/// A selection in one tab's text, as byte offsets. `anchor` is where the
/// drag started, `head` where it is now; either may be the larger.
#[derive(Clone, PartialEq)]
struct Selection {
    tab: String,
    anchor: usize,
    head: usize,
}

/// An open find bar: what was searched for in which tab, and where it hit.
struct Find {
    tab: String,
    query: String,
    /// Byte ranges into the tab's text, ascending.
    matches: Vec<Range<usize>>,
    current: Option<usize>,
}

impl Selection {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// The editor's laid-out geometry, recorded during paint. Everything is
/// uniform — fixed row height, one monospaced advance — so a point maps to
/// a text offset with arithmetic rather than hit-testing.
#[derive(Clone, Copy)]
struct EditorGeometry {
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    /// Left edge of the text column, before scrolling.
    text_left: Pixels,
    char_width: Pixels,
}

#[derive(Clone, PartialEq)]
struct Hover {
    tab: String,
    line: usize,
    token: Token,
}

enum QuickBody {
    Symbol(SemanticSymbol),
    Imports(Vec<SemanticLocation>),
    SkippedImports(Vec<SemanticSkippedImport>),
    Unavailable(String),
}

struct QuickInfo {
    hover: Hover,
    title: String,
    body: QuickBody,
    context_label: Option<String>,
    position: Point<Pixels>,
    pinned: bool,
}

struct Chooser {
    position: Point<Pixels>,
    locations: Vec<SemanticLocation>,
    evaluation_id: Option<String>,
}

pub struct SourceWell {
    session: Arc<Session>,
    inspector: Entity<Inspector>,
    tabs: Vec<Tab>,
    /// 0 = Details; n = tabs[n - 1].
    selected: usize,
    hover: Option<Hover>,
    last_mouse: Point<Pixels>,
    cmd_down: bool,
    quick_info: Option<QuickInfo>,
    quick_info_task: Option<Task<()>>,
    chooser: Option<Chooser>,
    context_menu_open: bool,
    /// Where the evaluation picker last laid out, so its menu can hang off
    /// it. `anchored` positions are window-relative, and the well does not
    /// know where in the window it sits.
    picker_bounds: Option<Bounds<Pixels>>,
    /// Drag state for the editor's overlay scrollbars.
    scrollbars: crate::scrollbar::Scrollbars,
    focus_handle: FocusHandle,
    /// Byte range in the open tab's text, as anchored and dragged. The
    /// gutter is painted and the inlays are separate elements, so a
    /// selection cannot pick either up.
    selection: Option<Selection>,
    /// The inlay badge under the mouse: its line, its detail text, and
    /// where the mouse was when it arrived.
    inlay_hover: Option<(usize, String, Point<Pixels>)>,
    selecting: bool,
    find: Option<Find>,
    find_input: Entity<TextInput>,
    /// Set when the find field gave up focus without a window to hand it
    /// back with; the next render does it.
    refocus_editor: bool,
    /// Where the editor laid out last frame, so a mouse position can be
    /// turned into an offset into the text.
    geometry: Option<EditorGeometry>,
    message: Option<String>,
    generation: u64,
}

impl EventEmitter<SourceEvent> for SourceWell {}

impl SourceWell {
    pub fn new(session: Arc<Session>, inspector: Entity<Inspector>, cx: &mut Context<Self>) -> Self {
        let find_input = cx.new(|cx| TextInput::new("Find in file", cx));
        cx.subscribe(&find_input, |this, _, event, cx| match event {
            InputEvent::Changed => this.refresh_find(true, cx),
            InputEvent::Submitted => this.find_step(1, cx),
            InputEvent::Cancelled => this.dismiss_find(cx),
        })
        .detach();
        cx.subscribe(&inspector, |this, _, event, cx| match event {
            InspectorEvent::OpenSource(node_id) => this.open_node_source(node_id.clone(), cx),
            InspectorEvent::OpenPreprocessed(node_id, title) => {
                this.open_preprocessed(node_id.clone(), title.clone(), cx)
            }
        })
        .detach();
        SourceWell {
            session,
            inspector,
            tabs: Vec::new(),
            selected: 0,
            hover: None,
            last_mouse: point(px(0.), px(0.)),
            cmd_down: false,
            quick_info: None,
            quick_info_task: None,
            chooser: None,
            context_menu_open: false,
            picker_bounds: None,
            scrollbars: crate::scrollbar::Scrollbars::new(),
            focus_handle: cx.focus_handle(),
            selection: None,
            selecting: false,
            inlay_hover: None,
            find: None,
            find_input,
            refocus_editor: false,
            geometry: None,
            message: None,
            generation: 0,
        }
    }

    pub fn show_details(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        cx.notify();
    }

    // ----- opening -----

    /// Source for a node: an error's file at its line, a project's file, an
    /// import's target...
    pub fn open_node_source(&mut self, node_id: String, cx: &mut Context<Self>) {
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.source(&node_id).await;
            this.update(cx, |this, cx| match result {
                Ok(location) => match location.text {
                    Some(text) => {
                        let line = location.line.map(|l| l as usize);
                        this.open_text(TabKind::File, location.file_path, text, line, None, cx)
                    }
                    None => this.show_message(format!("'{}' is not embedded in this binlog.", location.file_path), cx),
                },
                Err(err) => this.show_message(format!("{err:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn open_file(&mut self, path: String, line: Option<usize>, preferred_evaluation: Option<String>, cx: &mut Context<Self>) {
        // Whoever opened the file (a sidebar row, the tree, a link) wants
        // the editor's shortcuts next; the next render hands focus over.
        self.refocus_editor = true;
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.read_file(&path).await;
            this.update(cx, |this, cx| match result {
                Ok(text) => this.open_text(TabKind::File, path, text, line, preferred_evaluation, cx),
                Err(err) => this.show_message(format!("{err:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn open_preprocessed(&mut self, node_id: String, title: String, cx: &mut Context<Self>) {
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.preprocess(&node_id).await;
            this.update(cx, |this, cx| match result {
                Ok(text) => {
                    let path = format!("preprocessed:{node_id}");
                    this.open_text(TabKind::Preprocessed, path, text, None, None, cx);
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.path == format!("preprocessed:{node_id}")) {
                        tab.title = format!("{title} (preprocessed)");
                    }
                }
                Err(err) => this.show_message(format!("{err:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    fn open_text(
        &mut self,
        kind: TabKind,
        path: String,
        text: String,
        line: Option<usize>,
        preferred_evaluation: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let id = format!("{}:{}", if kind == TabKind::File { "file" } else { "pre" }, path);
        if let Some(ix) = self.tabs.iter().position(|t| t.id == id) {
            self.selected = ix + 1;
            self.goto(ix, line, cx);
            cx.notify();
            return;
        }

        self.generation += 1;
        let generation = self.generation;
        let is_msbuild = kind == TabKind::File && msbuild::is_msbuild_file(&path, &text);

        cx.spawn(async move |this, cx| {
            // Multi-MB build files: scan off the UI thread.
            let (text, lines, highlights, widest) = cx
                .background_executor()
                .spawn(async move {
                    let mut lines = Vec::new();
                    let mut start = 0;
                    for (i, b) in text.bytes().enumerate() {
                        if b == b'\n' {
                            lines.push(start..i);
                            start = i + 1;
                        }
                    }
                    lines.push(start..text.len());
                    let widest = lines
                        .iter()
                        .enumerate()
                        // Monospaced, so character count is the width.
                        .max_by_key(|(_, range)| text[range.start..range.end].trim_end_matches('\r').chars().count())
                        .map_or(0, |(ix, _)| ix);
                    let highlights = Highlights::scan(&text);
                    (text, lines, highlights, widest)
                })
                .await;

            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let tab = Tab {
                    id: id.clone(),
                    kind,
                    title: msbuild::file_name(&path),
                    path: path.clone(),
                    text: Arc::new(text),
                    lines: Arc::new(lines),
                    widest_line: widest,
                    highlights: Arc::new(highlights),
                    semantics: None,
                    inlays: Arc::new(HashMap::new()),
                    evaluated_pills: HashSet::new(),
                    evaluation_id: preferred_evaluation.clone(),
                    contexts: Vec::new(),
                    contexts_total: 0,
                    highlight_line: None,
                    scroll: UniformListScrollHandle::new(),
                    semantics_generation: 0,
                };
                this.tabs.push(tab);
                let ix = this.tabs.len() - 1;
                this.selected = ix + 1;
                this.goto(ix, line, cx);
                if is_msbuild {
                    this.load_semantics(id, preferred_evaluation, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn goto(&mut self, ix: usize, line: Option<usize>, _cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(ix) else { return };
        let Some(line) = line.filter(|l| *l >= 1) else { return };
        let row = (line - 1).min(tab.lines.len().saturating_sub(1));
        tab.highlight_line = Some(row);
        tab.scroll.scroll_to_item(row, ScrollStrategy::Center);
    }

    fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        let closed = self.tabs.remove(ix);
        if self.selection.as_ref().is_some_and(|s| s.tab == closed.id) {
            self.selection = None;
        }
        if self.selected == ix + 1 {
            self.selected = if ix < self.tabs.len() { ix + 1 } else { self.tabs.len() };
        } else if self.selected > ix + 1 {
            self.selected -= 1;
        }
        self.dismiss(cx);
        cx.notify();
    }

    fn show_message(&mut self, message: String, cx: &mut Context<Self>) {
        eprintln!("source: {message}");
        self.message = Some(message);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            this.update(cx, |this, cx| {
                this.message = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ----- semantics -----

    fn load_semantics(&mut self, tab_id: String, evaluation_id: Option<String>, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        tab.semantics_generation += 1;
        let generation = tab.semantics_generation;
        let path = tab.path.clone();
        let text = tab.text.clone();
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            // The engine answer is async; tokenizing a multi-MB file still
            // happens off the UI thread where there is one.
            let result = match session.semantic_file(&path, evaluation_id).await {
                Ok(file) => {
                    cx.background_executor()
                        .spawn(async move {
                            let index = SemanticIndex::new(&text, file);
                            let inlays = msbuild::import_inlays(&text, &index.skipped_imports());
                            anyhow::Ok((index, inlays))
                        })
                        .await
                }
                Err(err) => Err(err),
            };
            this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
                if tab.semantics_generation != generation {
                    return;
                }
                match result {
                    Ok((index, inlays)) => {
                        eprintln!(
                            "semantics({}): {} tokens, {} imports, {} skipped, {} inlays, contexts {}/{}",
                            tab.path,
                            index.tokens.len(),
                            index.file.imports.len(),
                            index.file.skipped_imports.len(),
                            inlays.len(),
                            index.file.contexts.len(),
                            index.file.contexts_total
                        );
                        tab.evaluation_id = index.file.evaluation_id.clone();
                        tab.contexts = index.file.contexts.clone();
                        tab.contexts_total = index.file.contexts_total.max(tab.contexts.len());
                        tab.inlays = Arc::new(inlays);
                        tab.semantics = Some(Arc::new(index));
                        cx.notify();
                    }
                    Err(err) => eprintln!("semantics({}): {err:#}", tab.path),
                }
            })
            .ok();
        })
        .detach();
    }

    fn select_context(&mut self, evaluation_id: String, cx: &mut Context<Self>) {
        self.context_menu_open = false;
        let Some(tab) = self.tabs.get(self.selected.wrapping_sub(1)) else { return };
        if tab.evaluation_id.as_ref() == Some(&evaluation_id) {
            cx.notify();
            return;
        }
        let id = tab.id.clone();
        self.load_semantics(id, Some(evaluation_id), cx);
        cx.notify();
    }

    // ----- hover, quick info, navigation -----

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.quick_info = None;
        self.quick_info_task = None;
        self.chooser = None;
        self.context_menu_open = false;
        self.inlay_hover = None;
        cx.notify();
    }

    fn set_hover(&mut self, hover: Option<Hover>, cx: &mut Context<Self>) {
        if self.hover == hover {
            return;
        }
        self.hover = hover.clone();
        cx.notify();
        match hover {
            Some(hover) => self.schedule_quick_info(hover, cx),
            None => {
                self.quick_info_task = None;
                if self.quick_info.as_ref().map_or(false, |q| !q.pinned) {
                    self.quick_info = None;
                    cx.notify();
                }
            }
        }
    }

    fn schedule_quick_info(&mut self, hover: Hover, cx: &mut Context<Self>) {
        if self.quick_info.as_ref().map_or(false, |q| q.pinned) {
            return;
        }
        let position = self.last_mouse;
        self.quick_info_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let still = this.update(cx, |this, _| this.hover.as_ref() == Some(&hover)).unwrap_or(false);
            if !still {
                return;
            }
            let _ = this.update(cx, |this, cx| this.fetch_quick_info(hover, position, false, cx));
        }));
    }

    fn fetch_quick_info(&mut self, hover: Hover, position: Point<Pixels>, pinned: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == hover.tab) else { return };
        let Some(index) = tab.semantics.clone() else { return };
        let context_label = tab.selected_context().map(|c| c.label.clone());
        let title = hover.token.title();
        let token = hover.token.clone();

        let Some(kind) = token.symbol_kind() else {
            let body = match index.import_navigation(&token) {
                Navigation::Open(location) => QuickBody::Imports(vec![location]),
                Navigation::Choose(locations) => QuickBody::Imports(locations),
                Navigation::None(reason) => {
                    let skipped = index.skipped_imports_for(&token);
                    if skipped.is_empty() { QuickBody::Unavailable(reason) } else { QuickBody::SkippedImports(skipped) }
                }
            };
            self.quick_info = Some(QuickInfo { hover, title, body, context_label, position, pinned });
            cx.notify();
            return;
        };

        let Some(evaluation_id) = tab.evaluation_id.clone() else {
            self.quick_info = Some(QuickInfo {
                hover,
                title,
                body: QuickBody::Unavailable("No evaluation context for this file.".into()),
                context_label,
                position,
                pinned,
            });
            cx.notify();
            return;
        };

        let session = self.session.clone();
        let name = token.name.clone();
        cx.spawn(async move |this, cx| {
            let result = session.semantic_resolve(&evaluation_id, kind, &name).await;
            this.update(cx, |this, cx| {
                if !pinned && this.hover.as_ref() != Some(&hover) {
                    return;
                }
                let body = match result {
                    Ok(symbol) => QuickBody::Symbol(symbol),
                    Err(err) => QuickBody::Unavailable(format!("{err:#}")),
                };
                this.quick_info = Some(QuickInfo { hover, title, body, context_label, position, pinned });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn activate_token(&mut self, hover: Hover, cx: &mut Context<Self>) {
        let position = self.last_mouse;
        let Some(tab) = self.tabs.iter().find(|t| t.id == hover.tab) else { return };
        let Some(index) = tab.semantics.clone() else { return };
        let evaluation_id = tab.evaluation_id.clone();
        let token = hover.token.clone();

        let Some(kind) = token.symbol_kind() else {
            let navigation = index.import_navigation(&token);
            self.follow(navigation, evaluation_id, position, cx);
            return;
        };
        let Some(eval) = evaluation_id.clone() else { return };
        let preference = if token.kind == TokenKind::TargetDefinition { Preference::Executions } else { Preference::Definitions };
        let session = self.session.clone();
        let name = token.name.clone();
        cx.spawn(async move |this, cx| {
            let result = session.semantic_resolve(&eval, kind, &name).await;
            this.update(cx, |this, cx| match result {
                Ok(symbol) => {
                    let navigation = msbuild::symbol_navigation(&symbol, preference);
                    this.follow(navigation, evaluation_id, position, cx);
                }
                Err(err) => this.show_message(format!("{err:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    fn follow(&mut self, navigation: Navigation, evaluation_id: Option<String>, position: Point<Pixels>, cx: &mut Context<Self>) {
        match navigation {
            Navigation::Open(location) => self.go(location, evaluation_id, cx),
            Navigation::Choose(locations) => {
                self.chooser = Some(Chooser { position, locations, evaluation_id });
                cx.notify();
            }
            Navigation::None(reason) => self.show_message(reason, cx),
        }
    }

    fn go(&mut self, location: SemanticLocation, evaluation_id: Option<String>, cx: &mut Context<Self>) {
        self.dismiss(cx);
        if let Some(path) = location.path.clone().filter(|_| location.available) {
            self.open_file(path, Some(location.line.unwrap_or(1)), evaluation_id, cx);
            return;
        }
        if let Some(node_id) = location.node_id {
            cx.emit(SourceEvent::Reveal(node_id));
            return;
        }
        let reason = match location.path {
            Some(p) => format!("'{p}' is not embedded in this binlog."),
            None => "This destination is not available.".into(),
        };
        self.show_message(reason, cx);
    }

    fn on_line_hover(&mut self, tab_id: &str, line: usize, char_ix: Option<usize>, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.last_mouse = event.position;
        self.cmd_down = event.modifiers.platform;
        let hover = char_ix.and_then(|ix| {
            let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
            let index = tab.semantics.as_ref()?;
            let offset = tab.lines[line].start + ix;
            let token = index.token_at(offset)?.clone();
            Some(Hover { tab: tab_id.to_string(), line, token })
        });
        self.set_hover(hover, cx);
    }

    fn on_token_click(&mut self, hover: Hover, window: &Window, cx: &mut Context<Self>) {
        // The mouse-down that started this click also anchored a text
        // selection; a click is not a drag, so let go of it here rather
        // than trusting the editor's mouse-up to run after us.
        self.selecting = false;
        if self.selection.as_ref().is_some_and(|s| s.is_empty()) {
            self.selection = None;
        }
        let navigable = self
            .tabs
            .iter()
            .find(|t| t.id == hover.tab)
            .and_then(|t| t.semantics.as_ref().map(|s| s.is_navigable(&hover.token)))
            .unwrap_or(false);
        if window.modifiers().platform && navigable {
            self.activate_token(hover, cx);
        } else {
            // A plain click keeps quick info open so it can be read.
            let position = self.last_mouse;
            self.quick_info_task = None;
            self.fetch_quick_info(hover, position, true, cx);
        }
    }

    fn on_editor_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A press anywhere dismisses transient UI; a token click that
        // follows re-pins quick info for the token.
        if self.quick_info.is_some() || self.chooser.is_some() || self.context_menu_open {
            self.dismiss(cx);
        }
        if event.button == MouseButton::Left {
            self.focus_handle.focus(window, cx);
            self.begin_selection(event.position, cx);
        }
    }

    fn on_editor_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.extend_selection(event.position, cx);
        }
    }

    fn on_editor_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.selecting = false;
            cx.notify();
        }
    }

    // ----- text selection -----

    /// The byte offset in `tab.text` under a window point.
    fn offset_at(&self, tab: &Tab, position: Point<Pixels>) -> Option<usize> {
        let g = self.geometry?;
        if g.char_width <= px(0.) {
            return None;
        }
        let y: f32 = (position.y - g.bounds.origin.y - g.offset.y).into();
        let row = (y / LINE_HEIGHT).floor().max(0.) as usize;
        let row = row.min(tab.lines.len().saturating_sub(1));
        let line = tab.line_text(row);
        let x: f32 = (position.x - g.text_left - g.offset.x).into();
        let x = text_x(x, &self.line_gaps(tab, row, g.char_width.into()), g.char_width.into());
        let column = (x / f32::from(g.char_width)).round().max(0.) as usize;
        let column = column.min(line.chars().count());
        let byte = line.char_indices().nth(column).map_or(line.len(), |(b, _)| b);
        Some(tab.lines[row].start + byte)
    }

    fn begin_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else { return };
        let Some(offset) = self.offset_at(tab, position) else { return };
        self.selection = Some(Selection { tab: tab.id.clone(), anchor: offset, head: offset });
        self.selecting = true;
        cx.notify();
    }

    fn extend_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else { return };
        let Some(offset) = self.offset_at(tab, position) else { return };
        if let Some(selection) = &mut self.selection
            && selection.head != offset
        {
            selection.head = offset;
            cx.notify();
        }
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.as_ref().filter(|s| !s.is_empty())?;
        let tab = self.tabs.iter().find(|t| t.id == selection.tab)?;
        let range = selection.range();
        Some(tab.text[range].to_string())
    }

    fn copy_selection(&mut self, _: &CopySelection, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn toggle_pill(&mut self, tab_id: &str, line: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        if !tab.evaluated_pills.remove(&line) {
            tab.evaluated_pills.insert(line);
        }
        // The press that became this click anchored a selection; a click
        // is not a drag.
        self.selecting = false;
        if self.selection.as_ref().is_some_and(|s| s.is_empty()) {
            self.selection = None;
        }
        cx.notify();
    }

    /// Where row `ix`'s text departs from "column × advance".
    fn line_gaps(&self, tab: &Tab, ix: usize, char_width: f32) -> Vec<Gap> {
        tab.inlays
            .get(&ix)
            .map(|inlay| inlay_gaps(inlay, tab.line_text(ix), tab.evaluated_pills.contains(&ix), char_width))
            .unwrap_or_default()
    }

    // ----- automation and tests -----

    /// The well's state as JSON, for `--automation` dumps and tests.
    pub fn describe(&self, cx: &App) -> serde_json::Value {
        let tabs: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .map(|tab| {
                let mut inlays: Vec<usize> = tab.inlays.keys().map(|ix| ix + 1).collect();
                inlays.sort_unstable();
                let mut evaluated: Vec<usize> = tab.evaluated_pills.iter().map(|ix| ix + 1).collect();
                evaluated.sort_unstable();
                serde_json::json!({
                    "title": tab.title,
                    "path": tab.path,
                    "lines": tab.lines.len(),
                    "evaluation": tab.evaluation_id,
                    "contexts": tab.contexts.len(),
                    "contextsTotal": tab.contexts_total,
                    "semantics": tab.semantics.is_some(),
                    "inlayLines": inlays,
                    "evaluatedPillLines": evaluated,
                    "highlightLine": tab.highlight_line.map(|l| l + 1),
                })
            })
            .collect();
        let selection = self.selection.as_ref().map(|s| {
            let range = s.range();
            let text = self.tabs.iter().find(|t| t.id == s.tab).map(|t| t.text[range.clone()].to_string()).unwrap_or_default();
            serde_json::json!({ "tab": s.tab, "start": range.start, "end": range.end, "text": text })
        });
        serde_json::json!({
            "tabs": tabs,
            "selected": self.selected.checked_sub(1),
            "find": self.find.as_ref().map(|f| serde_json::json!({
                "query": f.query,
                "matches": f.matches.len(),
                "current": f.current,
                "text": self.find_input.read(cx).text(),
            })),
            "selection": selection,
            "quickInfo": self.quick_info.as_ref().map(|q| q.title.clone()),
            "quickInfoPinned": self.quick_info.as_ref().map(|q| q.pinned),
            "chooser": self.chooser.as_ref().map(|c| c.locations.len()),
            "contextMenuOpen": self.context_menu_open,
            "message": self.message,
        })
    }

    pub fn focus_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    /// Which part of the well has keyboard focus, if any.
    pub fn focus_kind(&self, window: &Window, cx: &App) -> Option<&'static str> {
        if self.focus_handle.is_focused(window) {
            Some("editor")
        } else if self.find_input.read(cx).focus_handle(cx).is_focused(window) {
            Some("find-input")
        } else {
            None
        }
    }

    // ----- find in file -----

    fn open_find(&mut self, _: &FindInFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else {
            // Nothing to search: let the workspace's ⌘F have it.
            cx.propagate();
            return;
        };
        let tab_id = tab.id.clone();
        if self.find.is_none() {
            self.find = Some(Find { tab: tab_id, query: String::new(), matches: Vec::new(), current: None });
        }
        // A short single-line selection is what the user wants to look for.
        if let Some(seed) = self.selected_text().filter(|t| !t.contains('\n') && t.len() <= 200) {
            self.find_input.update(cx, |input, cx| input.set_text(seed, cx));
        } else {
            self.refresh_find(true, cx);
        }
        let handle = self.find_input.read(cx).focus_handle(cx);
        handle.focus(window, cx);
        cx.notify();
    }

    fn close_find(&mut self, _: &CloseFind, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.focus_handle.focus(window, cx);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    /// Escape inside the field: the input has no window to hand back
    /// focus with, so the next render does it.
    fn dismiss_find(&mut self, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.refocus_editor = true;
            cx.notify();
        }
    }

    /// The buttons' mouse-down moved focus to the well; typing should
    /// still land in the field.
    fn focus_find_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.find_input.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    }

    fn find_next(&mut self, _: &FindNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(1, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, _window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(-1, cx);
    }

    /// Re-run the query against the selected tab. Keeps the current hit
    /// where it was when the text merely grew; otherwise starts from the
    /// selection.
    fn refresh_find(&mut self, jump: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else { return };
        let query = self.find_input.read(cx).text().to_string();
        let Some(find) = self.find.as_mut() else { return };
        let same_tab = find.tab == tab.id;
        let anchor = find
            .current
            .and_then(|i| find.matches.get(i))
            .filter(|_| same_tab)
            .map(|m| m.start)
            .or_else(|| self.selection.as_ref().filter(|s| s.tab == tab.id).map(|s| s.range().start))
            .unwrap_or(0);
        find.tab = tab.id.clone();
        find.query = query;
        find.matches = find_matches(&tab.text, &find.query);
        find.current = if find.matches.is_empty() {
            None
        } else {
            Some(find.matches.iter().position(|m| m.start >= anchor).unwrap_or(0))
        };
        if find.current.is_none() {
            // Whatever a shorter query had selected no longer applies.
            self.selection = None;
        } else if jump {
            self.reveal_current_match(cx);
        }
        cx.notify();
    }

    fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else { return };
        let n = find.matches.len() as isize;
        if n == 0 {
            return;
        }
        let current = find.current.map(|c| c as isize).unwrap_or(-delta);
        find.current = Some((((current + delta) % n + n) % n) as usize);
        self.reveal_current_match(cx);
        cx.notify();
    }

    /// Scroll the current hit into view (both axes) and select it, so the
    /// selection painting marks it and ⌘C copies it.
    fn reveal_current_match(&mut self, _cx: &mut Context<Self>) {
        let Some(find) = &self.find else { return };
        let Some(range) = find.current.and_then(|i| find.matches.get(i)).cloned() else { return };
        let tab_id = find.tab.clone();
        let geometry = self.geometry;
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        let row = match tab.lines.binary_search_by(|l| l.start.cmp(&range.start)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        if let Some(g) = geometry {
            let column = tab.text[tab.lines[row].start..range.start].chars().count() as f32;
            let x: f32 = (g.char_width * column).into();
            let text_left: f32 = (g.text_left - g.bounds.origin.x).into();
            let view_width: f32 = f32::from(g.bounds.size.width) - text_left;
            let offset = tab.scroll.0.borrow().base_handle.offset();
            let visible_left: f32 = (-offset.x).into();
            if x < visible_left || x > visible_left + view_width - 60. {
                let new_x = (x - view_width * 0.3).max(0.);
                tab.scroll.0.borrow().base_handle.set_offset(point(px(-new_x), offset.y));
            }
        }
        tab.scroll.scroll_to_item(row, ScrollStrategy::Center);
        self.selection = Some(Selection { tab: tab_id, anchor: range.start, head: range.end });
        self.selecting = false;
    }

    fn render_find_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let find = self.find.as_ref()?;
        let (count, alarming) = match (find.matches.len(), find.current) {
            (0, _) if find.query.is_empty() => (String::new(), false),
            (0, _) => ("No matches".to_string(), true),
            (n, Some(i)) => (format!("{} of {}", i + 1, n), false),
            (n, None) => (format!("{n} matches"), false),
        };
        let button = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .relative()
                .child(crate::automation::probe(id))
                .h(px(20.))
                .px(px(6.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor(CursorStyle::PointingHand)
                .text_color(theme.text_secondary)
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(glyph)
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(34.))
                .w_full()
                .flex_none()
                .px(px(8.))
                .bg(theme.titlebar_background)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(11.))
                .child(div().w(px(260.)).flex_none().child(self.find_input.clone()))
                .child(
                    div()
                        .min_w(px(72.))
                        .text_color(if alarming { theme.error } else { theme.text_secondary })
                        .child(count),
                )
                .child(button("find-prev", "‹").on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.find_step(-1, cx);
                    this.focus_find_input(window, cx);
                })))
                .child(button("find-next", "›").on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.find_step(1, cx);
                    this.focus_find_input(window, cx);
                })))
                .child(div().flex_1())
                .child(button("find-close", "✕").on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.close_find(&CloseFind, window, cx)
                })))
                .into_any_element(),
        )
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else { return };
        self.selection = Some(Selection { tab: tab.id.clone(), anchor: 0, head: tab.text.len() });
        cx.notify();
    }

    fn on_scroll(&mut self, _: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_info.is_some() || self.chooser.is_some() {
            self.dismiss(cx);
        }
        // The badge scrolls out from under a still mouse without a hover
        // event to say so.
        if self.inlay_hover.take().is_some() {
            cx.notify();
        }
    }

    fn on_modifiers(&mut self, event: &ModifiersChangedEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.cmd_down != event.modifiers.platform {
            self.cmd_down = event.modifiers.platform;
            cx.notify();
        }
    }

    // ----- rendering -----

    fn render_tab_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut bar = div()
            .flex()
            .items_center()
            .h(px(28.))
            .w_full()
            .flex_none()
            .px(px(4.))
            .gap(px(2.))
            .bg(theme.titlebar_background)
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(11.))
            .overflow_hidden();

        let selected = self.selected;
        bar = bar.child(
            div()
                .id("tab-details")
                .px(px(8.))
                .py(px(3.))
                .rounded(px(4.))
                .cursor(CursorStyle::PointingHand)
                .when(selected == 0, |d| d.bg(theme.selection_inactive))
                .when(selected != 0, |d| d.hover(|s| s.bg(theme.hover)))
                .text_color(theme.text)
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.selected = 0;
                    this.dismiss(cx);
                }))
                .child("Details"),
        );

        for (ix, tab) in self.tabs.iter().enumerate() {
            let is_selected = selected == ix + 1;
            let glyph = if tab.kind == TabKind::Preprocessed { "✦" } else { "▭" };
            bar = bar.child(
                div()
                    .id(ElementId::NamedInteger("tab".into(), ix as u64))
                    .relative()
                    .child(crate::automation::probe(format!("source-tab-{ix}")))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .max_w(px(220.))
                    .cursor(CursorStyle::PointingHand)
                    .when(is_selected, |d| d.bg(theme.selection_inactive))
                    .when(!is_selected, |d| d.hover(|s| s.bg(theme.hover)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.selected = ix + 1;
                        this.dismiss(cx);
                    }))
                    // Middle-click closes the tab, as everywhere else.
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.close_tab(ix, cx);
                        }),
                    )
                    .child(div().text_size(px(9.)).text_color(theme.text_secondary).child(glyph))
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.text).child(tab.title.clone()))
                    .child(
                        div()
                            .id("close")
                            .relative()
                            .child(crate::automation::probe(format!("source-tab-close-{ix}")))
                            .text_size(px(10.))
                            .text_color(theme.text_tertiary)
                            .hover(|s| s.text_color(theme.text))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                cx.stop_propagation();
                                this.close_tab(ix, cx);
                            }))
                            .child("✕"),
                    ),
            );
        }
        bar.into_any_element()
    }

    fn render_context_bar(&self, tab_ix: usize, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let tab = self.tabs.get(tab_ix)?;
        if tab.contexts.is_empty() {
            return None;
        }
        let selected = tab.selected_context();
        let label = selected.map(|c| c.label.clone()).unwrap_or_else(|| "Choose an evaluation".into());
        let total = tab.contexts_total;
        // A project file is evaluated a few times (restore, outer, inner);
        // a shared .props/.targets is *imported by* every evaluation in the
        // build, and a bare number reads as if this project ran that often.
        let count = if selected.is_some_and(|c| c.is_project_file) {
            format!("evaluated {total} times")
        } else {
            format!("imported by {total} evaluations")
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(24.))
                .w_full()
                .flex_none()
                .px(px(8.))
                .bg(theme.titlebar_background)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(11.))
                .child(
                    div()
                        .id("context-picker")
                        .relative()
                        .child(crate::automation::probe("context-picker"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .min_w_0()
                        .cursor(CursorStyle::PointingHand)
                        .child({
                            // A zero-size probe: prepaint reports where the
                            // button ended up, which is the only way to hang
                            // a window-anchored menu off it.
                            let entity = cx.entity();
                            canvas(
                                move |bounds, _window, cx| {
                                    entity.update(cx, |this, _| this.picker_bounds = Some(bounds));
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0()
                        })
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.context_menu_open = !this.context_menu_open;
                            this.quick_info = None;
                            cx.notify();
                        }))
                        .child(div().text_color(theme.text_secondary).child("◎"))
                        .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.text).child(label))
                        .child(div().text_color(theme.text_tertiary).child("▾")),
                )
                .when(total > 1, |d| d.child(div().ml_auto().flex_none().text_color(theme.text_tertiary).child(count)))
                .into_any_element(),
        )
    }

    fn render_context_menu(&self, tab_ix: usize, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.context_menu_open {
            return None;
        }
        let tab = self.tabs.get(tab_ix)?;
        let current = tab.evaluation_id.clone();
        let mut menu = div()
            .id("context-menu")
            // The editor underneath dismisses transient UI on mouse-down, and
            // an element removed on mouse-down never sees the mouse-up that
            // fires its click. Keep the press to ourselves, and stop the rows
            // beneath from hovering through us.
            .occlude()
            .on_any_mouse_down(|_, _window, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .w(px(360.))
            .max_h(px(360.))
            .overflow_hidden()
            .p(px(4.))
            .rounded(px(8.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .text_size(px(12.));
        // The contexts scroll; the footer and the reveal item stay put, so a
        // file imported by dozens of evaluations still reaches them.
        let mut list = div().id("context-list").flex().flex_col().min_h_0().flex_1().overflow_y_scroll();
        for (i, context) in tab.contexts.iter().enumerate() {
            let is_current = Some(&context.evaluation_id) == current.as_ref();
            let eval = context.evaluation_id.clone();
            list = list.child(
                div()
                    .id(ElementId::NamedInteger("ctx".into(), i as u64))
                    .relative()
                    .child(crate::automation::probe(format!("context-{i}")))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .h(px(24.))
                    .rounded(px(5.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        let eval = eval.clone();
                        this.select_context(eval, cx);
                    }))
                    .child(div().w(px(12.)).text_color(theme.accent).child(if is_current { "✓" } else { "" }))
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.text).child(context.label.clone())),
            );
        }
        menu = menu.child(list);
        if tab.contexts_total > tab.contexts.len() {
            menu = menu.child(
                div().px(px(8.)).py(px(4.)).text_size(px(11.)).text_color(theme.text_tertiary).child(format!(
                    "Showing {} of {}",
                    tab.contexts.len(),
                    tab.contexts_total
                )),
            );
        }
        if let Some(eval) = current {
            menu = menu.child(div().h(px(1.)).w_full().my(px(3.)).bg(theme.border)).child(
                div()
                    .id("reveal-context")
                    .relative()
                    .child(crate::automation::probe("reveal-context"))
                    .px(px(8.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme.hover))
                    .text_color(theme.text)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.context_menu_open = false;
                        cx.emit(SourceEvent::Reveal(eval.clone()));
                        cx.notify();
                    }))
                    .child("Reveal Evaluation in Tree"),
            );
        }
        let anchor = self
            .picker_bounds
            .map(|b| point(b.origin.x, b.origin.y + b.size.height + px(4.)))
            .unwrap_or_else(|| point(px(8.), px(28.)));
        Some(
            deferred(anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(menu))
                .into_any_element(),
        )
    }

    fn render_editor(&mut self, tab_ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(tab) = self.tabs.get(tab_ix) else { return div().into_any_element() };
        let line_count = tab.lines.len();
        let widest_line = tab.widest_line;
        let gutter_width = px(12. + 7.5 * (line_count.max(1).to_string().len() as f32));
        let tab_id = tab.id.clone();
        let scroll = tab.scroll.clone();
        let cmd_down = self.cmd_down;
        let hover = self.hover.clone();
        let tab_id_for_rows = tab_id.clone();

        let list = uniform_list(
            "source-lines",
            line_count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                let theme = *cx.global::<Theme>();
                let char_width = this.geometry.map(|g| g.char_width);
                let Some(tab) = this.tabs.iter().find(|t| t.id == tab_id_for_rows) else { return Vec::new() };
                let (matches, current): (&[Range<usize>], Option<Range<usize>>) = match &this.find {
                    Some(find) if find.tab == tab.id => {
                        (&find.matches, find.current.and_then(|i| find.matches.get(i).cloned()))
                    }
                    _ => (&[], None),
                };
                let mut items = Vec::with_capacity(range.len());
                for ix in range {
                    if ix >= tab.lines.len() {
                        break;
                    }
                    let row = RowContext { cmd_down, hover: hover.as_ref(), gutter_width, char_width, matches, current: current.clone() };
                    items.push(render_line(tab, ix, &theme, &row, cx));
                }
                items
            }),
        )
        .track_scroll(&scroll)
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .with_width_from_item(Some(widest_line))
        .size_full();

        // Painted under the rows so the text stays legible through it, and
        // it doubles as the place the editor's geometry is recorded from.
        let selection_layer = {
            let scroll = scroll.clone();
            let entity = cx.entity();
            let selection = self.selection.clone().filter(|s| s.tab == tab_id && !s.is_empty());
            let lines = tab.lines.clone();
            let text = tab.text.clone();
            let gaps: HashMap<usize, Vec<Gap>> = match self.geometry {
                Some(g) => tab.inlays.keys().map(|&ix| (ix, self.line_gaps(tab, ix, g.char_width.into()))).collect(),
                None => HashMap::new(),
            };
            let theme = *theme;
            canvas(
                move |bounds, window, cx| {
                    let offset = scroll.0.borrow().base_handle.offset();
                    let mut mono = window.text_style().font();
                    mono.family = crate::theme::MONO.into();
                    let run = gpui::TextRun {
                        len: 8,
                        font: mono.clone(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    // One advance, measured from the font actually in use.
                    let width = window
                        .text_system()
                        .shape_line("00000000".into(), px(FONT_SIZE), &[run], None)
                        .width
                        / 8.;
                    entity.update(cx, |this, _| {
                        this.geometry = Some(EditorGeometry {
                            bounds,
                            offset,
                            text_left: bounds.origin.x + gutter_width + px(8.),
                            char_width: width,
                        });
                    });
                    (bounds, offset, width)
                },
                move |_, (bounds, offset, char_width): (Bounds<Pixels>, Point<Pixels>, Pixels), window, _| {
                    let Some(selection) = selection else { return };
                    paint_selection(
                        bounds,
                        offset,
                        char_width,
                        gutter_width,
                        &lines,
                        &text,
                        &gaps,
                        selection.range(),
                        &theme,
                        window,
                    );
                },
            )
            .absolute()
            .inset_0()
        };

        let overlay = {
            let scroll = scroll.clone();
            let highlight_line = tab.highlight_line;
            let theme = *theme;
            canvas(
                move |bounds, window, _cx| {
                    let offset = scroll.0.borrow().base_handle.offset();
                    let style = window.text_style();
                    let mut mono = style.font();
                    mono.family = crate::theme::MONO.into();
                    (bounds, offset, mono)
                },
                move |bounds, (_, offset, mono): (Bounds<Pixels>, Point<Pixels>, gpui::Font), window, cx| {
                    paint_gutter(bounds, offset, mono, line_count, gutter_width, highlight_line, &theme, window, cx)
                },
            )
            .absolute()
            .inset_0()
        };

        div()
            .id(ElementId::Name(format!("editor:{tab_id}").into()))
            .size_full()
            .relative()
            .child(crate::automation::probe("editor"))
            .bg(theme.content_background)
            .font_family(crate::theme::MONO)
            .text_size(px(FONT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .text_color(theme.text)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_mouse_down))
            .on_mouse_move(cx.listener(Self::on_editor_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .cursor(CursorStyle::IBeam)
            .child(selection_layer)
            .child(list)
            .child(overlay)
            .child(self.scrollbars.render(&scroll.0.borrow().base_handle, theme))
            .into_any_element()
    }

    fn render_quick_info(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let info = self.quick_info.as_ref()?;
        let mono = |text: String, color: gpui::Hsla| div().font_family(crate::theme::MONO).text_size(px(11.)).text_color(color).child(text);
        let caption = |text: String| div().text_size(px(11.)).text_color(theme.text_secondary).child(text);

        let location_list = |title: String, locations: &[SemanticLocation]| {
            let mut list = div().flex().flex_col().gap(px(3.)).child(caption(title));
            for location in locations.iter().take(MAX_LOCATIONS) {
                let color = if location.is_reachable() { theme.text } else { theme.text_tertiary };
                let mut entry = div().flex().flex_col().child(mono(location.title(), color));
                if let Some(secondary) = location.secondary() {
                    entry = entry.child(mono(secondary, theme.text_secondary));
                }
                list = list.child(entry);
            }
            if locations.len() > MAX_LOCATIONS {
                list = list.child(caption(format!("…and {} more", locations.len() - MAX_LOCATIONS)));
            }
            list
        };

        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w(px(QUICK_INFO_WIDTH))
            .max_h(px(420.))
            .p(px(12.))
            .rounded(px(8.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .text_size(px(12.))
            .child(div().font_family(crate::theme::MONO).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child(info.title.clone()));

        match &info.body {
            QuickBody::Symbol(symbol) if !symbol.found => {
                panel = panel.child(caption("Not defined in this evaluation.".into()));
            }
            QuickBody::Symbol(symbol) => {
                if let Some(value) = symbol.value.as_ref().filter(|v| !v.is_empty()) {
                    panel = panel.child(mono(value.clone(), theme.text));
                }
                if !symbol.facts.is_empty() {
                    let mut grid = div().flex().flex_col().gap(px(3.));
                    for fact in &symbol.facts {
                        grid = grid.child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(div().w(px(90.)).flex_none().text_size(px(11.)).text_color(theme.text_secondary).child(fact.label.clone().unwrap_or_default()))
                                .child(div().flex_1().min_w_0().child(mono(fact.value.clone().unwrap_or_default(), theme.text))),
                        );
                    }
                    panel = panel.child(div().h(px(1.)).bg(theme.border)).child(grid);
                }
                if !symbol.definitions.is_empty() {
                    let title = if symbol.definitions.len() == 1 { "Defined in".to_string() } else { format!("Defined in {} places", symbol.definitions.len()) };
                    panel = panel.child(location_list(title, &symbol.definitions));
                }
                if !symbol.executions.is_empty() {
                    let title = if symbol.executions.len() == 1 { "Ran once".to_string() } else { format!("Ran {} times", symbol.executions.len()) };
                    panel = panel.child(location_list(title, &symbol.executions));
                }
                if let Some(note) = &symbol.note {
                    panel = panel.child(caption(note.clone()));
                }
            }
            QuickBody::Imports(locations) => {
                let title = if locations.len() == 1 { "Imports".to_string() } else { format!("Imports {} files", locations.len()) };
                panel = panel.child(location_list(title, locations));
            }
            QuickBody::SkippedImports(skipped) => {
                panel = panel.child(caption("↓ Not imported".into()));
                for record in skipped {
                    if record.has_condition() {
                        let row = |label: &str, value: String, color: gpui::Hsla| {
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(div().w(px(70.)).flex_none().text_size(px(11.)).text_color(theme.text_secondary).child(label.to_string()))
                                .child(div().flex_1().min_w_0().child(mono(value, color)))
                        };
                        panel = panel.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(row("Condition", record.condition.clone().unwrap_or_default(), theme.text))
                                .child(row("Evaluated", record.evaluated_condition.clone().unwrap_or_default(), theme.text))
                                .child(row("Result", "false".into(), theme.warning)),
                        );
                    } else if let Some(reason) = &record.reason {
                        panel = panel.child(mono(reason.clone(), theme.text_secondary));
                    }
                }
                panel = panel.child(div().text_size(px(10.)).text_color(theme.text_tertiary).child("As evaluated during the build."));
            }
            QuickBody::Unavailable(reason) => {
                panel = panel.child(caption(reason.clone()));
            }
        }

        if let Some(label) = &info.context_label {
            panel = panel
                .child(div().h(px(1.)).bg(theme.border))
                .child(div().text_size(px(11.)).text_color(theme.text_secondary).child(format!("◎ {label}")));
        }
        panel = panel.child(div().text_size(px(10.)).text_color(theme.text_tertiary).child(if info.pinned {
            "⌘-click to navigate"
        } else {
            "⌘-click to navigate · click to keep open"
        }));

        let position = point(info.position.x + px(12.), info.position.y + px(16.));
        let _ = cx;
        Some(
            deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(panel))
                .into_any_element(),
        )
    }

    fn render_inlay_tooltip(&self, theme: &Theme) -> Option<gpui::AnyElement> {
        let (_, detail, position) = self.inlay_hover.as_ref()?;
        let panel = div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .max_w(px(640.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(6.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .font_family(crate::theme::MONO)
            .text_size(px(11.))
            .text_color(theme.text)
            .children(detail.lines().map(|line| div().child(line.to_string())));
        Some(
            deferred(
                anchored()
                    .position(point(position.x + px(12.), position.y + px(16.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(panel),
            )
            .into_any_element(),
        )
    }

    fn render_chooser(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let chooser = self.chooser.as_ref()?;
        let mut menu = div()
            .id("chooser")
            // The editor underneath dismisses transient UI on mouse-down, and
            // an element removed on mouse-down never sees the mouse-up that
            // fires its click. Keep the press to ourselves, and stop the rows
            // beneath from hovering through us.
            .occlude()
            .on_any_mouse_down(|_, _window, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .w(px(420.))
            .max_h(px(360.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .text_size(px(12.))
            .child(div().px(px(8.)).py(px(4.)).text_size(px(11.)).text_color(theme.text_secondary).child(format!("{} destinations", chooser.locations.len())));
        for (i, location) in chooser.locations.iter().take(40).enumerate() {
            let loc = location.clone();
            let eval = chooser.evaluation_id.clone();
            let mut entry = div()
                .id(ElementId::NamedInteger("choice".into(), i as u64))
                .relative()
                .child(crate::automation::probe(format!("choice-{i}")))
                .flex()
                .flex_col()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(5.))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.go(loc.clone(), eval.clone(), cx);
                }))
                .child(div().font_family(crate::theme::MONO).text_size(px(11.)).text_color(theme.text).child(location.title()));
            if let Some(secondary) = location.secondary() {
                entry = entry.child(div().font_family(crate::theme::MONO).text_size(px(10.)).text_color(theme.text_secondary).whitespace_nowrap().overflow_hidden().text_ellipsis().child(secondary));
            }
            menu = menu.child(entry);
        }
        let position = point(chooser.position.x + px(8.), chooser.position.y + px(14.));
        Some(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).into_any_element())
    }
}

/// Line numbers, pinned to the viewport's left edge over the scrolled rows.
#[allow(clippy::too_many_arguments)]
fn paint_gutter(
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    mono: gpui::Font,
    line_count: usize,
    gutter_width: Pixels,
    highlight_line: Option<usize>,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) {
    let scroll_y: f32 = offset.y.into();
    let view_h: f32 = bounds.size.height.into();
    let first = ((-scroll_y) / LINE_HEIGHT).floor().max(0.) as usize;
    let last = (((-scroll_y + view_h) / LINE_HEIGHT).ceil() as usize + 1).min(line_count);
    let row_top = |ix: usize| bounds.origin.y + px(ix as f32 * LINE_HEIGHT + scroll_y);
    let shape = |text: String, size: f32, color: gpui::Hsla, window: &Window| {
        let run = gpui::TextRun { len: text.len(), font: mono.clone(), color, background_color: None, underline: None, strikethrough: None };
        window.text_system().shape_line(text.into(), px(size), &[run], None)
    };

    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        // Gutter column: opaque so scrolled text never shows through it.
        window.paint_quad(gpui::fill(
            Bounds::new(bounds.origin, gpui::size(gutter_width, bounds.size.height)),
            theme.content_background,
        ));
        for ix in first..last {
            let y = row_top(ix);
            let color = if highlight_line == Some(ix) { theme.text } else { theme.text_tertiary };
            if highlight_line == Some(ix) {
                window.paint_quad(gpui::fill(
                    Bounds::new(gpui::point(bounds.origin.x, y), gpui::size(gutter_width, px(LINE_HEIGHT))),
                    theme.accent.opacity(0.14),
                ));
            }
            let line = shape((ix + 1).to_string(), 11., color, window);
            let x = bounds.origin.x + gutter_width - px(6.) - line.width;
            line.paint(gpui::point(x, y), px(LINE_HEIGHT), gpui::TextAlign::Left, None, window, cx).ok();
        }
    });
}

const PILL_HEIGHT: f32 = 15.;
const PILL_PAD: f32 = 5.;
const PILL_MARGIN: f32 = 3.;
/// The state icon at the pill's right: its box, and the gap before it.
const PILL_ICON: f32 = 10.;
const PILL_ICON_GAP: f32 = 5.;

/// A pill around `chars` characters of editor-sized text, border and (for
/// a condition pill) the state icon included. Given an exact width so
/// selection geometry can predict it from the font's advance instead of
/// asking the layout engine.
fn pill_width(chars: usize, char_width: Pixels, with_icon: bool) -> Pixels {
    let icon = if with_icon { PILL_ICON_GAP + PILL_ICON } else { 0. };
    char_width * chars as f32 + px(2. * PILL_PAD + 2. + icon)
}

/// Where a row's text departs from "column × advance": `advance` px are
/// inserted before `col`, standing in for `hidden` characters (zero for a
/// pill edge, the whole value for a pill showing its evaluated form).
#[derive(Clone, Copy, Debug)]
struct Gap {
    col: f32,
    hidden: f32,
    advance: f32,
}

fn inlay_gaps(inlay: &msbuild::Inlay, line: &str, evaluated: bool, cw: f32) -> Vec<Gap> {
    let col = |byte: usize| line[..byte.min(line.len())].chars().count() as f32;
    match &inlay.value {
        Some(value) if evaluated => {
            let shown = inlay.evaluated.as_deref().unwrap_or("").chars().count();
            let advance = f32::from(pill_width(shown, px(cw), true)) + 2. * PILL_MARGIN;
            vec![Gap { col: col(value.start), hidden: col(value.end) - col(value.start), advance }]
        }
        Some(value) => {
            let left = PILL_MARGIN + 1. + PILL_PAD;
            let right = left + PILL_ICON_GAP + PILL_ICON;
            vec![Gap { col: col(value.start), hidden: 0., advance: left }, Gap { col: col(value.end), hidden: 0., advance: right }]
        }
        None => {
            let advance = f32::from(pill_width(SKIPPED.chars().count(), px(cw), false)) + 2. * PILL_MARGIN;
            vec![Gap { col: col(inlay.at), hidden: 0., advance }]
        }
    }
}

/// Text-space x of a column once the row's gaps are applied. An
/// exclusive end that falls inside an inlay covers it; a start inside one
/// begins at its left edge.
fn x_of(col: f32, gaps: &[Gap], cw: f32, exclusive_end: bool) -> f32 {
    let mut shift = 0.;
    for g in gaps {
        if col >= g.col + g.hidden && (!exclusive_end || col > g.col) {
            shift += g.advance - g.hidden * cw;
        } else if col > g.col || (!exclusive_end && col >= g.col) {
            return g.col * cw + shift + if exclusive_end { g.advance } else { 0. };
        } else {
            break;
        }
    }
    col * cw + shift
}

/// The inverse: a row-space x back to text space, so `x / cw` is a
/// column. Points over an inlay land on its first column.
fn text_x(x: f32, gaps: &[Gap], cw: f32) -> f32 {
    let mut shift = 0.;
    for g in gaps {
        let left = g.col * cw + shift;
        if x < left {
            break;
        }
        if x < left + g.advance {
            return g.col * cw;
        }
        shift += g.advance - g.hidden * cw;
    }
    x - shift
}

/// What every row of one editor shares.
struct RowContext<'a> {
    cmd_down: bool,
    hover: Option<&'a Hover>,
    gutter_width: Pixels,
    char_width: Option<Pixels>,
    /// Find hits across the whole text, ascending; the current one is
    /// painted by the selection instead.
    matches: &'a [Range<usize>],
    current: Option<Range<usize>>,
}

/// One editor row: highlighted text with clickable tokens, split around
/// its inlay when it has one.
fn render_line(tab: &Tab, ix: usize, theme: &Theme, row: &RowContext, cx: &mut Context<SourceWell>) -> gpui::AnyElement {
    let range = tab.lines[ix].clone();
    let text = tab.line_text(ix).to_string();
    let text_len = text.len();

    let palette = |kind: HighlightKind| -> gpui::Hsla {
        let (light, dark) = match kind {
            HighlightKind::Text => (0x1F2328, 0xC9D1D9),
            HighlightKind::Punctuation => (0x8C959F, 0x6E7681),
            HighlightKind::ElementName => (0x116329, 0x7EE787),
            HighlightKind::AttributeName => (0x6639BA, 0xD2A8FF),
            HighlightKind::AttributeValue => (0x0A63C7, 0x79C0FF),
            HighlightKind::Comment => (0x6E7781, 0x8B949E),
            HighlightKind::Entity => (0x0F6E6E, 0x56D4DD),
            HighlightKind::Expression => (0x953800, 0xFFA657),
        };
        gpui::rgb(if theme.dark { dark } else { light }).into()
    };

    // The token under a ⌘-hover gets an underline; split runs around it.
    let underline: Option<Range<usize>> = row
        .hover
        .filter(|h| h.tab == tab.id && h.line == ix && row.cmd_down)
        .filter(|h| tab.semantics.as_ref().map_or(false, |s| s.is_navigable(&h.token)))
        .map(|h| {
            let start = h.token.range.start.saturating_sub(range.start).min(text_len);
            let end = h.token.range.end.saturating_sub(range.start).min(text_len);
            start..end
        });

    // Line-relative highlight runs.
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for (run, kind) in tab.highlights.runs(range.start..range.start + text_len) {
        let color = palette(kind);
        let mut push = |r: Range<usize>, underlined: bool| {
            if r.start >= r.end {
                return;
            }
            highlights.push((
                r,
                HighlightStyle {
                    color: Some(color),
                    underline: underlined.then(|| UnderlineStyle { thickness: px(1.), color: Some(theme.link), wavy: false }),
                    ..Default::default()
                },
            ));
        };
        match &underline {
            Some(u) if u.start < run.end && u.end > run.start => {
                push(run.start..u.start.max(run.start), false);
                push(u.start.max(run.start)..u.end.min(run.end), true);
                push(u.end.min(run.end)..run.end, false);
            }
            _ => push(run, false),
        }
    }

    // Find hits on this line get a wash behind them.
    let first = row.matches.partition_point(|m| m.end <= range.start);
    let washes: Vec<(Range<usize>, gpui::Hsla)> = row.matches[first..]
        .iter()
        .take_while(|m| m.start < range.start + text_len)
        .filter(|m| row.current.as_ref() != Some(*m))
        .map(|m| (m.start.saturating_sub(range.start)..(m.end - range.start).min(text_len), theme.highlight_background))
        .filter(|(r, _)| r.start < r.end && text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
        .collect();
    if !washes.is_empty() {
        highlights = overlay_backgrounds(highlights, &washes, text_len);
    }

    // Line-relative tokens.
    let line_tokens: Vec<(Range<usize>, Token)> = tab
        .semantics
        .as_ref()
        .map(|s| {
            s.tokens
                .iter()
                .filter(|t| t.range.start >= range.start && t.range.end <= range.start + text_len)
                .map(|t| (t.range.start - range.start..t.range.end - range.start, t.clone()))
                .collect()
        })
        .unwrap_or_default();

    let entity = cx.entity();
    let tab_id = tab.id.clone();
    // One interactive text per piece of the line; hover and click offsets
    // are re-based so the well still sees whole-line positions.
    let piece = |k: usize, piece: Range<usize>| -> gpui::AnyElement {
        let body = text[piece.clone()].to_string();
        let highlights: Vec<(Range<usize>, HighlightStyle)> = highlights
            .iter()
            .filter_map(|(r, style)| {
                let start = r.start.max(piece.start);
                let end = r.end.min(piece.end);
                (start < end).then(|| (start - piece.start..end - piece.start, style.clone()))
            })
            .collect();
        let tokens: Vec<Token> = line_tokens
            .iter()
            .filter(|(r, _)| r.start >= piece.start && r.end <= piece.end)
            .map(|(_, t)| t.clone())
            .collect();
        let click_ranges: Vec<Range<usize>> = line_tokens
            .iter()
            .filter(|(r, _)| r.start >= piece.start && r.end <= piece.end)
            .map(|(r, _)| r.start - piece.start..r.end - piece.start)
            .collect();
        let start = piece.start;
        let hover_entity = entity.clone();
        let hover_tab = tab_id.clone();
        let mut el = InteractiveText::new(
            ElementId::Name(format!("line-{ix}-{k}").into()),
            StyledText::new(body).with_highlights(highlights),
        )
        .on_hover(move |char_ix, event, _window, cx| {
            let char_ix = char_ix.map(|i| i + start);
            hover_entity.update(cx, |this, cx| this.on_line_hover(&hover_tab, ix, char_ix, &event, cx));
        });
        if !click_ranges.is_empty() {
            let entity = entity.clone();
            let tab_id = tab_id.clone();
            el = el.on_click(click_ranges, move |range_ix, window, cx| {
                let Some(token) = tokens.get(range_ix).cloned() else { return };
                let hover = Hover { tab: tab_id.clone(), line: ix, token };
                // A token click is not a pill click.
                cx.stop_propagation();
                entity.update(cx, |this, cx| this.on_token_click(hover, window, cx));
            });
        }
        el.into_any_element()
    };

    let inlay = tab.inlays.get(&ix).filter(|i| {
        i.at <= text_len
            && text.is_char_boundary(i.at)
            && i.value.as_ref().map_or(true, |v| v.end <= text_len && text.is_char_boundary(v.start) && text.is_char_boundary(v.end))
    });
    let mut content = div().pl(px(8.)).flex_none().flex().items_center();
    match inlay {
        Some(inlay) => match &inlay.value {
            Some(value) => {
                let evaluated = tab.evaluated_pills.contains(&ix);
                if value.start > 0 {
                    content = content.child(piece(0, 0..value.start));
                }
                let inner = (!evaluated).then(|| piece(1, value.clone()));
                let chars = text[value.clone()].chars().count();
                content = content.child(render_pill(ix, tab_id.clone(), inlay, chars, inner, row.char_width, theme, cx));
                if value.end < text_len {
                    content = content.child(piece(2, value.end..text_len));
                }
            }
            None => {
                if inlay.at > 0 {
                    content = content.child(piece(0, 0..inlay.at));
                }
                content = content.child(render_skipped(ix, inlay, row.char_width, theme, cx));
                if inlay.at < text_len {
                    content = content.child(piece(1, inlay.at..text_len));
                }
            }
        },
        None => content = content.child(piece(0, 0..text_len)),
    }

    let highlighted = tab.highlight_line == Some(ix);
    // The gutter is painted by the overlay (pinned to the viewport); rows
    // just leave room for it.
    div()
        .id(ElementId::NamedInteger("row".into(), ix as u64))
        .relative()
        .child(crate::automation::probe(format!("line-{}", ix + 1)))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .when(highlighted, |d| d.bg(theme.accent.opacity(0.14)))
        .child(div().w(row.gutter_width).flex_none())
        .child(content)
        .into_any_element()
}

/// The chip every inlay is drawn in.
fn pill_shell(id: ElementId, width: Pixels, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .mx(px(PILL_MARGIN))
        .px(px(PILL_PAD))
        .w(width)
        .h(px(PILL_HEIGHT))
        .rounded(px(PILL_HEIGHT / 2.))
        .bg(theme.chip_background)
        .border_1()
        .border_color(theme.border)
}

/// The pill: the Condition expression in a chip, or — after a click — what
/// it evaluated to. `inner` is the expression piece; `None` means show the
/// evaluated form instead. The icon at the right says which.
#[allow(clippy::too_many_arguments)]
fn render_pill(
    ix: usize,
    tab_id: String,
    inlay: &msbuild::Inlay,
    value_chars: usize,
    inner: Option<gpui::AnyElement>,
    char_width: Option<Pixels>,
    theme: &Theme,
    cx: &mut Context<SourceWell>,
) -> gpui::AnyElement {
    // Before the first frame has measured the font, guess; the row is
    // re-laid out with the real advance right after.
    let char_width = char_width.unwrap_or(px(FONT_SIZE * 0.6));
    let evaluated = inlay.evaluated.clone().unwrap_or_default();
    let width = match &inner {
        Some(_) => pill_width(value_chars, char_width, true),
        None => pill_width(evaluated.chars().count(), char_width, true),
    };
    let is_evaluated = inner.is_none();
    let mut pill = pill_shell(ElementId::NamedInteger("pill".into(), ix as u64), width, theme)
        .relative()
        .child(crate::automation::probe(format!("pill-{}", ix + 1)))
        .debug_selector(move || format!("pill-{}", ix + 1))
        .hover(|s| s.border_color(theme.accent))
        .cursor(CursorStyle::PointingHand)
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| this.toggle_pill(&tab_id, ix, cx)));
    pill = match inner {
        Some(inner) => pill.child(inner),
        None => pill.text_color(theme.text).child(evaluated),
    };
    pill.child(render_pill_icon(is_evaluated, theme)).into_any_element()
}

/// `<>` while the pill shows the source expression, `=` once it shows the
/// evaluated one.
fn render_pill_icon(evaluated: bool, theme: &Theme) -> gpui::AnyElement {
    let color = theme.text_tertiary;
    div()
        .flex_none()
        .ml(px(PILL_ICON_GAP))
        .w(px(PILL_ICON))
        .h(px(PILL_ICON))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window: &mut Window, _: &mut App| {
                    let o = bounds.origin;
                    let w = f32::from(bounds.size.width);
                    let h = f32::from(bounds.size.height);
                    if evaluated {
                        let bar = |y: f32| Bounds::new(point(o.x + px(1.), o.y + px(y)), gpui::size(px(w - 2.), px(1.5)));
                        window.paint_quad(gpui::fill(bar(h * 0.5 - 2.5), color));
                        window.paint_quad(gpui::fill(bar(h * 0.5 + 1.), color));
                    } else {
                        let mut stroke = gpui::PathBuilder::stroke(px(1.2));
                        stroke.move_to(point(o.x + px(3.5), o.y + px(1.5)));
                        stroke.line_to(point(o.x + px(0.8), o.y + px(h * 0.5)));
                        stroke.line_to(point(o.x + px(3.5), o.y + px(h - 1.5)));
                        stroke.move_to(point(o.x + px(w - 3.5), o.y + px(1.5)));
                        stroke.line_to(point(o.x + px(w - 0.8), o.y + px(h * 0.5)));
                        stroke.line_to(point(o.x + px(w - 3.5), o.y + px(h - 1.5)));
                        if let Ok(path) = stroke.build() {
                            window.paint_path(path, color);
                        }
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

const SKIPPED: &str = "skipped";

/// An import MSBuild skipped without evaluating a condition (a missing
/// file, say): nothing to flip, the reason is a hover away.
fn render_skipped(
    ix: usize,
    inlay: &msbuild::Inlay,
    char_width: Option<Pixels>,
    theme: &Theme,
    cx: &mut Context<SourceWell>,
) -> gpui::AnyElement {
    let char_width = char_width.unwrap_or(px(FONT_SIZE * 0.6));
    let detail = inlay.detail.clone();
    let entity = cx.entity();
    pill_shell(ElementId::NamedInteger("skipped".into(), ix as u64), pill_width(SKIPPED.chars().count(), char_width, false), theme)
        .text_color(theme.text_secondary)
        .cursor(CursorStyle::Arrow)
        .on_hover(move |hovered, window, cx| {
            let position = window.mouse_position();
            let detail = detail.clone();
            entity.update(cx, |this, cx| {
                if *hovered {
                    this.inlay_hover = Some((ix, detail, position));
                } else if this.inlay_hover.as_ref().is_some_and(|(line, ..)| *line == ix) {
                    this.inlay_hover = None;
                }
                cx.notify();
            });
        })
        .child(SKIPPED)
        .into_any_element()
}

impl Render for SourceWell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        if std::mem::take(&mut self.refocus_editor) {
            self.focus_handle.focus(window, cx);
        }
        let tab_bar = self.render_tab_bar(&theme, cx);

        let mut body = div().flex().flex_col().size_full().min_h_0();
        if self.selected == 0 || self.selected > self.tabs.len() {
            self.selected = 0;
            body = body.child(div().size_full().child(self.inspector.clone()));
        } else {
            let tab_ix = self.selected - 1;
            // The find bar follows the selected tab.
            if self.find.as_ref().is_some_and(|f| f.tab != self.tabs[tab_ix].id) {
                if let Some(find) = self.find.as_mut() {
                    find.matches.clear();
                    find.current = None;
                }
                self.refresh_find(false, cx);
            }
            if let Some(bar) = self.render_context_bar(tab_ix, &theme, cx) {
                body = body.child(bar);
            }
            if let Some(bar) = self.render_find_bar(&theme, cx) {
                body = body.child(bar);
            }
            body = body.child(div().flex_1().min_h_0().child(self.render_editor(tab_ix, &theme, cx)));
        }

        let mut root = div()
            .id("source-well")
            .key_context(WELL_CONTEXT)
            // A click on the tab strip or the find bar keeps focus in the
            // well rather than handing it to the workspace root.
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::open_find))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::close_find))
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(theme.sidebar_background)
            // A pinned popover or chooser goes away when the click lands
            // anywhere else in the window, like an NSPopover.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                if this.quick_info.is_some() || this.chooser.is_some() || this.context_menu_open {
                    this.dismiss(cx);
                }
            }))
            .child(tab_bar)
            .child(div().flex_1().min_h_0().w_full().child(body));

        if let Some(message) = &self.message {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(12.))
                    .left(px(12.))
                    .right(px(12.))
                    .p(px(8.))
                    .rounded(px(6.))
                    .bg(theme.window_background)
                    .border_1()
                    .border_color(theme.warning)
                    .text_size(px(12.))
                    .text_color(theme.text)
                    .child(message.clone()),
            );
        }
        if self.selected > 0 {
            if let Some(menu) = self.render_context_menu(self.selected - 1, &theme, cx) {
                root = root.child(menu);
            }
        }
        if let Some(info) = self.render_quick_info(&theme, cx) {
            root = root.child(info);
        }
        if let Some(chooser) = self.render_chooser(&theme, cx) {
            root = root.child(chooser);
        }
        if let Some(tooltip) = self.render_inlay_tooltip(&theme) {
            root = root.child(tooltip);
        }
        root
    }
}

#[allow(dead_code)]
fn _unused(_: &App) {}

/// Paints the selected span, one rectangle per visible line. Offsets are
/// bytes into the whole document; the font is monospaced, so a column is
/// just a character count.
#[allow(clippy::too_many_arguments)]
fn paint_selection(
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    char_width: Pixels,
    gutter_width: Pixels,
    lines: &[Range<usize>],
    text: &str,
    gaps: &HashMap<usize, Vec<Gap>>,
    selection: Range<usize>,
    theme: &Theme,
    window: &mut Window,
) {
    let scroll_y: f32 = offset.y.into();
    let view_h: f32 = bounds.size.height.into();
    let first = ((-scroll_y) / LINE_HEIGHT).floor().max(0.) as usize;
    let last = (((-scroll_y + view_h) / LINE_HEIGHT).ceil() as usize + 1).min(lines.len());
    let text_left = bounds.origin.x + gutter_width + px(8.) + offset.x;
    let color = theme.accent.opacity(if theme.dark { 0.35 } else { 0.25 });

    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        for ix in first..last {
            let line = &lines[ix];
            // A newline is inside the selection but past the line's end;
            // show it as a sliver so multi-line drags read as continuous.
            let start = selection.start.max(line.start);
            let end = selection.end.min(line.end + 1);
            if start >= end {
                continue;
            }
            let column = |offset: usize| {
                let clamped = offset.clamp(line.start, line.end);
                text[line.start..clamped].chars().count() as f32
            };
            let from = column(start);
            let to = column(end);
            let trailing = if selection.end > line.end { 0.6 } else { 0. };
            // An inlay on this line pushes the columns after it right; a
            // selection that spans it simply covers it too.
            let cw: f32 = char_width.into();
            let line_gaps = gaps.get(&ix).map(Vec::as_slice).unwrap_or(&[]);
            let left = px(x_of(from, line_gaps, cw, false));
            let right = px(x_of(to, line_gaps, cw, true)) + char_width * trailing;
            if right <= left {
                continue;
            }
            let y = bounds.origin.y + px(ix as f32 * LINE_HEIGHT + scroll_y);
            window.paint_quad(gpui::fill(
                Bounds::new(point(text_left + left, y), gpui::size(right - left, px(LINE_HEIGHT))),
                color,
            ));
        }
    });
}

// ----- find helpers -----

/// Case-insensitive for ASCII queries (the overwhelming case in build
/// files), exact otherwise, so byte offsets stay honest.
fn find_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    const LIMIT: usize = 20_000;
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if query.is_ascii() {
        let q = query.as_bytes();
        let t = text.as_bytes();
        let mut i = 0;
        while i + q.len() <= t.len() && out.len() < LIMIT {
            if t[i..i + q.len()].eq_ignore_ascii_case(q) && text.is_char_boundary(i) && text.is_char_boundary(i + q.len()) {
                out.push(i..i + q.len());
                i += q.len();
            } else {
                i += 1;
            }
        }
    } else {
        out.extend(text.match_indices(query).take(LIMIT).map(|(i, m)| i..i + m.len()));
    }
    out
}

/// Lay background colours over existing highlight runs. `StyledText`
/// wants runs sorted and disjoint, so both sets are cut at every edge.
fn overlay_backgrounds(
    runs: Vec<(Range<usize>, HighlightStyle)>,
    backgrounds: &[(Range<usize>, gpui::Hsla)],
    len: usize,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut cuts: Vec<usize> = vec![0, len];
    cuts.extend(runs.iter().flat_map(|(r, _)| [r.start, r.end]));
    cuts.extend(backgrounds.iter().flat_map(|(r, _)| [r.start, r.end]));
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let mut style = runs.iter().find(|(r, _)| r.start <= a && b <= r.end).map(|(_, s)| s.clone()).unwrap_or_default();
            if let Some((_, color)) = backgrounds.iter().find(|(r, _)| r.start <= a && b <= r.end) {
                style.background_color = Some(*color);
            }
            (a..b, style)
        })
        .collect()
}
