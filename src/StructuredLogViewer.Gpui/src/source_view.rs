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
use crate::theme::Theme;
use gpui::{
    App, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, ElementId, Entity, EventEmitter,
    FocusHandle, FontWeight, HighlightStyle, InteractiveText, ListHorizontalSizingBehavior,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollStrategy, ScrollWheelEvent, StyledText, Task, UnderlineStyle, UniformListScrollHandle,
    Window, actions, anchored, canvas, deferred, div, point, prelude::*, px, uniform_list,
};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

actions!(source_editor, [CopySelection, SelectAll]);

pub const KEY_CONTEXT: &str = "SourceEditor";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding as K;
    let c = Some(KEY_CONTEXT);
    vec![K::new("cmd-c", CopySelection, c), K::new("cmd-a", SelectAll, c)]
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
    annotations: Arc<HashMap<usize, String>>,
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
    /// gutter and the inlays are painted, never part of the text, so a
    /// selection cannot pick them up.
    selection: Option<Selection>,
    selecting: bool,
    /// Where the editor laid out last frame, so a mouse position can be
    /// turned into an offset into the text.
    geometry: Option<EditorGeometry>,
    message: Option<String>,
    generation: u64,
}

impl EventEmitter<SourceEvent> for SourceWell {}

impl SourceWell {
    pub fn new(session: Arc<Session>, inspector: Entity<Inspector>, cx: &mut Context<Self>) -> Self {
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
                    annotations: Arc::new(HashMap::new()),
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
                            let annotations = msbuild::import_annotations(&text, &index.skipped_imports());
                            anyhow::Ok((index, annotations))
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
                    Ok((index, annotations)) => {
                        eprintln!(
                            "semantics({}): {} tokens, {} imports, {} skipped, {} annotations, contexts {}/{}",
                            tab.path,
                            index.tokens.len(),
                            index.file.imports.len(),
                            index.file.skipped_imports.len(),
                            annotations.len(),
                            index.file.contexts.len(),
                            index.file.contexts_total
                        );
                        tab.evaluation_id = index.file.evaluation_id.clone();
                        tab.contexts = index.file.contexts.clone();
                        tab.contexts_total = index.file.contexts_total.max(tab.contexts.len());
                        tab.annotations = Arc::new(annotations);
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
        // Clicks land on tokens first (which stop propagation); anything
        // else dismisses transient UI.
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

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.selected.checked_sub(1).and_then(|ix| self.tabs.get(ix)) else { return };
        self.selection = Some(Selection { tab: tab.id.clone(), anchor: 0, head: tab.text.len() });
        cx.notify();
    }

    fn on_scroll(&mut self, _: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_info.is_some() || self.chooser.is_some() {
            self.dismiss(cx);
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
        let label = tab.selected_context().map(|c| c.label.clone()).unwrap_or_else(|| "Choose an evaluation".into());
        let total = tab.contexts_total;
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
                .when(total > 1, |d| d.child(div().ml_auto().text_color(theme.text_tertiary).child(format!("{total} evaluations"))))
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
        for (i, context) in tab.contexts.iter().enumerate() {
            let is_current = Some(&context.evaluation_id) == current.as_ref();
            let eval = context.evaluation_id.clone();
            menu = menu.child(
                div()
                    .id(ElementId::NamedInteger("ctx".into(), i as u64))
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
                let Some(tab) = this.tabs.iter().find(|t| t.id == tab_id_for_rows) else { return Vec::new() };
                let mut items = Vec::with_capacity(range.len());
                for ix in range {
                    if ix >= tab.lines.len() {
                        break;
                    }
                    items.push(render_line(tab, ix, &theme, cmd_down, hover.as_ref(), gutter_width, cx));
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
            let annotations = tab.annotations.clone();
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
                    paint_gutter_and_notes(
                        bounds, offset, mono, line_count, gutter_width, &annotations, highlight_line, &theme, window, cx,
                    )
                },
            )
            .absolute()
            .inset_0()
        };

        div()
            .id(ElementId::Name(format!("editor:{tab_id}").into()))
            .size_full()
            .relative()
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

    fn render_chooser(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let chooser = self.chooser.as_ref()?;
        let mut menu = div()
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

/// Viewport-pinned chrome over the scrolling lines: the line-number gutter
/// and the skipped-import notes, which sit against the trailing edge like
/// the Mac editor's pills (it wraps; we scroll, so anchoring to the row's
/// end would put them off screen on long lines).
#[allow(clippy::too_many_arguments)]
fn paint_gutter_and_notes(
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    mono: gpui::Font,
    line_count: usize,
    gutter_width: Pixels,
    annotations: &HashMap<usize, String>,
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

        for ix in first..last {
            let Some(note) = annotations.get(&ix) else { continue };
            let line = shape(note.clone(), 11., theme.text_secondary, window);
            let pad_x = px(7.);
            let pill_w = line.width + pad_x * 2.;
            let pill_h = px(15.);
            let x = bounds.origin.x + bounds.size.width - px(12.) - pill_w;
            let y = row_top(ix) + px((LINE_HEIGHT - 15.) / 2.);
            let rect = Bounds::new(gpui::point(x, y), gpui::size(pill_w, pill_h));
            window.paint_quad(gpui::quad(
                rect,
                pill_h / 2.,
                theme.chip_background,
                gpui::Edges::all(px(1.)),
                theme.border,
                gpui::BorderStyle::Solid,
            ));
            line.paint(gpui::point(x + pad_x, y), pill_h, gpui::TextAlign::Left, None, window, cx).ok();
        }
    });
}

/// One editor row: highlighted text with clickable tokens.
fn render_line(
    tab: &Tab,
    ix: usize,
    theme: &Theme,
    cmd_down: bool,
    hover: Option<&Hover>,
    gutter_width: Pixels,
    cx: &mut Context<SourceWell>,
) -> gpui::AnyElement {
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
    let underline: Option<Range<usize>> = hover
        .filter(|h| h.tab == tab.id && h.line == ix && cmd_down)
        .filter(|h| tab.semantics.as_ref().map_or(false, |s| s.is_navigable(&h.token)))
        .map(|h| {
            let start = h.token.range.start.saturating_sub(range.start).min(text_len);
            let end = h.token.range.end.saturating_sub(range.start).min(text_len);
            start..end
        });

    let mut highlights = Vec::new();
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

    let tokens: Vec<Token> = tab
        .semantics
        .as_ref()
        .map(|s| {
            s.tokens
                .iter()
                .filter(|t| t.range.start >= range.start && t.range.end <= range.start + text_len)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let click_ranges: Vec<Range<usize>> =
        tokens.iter().map(|t| t.range.start - range.start..t.range.end - range.start).collect();

    let tab_id = tab.id.clone();
    let entity = cx.entity();
    let hover_entity = entity.clone();
    let hover_tab = tab_id.clone();
    let click_tokens = tokens.clone();

    let mut text_el = InteractiveText::new(
        ElementId::NamedInteger("line".into(), ix as u64),
        StyledText::new(text).with_highlights(highlights),
    )
    .on_hover(move |char_ix, event, _window, cx| {
        hover_entity.update(cx, |this, cx| this.on_line_hover(&hover_tab, ix, char_ix, &event, cx));
    });
    if !click_ranges.is_empty() {
        text_el = text_el.on_click(click_ranges, move |range_ix, window, cx| {
            let Some(token) = click_tokens.get(range_ix).cloned() else { return };
            let hover = Hover { tab: tab_id.clone(), line: ix, token };
            cx.stop_propagation();
            entity.update(cx, |this, cx| this.on_token_click(hover, window, cx));
        });
    }

    let highlighted = tab.highlight_line == Some(ix);
    // The gutter and the end-of-element notes are painted by the overlay
    // (pinned to the viewport); rows just leave room for the gutter.
    div()
        .id(ElementId::NamedInteger("row".into(), ix as u64))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .when(highlighted, |d| d.bg(theme.accent.opacity(0.14)))
        .child(div().w(gutter_width).flex_none())
        .child(div().pl(px(8.)).flex_none().child(text_el))
        .into_any_element()
}

impl Render for SourceWell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let tab_bar = self.render_tab_bar(&theme, cx);

        let mut body = div().flex().flex_col().size_full().min_h_0();
        if self.selected == 0 || self.selected > self.tabs.len() {
            self.selected = 0;
            body = body.child(div().size_full().child(self.inspector.clone()));
        } else {
            let tab_ix = self.selected - 1;
            if let Some(bar) = self.render_context_bar(tab_ix, &theme, cx) {
                body = body.child(bar);
            }
            body = body.child(div().flex_1().min_h_0().child(self.render_editor(tab_ix, &theme, cx)));
        }

        let mut root = div()
            .id("source-well")
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
            let width = (to - from + trailing).max(0.);
            if width <= 0. {
                continue;
            }
            let y = bounds.origin.y + px(ix as f32 * LINE_HEIGHT + scroll_y);
            window.paint_quad(gpui::fill(
                Bounds::new(
                    point(text_left + char_width * from, y),
                    gpui::size(char_width * width, px(LINE_HEIGHT)),
                ),
                color,
            ));
        }
    });
}
