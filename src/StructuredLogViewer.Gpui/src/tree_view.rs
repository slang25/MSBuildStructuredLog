//! The build tree: a `uniform_list` over `TreeModel`, with children fetched
//! from the bridge off-thread on expand. Keyboard and mouse behaviour
//! follows NSOutlineView (arrows walk and fold, double-click toggles,
//! selection follows focus colour).

use crate::engine::Session;
use crate::favorites::Favorites;
use crate::model::SharedNode;
use crate::styling::{SegmentStyle, segments, state_accent, style_for};
use crate::theme::Theme;
use crate::tree::TreeModel;
use gpui::{
    App, ClickEvent, ClipboardItem, Context, CursorStyle, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, MouseButton, MouseDownEvent, Pixels, ScrollStrategy,
    UniformListScrollHandle, Window, actions, anchored, deferred, div, prelude::*, px, uniform_list,
};
use std::sync::Arc;

actions!(
    build_tree,
    [
        SelectNext,
        SelectPrevious,
        ExpandRow,
        CollapseRow,
        ToggleRow,
        CopyRow,
        SelectFirst,
        SelectLast,
        ViewSource,
        ToggleFavorite,
        CloseMenu
    ]
);

pub const KEY_CONTEXT: &str = "BuildTree";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding as K;
    let c = Some(KEY_CONTEXT);
    vec![
        K::new("down", SelectNext, c),
        K::new("up", SelectPrevious, c),
        K::new("right", ExpandRow, c),
        K::new("left", CollapseRow, c),
        K::new("enter", ToggleRow, c),
        K::new("space", ViewSource, c),
        K::new("cmd-c", CopyRow, c),
        K::new("home", SelectFirst, c),
        K::new("end", SelectLast, c),
        K::new("cmd-enter", ViewSource, c),
        K::new("cmd-d", ToggleFavorite, c),
        K::new("escape", CloseMenu, c),
    ]
}

pub const ROW_HEIGHT: f32 = 22.;
const INDENT: f32 = 16.;

pub enum TreeEvent {
    Selected(SharedNode),
    ViewSource(SharedNode),
    /// The Project or ProjectEvaluation the selection sits under, which the
    /// Properties and items pane scopes its queries to. None outside one.
    ProjectContext(Option<SharedNode>),
    /// Inline the node's imports into a new tab in the document well.
    Preprocess(SharedNode),
    /// Add a clause to the Search Log query, and show that pane.
    AppendSearch(String),
    /// Replace the Search Log query, and show that pane.
    SetSearch(String),
    ShowInTimeline(SharedNode),
}

/// An open right-click menu: which row it belongs to and where it was
/// summoned, in window coordinates.
struct ContextMenu {
    ix: usize,
    node: SharedNode,
    position: gpui::Point<Pixels>,
}

pub struct TreeView {
    session: Arc<Session>,
    model: TreeModel,
    favorites: Entity<Favorites>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
    /// Bridge sort mode per node id, for rows the user re-sorted.
    /// 0 natural, 1 by name, 2 by duration; absent means natural.
    sort: std::collections::HashMap<String, i32>,
    menu: Option<ContextMenu>,
    focus_handle: FocusHandle,
}

impl EventEmitter<TreeEvent> for TreeView {}

impl Focusable for TreeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TreeView {
    pub fn new(
        session: Arc<Session>,
        root: crate::model::NodeSummary,
        favorites: Entity<Favorites>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&favorites, |_, _, cx| cx.notify()).detach();
        let mut this = TreeView {
            session,
            model: TreeModel::new(root),
            favorites,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            scrollbars: crate::scrollbar::Scrollbars::new(),
            sort: std::collections::HashMap::new(),
            menu: None,
            focus_handle: cx.focus_handle(),
        };
        this.expand(0, cx);
        this
    }

    // ----- model mutations -----

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.model.len() {
            return;
        }
        self.selected = Some(ix);
        if let Some(row) = self.model.row(ix) {
            cx.emit(TreeEvent::Selected(row.node.clone()));
        }
        cx.emit(TreeEvent::ProjectContext(self.project_context(ix)));
        cx.notify();
    }

    /// The nearest Project at or above `ix`, else the nearest
    /// ProjectEvaluation — the WPF viewer's `UpdateProjectContext`. Every
    /// ancestor of a visible row is expanded, so this needs no bridge call.
    fn project_context(&self, ix: usize) -> Option<SharedNode> {
        let mut evaluation = None;
        let mut at = Some(ix);
        while let Some(i) = at {
            let row = self.model.row(i)?;
            match row.node.kind.as_str() {
                "Project" => return Some(row.node.clone()),
                "ProjectEvaluation" if evaluation.is_none() => evaluation = Some(row.node.clone()),
                _ => {}
            }
            at = self.model.parent_index(i);
        }
        evaluation
    }

    fn on_close_menu(&mut self, _: &CloseMenu, _: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
    }

    fn toggle_favorite(&mut self, _: &ToggleFavorite, _: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.selected.and_then(|ix| self.model.row(ix)).map(|r| r.node.clone()) else { return };
        self.favorites.update(cx, |favorites, cx| favorites.toggle(node, cx));
    }

    fn select_and_reveal(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.select(ix, cx);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
    }

    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        match self.model.row(ix) {
            Some(row) if row.expanded => {
                let keep = self.selected.filter(|&s| s > ix);
                self.model.collapse(ix);
                if keep.is_some() && self.selected.map_or(false, |s| s >= self.model.len() || s > ix) {
                    // The selection was folded away; park it on the parent.
                    self.select(ix, cx);
                }
                cx.notify();
            }
            Some(_) => self.expand(ix, cx),
            None => {}
        }
    }

    /// Fetches children off-thread and splices them in when they arrive.
    fn expand(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.model.needs_fetch(ix) {
            return;
        }
        self.model.mark_loading(ix);
        cx.notify();
        let id = self.model.row(ix).map(|r| r.node.id.clone()).unwrap_or_default();
        let sort = self.sort.get(&id).copied().unwrap_or(0);
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.all_children(&id, sort).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(children) => {
                        this.model.apply_children(&id, children);
                    }
                    Err(err) => {
                        eprintln!("children({id}): {err:#}");
                        if let Some(ix) = this.model.index_of(&id) {
                            this.model.apply_children(&id, Vec::new());
                            this.model.collapse(ix);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Expands every ancestor of `node_id`, then selects and scrolls to it.
    pub fn reveal(&mut self, node_id: String, cx: &mut Context<Self>) {
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let chain = match session.ancestors(&node_id).await {
                Ok(a) => a.chain,
                Err(err) => {
                    eprintln!("ancestors({node_id}): {err:#}");
                    return;
                }
            };
            let Some((_, ancestors)) = chain.split_last() else { return };
            for ancestor in ancestors {
                let aid = ancestor.id.clone();
                // Fetch whenever the ancestor isn't expanded yet, even if an
                // expand is already in flight: apply_children is idempotent
                // for an expanded row, so whichever lands second is a no-op.
                let needs = this
                    .update(cx, |this, _| {
                        this.model.index_of(&aid).map(|ix| !this.model.row(ix).map_or(true, |r| r.expanded))
                    })
                    .ok()
                    .flatten();
                match needs {
                    Some(true) => {
                        match session.all_children(&aid, 0).await {
                            Ok(children) => {
                                if this
                                    .update(cx, |this, cx| {
                                        this.model.apply_children(&aid, children);
                                        cx.notify();
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Err(err) => {
                                eprintln!("children({aid}): {err:#}");
                                return;
                            }
                        }
                    }
                    Some(false) => {}
                    None => return,
                }
            }
            this.update(cx, |this, cx| {
                if let Some(ix) = this.model.index_of(&node_id) {
                    this.select(ix, cx);
                    this.scroll.scroll_to_item(ix, ScrollStrategy::Center);
                }
            })
            .ok();
        })
        .detach();
    }

    // ----- actions -----

    fn current(&self) -> usize {
        self.selected.unwrap_or(0).min(self.model.len().saturating_sub(1))
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let next = match self.selected {
            None => 0,
            Some(ix) => (ix + 1).min(self.model.len().saturating_sub(1)),
        };
        self.select_and_reveal(next, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let prev = self.selected.map_or(0, |ix| ix.saturating_sub(1));
        self.select_and_reveal(prev, cx);
    }

    fn select_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.select_and_reveal(0, cx);
    }

    fn select_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.select_and_reveal(self.model.len().saturating_sub(1), cx);
    }

    fn expand_row(&mut self, _: &ExpandRow, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.current();
        if self.selected.is_none() {
            self.select_and_reveal(0, cx);
            return;
        }
        match self.model.row(ix) {
            Some(row) if row.has_children() && !row.expanded => self.expand(ix, cx),
            Some(row) if row.expanded && ix + 1 < self.model.len() => self.select_and_reveal(ix + 1, cx),
            _ => {}
        }
    }

    fn collapse_row(&mut self, _: &CollapseRow, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.current();
        if self.selected.is_none() {
            return;
        }
        if self.model.row(ix).map_or(false, |r| r.expanded) {
            self.toggle(ix, cx);
        } else if let Some(parent) = self.model.parent_index(ix) {
            self.select_and_reveal(parent, cx);
        }
    }

    fn toggle_row(&mut self, _: &ToggleRow, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_some() {
            self.toggle(self.current(), cx);
        }
    }

    fn view_source(&mut self, _: &ViewSource, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.open_or_toggle(ix, cx);
        }
    }

    /// Space and double-click: the node's source when it has one (an error
    /// at its line, a project's file, an import's target); otherwise the
    /// next most useful thing, folding the row.
    fn open_or_toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.model.row(ix) else { return };
        if row.node.has_source {
            cx.emit(TreeEvent::ViewSource(row.node.clone()));
        } else {
            self.toggle(ix, cx);
        }
    }

    fn copy_row(&mut self, _: &CopyRow, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.selected.and_then(|ix| self.model.row(ix)) {
            cx.write_to_clipboard(ClipboardItem::new_string(row.node.title.clone()));
        }
    }

    fn on_row_click(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        self.select(ix, cx);
        if let ClickEvent::Mouse(mouse) = event
            && mouse.down.click_count == 2
        {
            self.open_or_toggle(ix, cx);
        }
    }
}

impl Render for TreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let focused = self.focus_handle.is_focused(window);
        let count = self.model.len();

        div()
            .id("build-tree")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::expand_row))
            .on_action(cx.listener(Self::collapse_row))
            .on_action(cx.listener(Self::toggle_row))
            .on_action(cx.listener(Self::copy_row))
            .on_action(cx.listener(Self::view_source))
            .on_action(cx.listener(Self::toggle_favorite))
            .on_action(cx.listener(Self::on_close_menu))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _window, cx| this.close_menu(cx)))
            .size_full()
            .relative()
            .bg(theme.content_background)
            .child(self.scrollbars.render(&self.scroll.0.borrow().base_handle, &theme))
            .children(self.render_menu(&theme, cx))
            .child(
                uniform_list(
                    "tree-rows",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        let mut items = Vec::with_capacity(range.len());
                        for ix in range {
                            let Some(row) = this.model.row(ix) else { continue };
                            let node = row.node.clone();
                            let depth = row.depth;
                            let expanded = row.expanded;
                            let loading = row.loading;
                            let has_children = row.has_children();
                            let selected = this.selected == Some(ix);
                            let on_accent = selected && focused;
                            let favorite = this.favorites.read(cx).contains(&node.id);

                            let style = style_for(&node, &theme);
                            let surface = if selected {
                                if focused { theme.selection } else { theme.selection_inactive }
                            } else {
                                theme.content_background
                            };
                            let primary = if on_accent {
                                theme.text_on_accent
                            } else {
                                match node.kind.as_str() {
                                    "Error" => theme.error,
                                    "Warning" => theme.warning,
                                    _ => state_accent(&node, &theme).unwrap_or(theme.text),
                                }
                            };
                            let secondary = if on_accent { theme.text_on_accent.opacity(0.75) } else { theme.text_secondary };

                            let mut el = div()
                                .id(ix)
                                .h(px(ROW_HEIGHT))
                                .w_full()
                                .flex()
                                .items_center()
                                .pl(px(6.))
                                .pr(px(10.))
                                .text_color(primary)
                                .when(selected, |d| {
                                    d.bg(if focused { theme.selection } else { theme.selection_inactive })
                                })
                                .when(!selected, |d| d.hover(|s| s.bg(theme.hover)))
                                .when(node.is_low_relevance && !selected, |d| d.opacity(0.6))
                                .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                                    this.on_row_click(ix, event, cx);
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.open_menu(ix, event.position, cx);
                                    }),
                                );

                            for _ in 0..depth {
                                el = el.child(
                                    div().w(px(INDENT)).h_full().flex_none().flex().justify_center().child(
                                        div().w(px(1.)).h_full().bg(if on_accent {
                                            theme.text_on_accent.opacity(0.25)
                                        } else {
                                            theme.guide
                                        }),
                                    ),
                                );
                            }

                            let chevron = if has_children {
                                div()
                                    .id("chevron")
                                    .w(px(INDENT))
                                    .h_full()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .text_color(secondary)
                                    .cursor(CursorStyle::PointingHand)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.toggle(ix, cx);
                                    }))
                                    .child(if loading { "…" } else if expanded { "▼" } else { "▶" })
                            } else {
                                div().id("chevron").w(px(INDENT)).h_full().flex_none()
                            };
                            el = el.child(chevron);

                            el = el.child(
                                div()
                                    .w(px(22.))
                                    .flex_none()
                                    .flex()
                                    .justify_center()
                                    .child(crate::icons::render(style.icon, &theme, surface)),
                            );

                            let mut text = div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap();

                            for segment in segments(&node) {
                                let piece = match segment.style {
                                    SegmentStyle::Primary => div()
                                        .min_w_0()
                                        .flex_shrink(1.)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .when(matches!(node.kind.as_str(), "Project" | "Build"), |d| {
                                            d.font_weight(FontWeight::SEMIBOLD)
                                        })
                                        .child(segment.text),
                                    SegmentStyle::KindLabel => div()
                                        .flex_none()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if on_accent { theme.text_on_accent } else { style.color })
                                        .child(segment.text),
                                    SegmentStyle::Secondary => {
                                        div().flex_none().text_color(secondary).child(segment.text)
                                    }
                                    SegmentStyle::Chip => div()
                                        .flex_none()
                                        .px(px(6.))
                                        .rounded(px(4.))
                                        .text_size(px(11.))
                                        .bg(theme.chip_background)
                                        .text_color(theme.chip_text)
                                        .child(segment.text),
                                    SegmentStyle::Badge => div()
                                        .flex_none()
                                        .px(px(6.))
                                        .rounded(px(4.))
                                        .text_size(px(11.))
                                        .bg(theme.badge_background)
                                        .text_color(theme.badge_text)
                                        .child(segment.text),
                                    SegmentStyle::Targets => div()
                                        .flex_none()
                                        .text_color(if on_accent { theme.text_on_accent } else { gpui::rgb(0x9b59d0).into() })
                                        .child(segment.text),
                                    SegmentStyle::Duration => div()
                                        .flex_none()
                                        .font_family(crate::theme::MONO)
                                        .text_size(px(11.))
                                        .text_color(secondary)
                                        .child(segment.text),
                                };
                                text = text.child(piece);
                            }
                            el = el.child(text);

                            // The star is an affordance, not decoration: it
                            // shows on the row it acts on, and on any row
                            // already kept.
                            if favorite || selected {
                                let starred = node.clone();
                                el = el.child(
                                    div()
                                        .id("favorite")
                                        .w(px(18.))
                                        .flex_none()
                                        .flex()
                                        .justify_center()
                                        .text_size(px(11.))
                                        .text_color(if on_accent {
                                            theme.text_on_accent
                                        } else if favorite {
                                            theme.warning
                                        } else {
                                            theme.text_tertiary
                                        })
                                        .cursor(CursorStyle::PointingHand)
                                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.favorites.update(cx, |favorites, cx| {
                                                favorites.toggle(starred.clone(), cx)
                                            });
                                        }))
                                        .child(if favorite { "★" } else { "☆" }),
                                );
                            }

                            items.push(el.into_any_element());
                        }
                        items
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
    }
}

// ===================================================================
// Right-click menu
// ===================================================================

/// One entry: a label, and what it does. Built per node, so an entry only
/// exists when it applies — the WPF viewer hides the same ones through
/// per-kind visibility rules in `ContextMenu_Opened`.
enum MenuAction {
    ToggleFavorite,
    ViewSource,
    Preprocess,
    Copy(String),
    /// Fetch the node's subtree as text, then copy it.
    CopySubtree,
    /// Fetch the node's children, then copy one title per line.
    CopyChildren,
    AppendSearch(String),
    SetSearch(String),
    ShowInTimeline,
    Sort(i32),
}

impl TreeView {
    fn open_menu(&mut self, ix: usize, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(node) = self.model.row(ix).map(|r| r.node.clone()) else { return };
        self.select(ix, cx);
        self.menu = Some(ContextMenu { ix, node, position });
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    fn menu_entries(&self, node: &SharedNode, cx: &App) -> Vec<(String, Option<MenuAction>)> {
        let mut out: Vec<(String, Option<MenuAction>)> = Vec::new();
        let separator = |out: &mut Vec<(String, Option<MenuAction>)>| {
            if !out.is_empty() && out.last().map_or(false, |(_, a)| a.is_some()) {
                out.push((String::new(), None));
            }
        };

        let favorite = self.favorites.read(cx).contains(&node.id);
        out.push((
            if favorite { "Remove from Favorites".into() } else { "Add to Favorites".into() },
            Some(MenuAction::ToggleFavorite),
        ));
        if node.has_source {
            out.push(("View Source".into(), Some(MenuAction::ViewSource)));
        }
        if node.can_preprocess {
            out.push(("Preprocess".into(), Some(MenuAction::Preprocess)));
        }

        separator(&mut out);
        out.push(("Copy".into(), Some(MenuAction::Copy(node.title.clone()))));
        if let Some(name) = node.name.clone().filter(|n| !n.is_empty()) {
            out.push(("Copy name".into(), Some(MenuAction::Copy(name))));
        }
        if let Some(value) = node.value.clone().filter(|v| !v.is_empty()) {
            out.push(("Copy value".into(), Some(MenuAction::Copy(value))));
        }
        // The bridge reports a source path in props for the kinds that have
        // one; the tree row already knows it, so no round trip is needed.
        if let Some(path) = node.prop("projectFile").or_else(|| node.prop("file")).or_else(|| node.prop("importedProject")) {
            out.push(("Copy file path".into(), Some(MenuAction::Copy(path.to_string()))));
        }
        if node.has_children {
            out.push(("Copy subtree".into(), Some(MenuAction::CopySubtree)));
            out.push(("Copy children".into(), Some(MenuAction::CopyChildren)));
        }

        // `under($42)` scopes a search to a subtree by node index — the same
        // clause the WPF menu builds.
        separator(&mut out);
        let numeric = node.id.chars().all(|c| c.is_ascii_digit());
        if numeric && node.has_children {
            out.push(("Search in subtree".into(), Some(MenuAction::AppendSearch(format!("under(${})", node.id)))));
            out.push((
                "Exclude subtree from search".into(),
                Some(MenuAction::AppendSearch(format!("notunder(${})", node.id))),
            ));
        }
        // The by-name scope clause needs a kind the query parser knows;
        // `mslog_search_help` lists them, and Folder (say) is not one.
        if let Some(name) = node.name.clone().filter(|n| !n.is_empty()) {
            let clause = match node.kind.as_str() {
                "Project" => Some(format!("project({name})")),
                "ProjectEvaluation" => Some(format!("under($projectevaluation {name})")),
                "Target" | "EntryTarget" => Some(format!("under($target {name})")),
                "Task" => Some(format!("under($task {name})")),
                _ => None,
            };
            if let Some(clause) = clause {
                out.push((format!("Search in '{}'", ellipsize(&name, 28)), Some(MenuAction::AppendSearch(clause))));
            }
        }
        let searchable = node.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| node.title.clone());
        if !searchable.is_empty() {
            out.push((format!("Search '{}'", ellipsize(&searchable, 32)), Some(MenuAction::SetSearch(searchable))));
        }
        if node.duration_ms.is_some() {
            out.push(("Show in Timeline".into(), Some(MenuAction::ShowInTimeline)));
        }

        if node.has_children {
            separator(&mut out);
            out.push(("Sort children by name".into(), Some(MenuAction::Sort(1))));
            out.push(("Sort children by duration".into(), Some(MenuAction::Sort(2))));
            out.push(("Restore child order".into(), Some(MenuAction::Sort(0))));
        }
        out
    }

    fn invoke(&mut self, ix: usize, node: SharedNode, action: MenuAction, cx: &mut Context<Self>) {
        self.menu = None;
        match action {
            MenuAction::ToggleFavorite => {
                self.favorites.update(cx, |favorites, cx| favorites.toggle(node, cx));
            }
            MenuAction::ViewSource => cx.emit(TreeEvent::ViewSource(node)),
            MenuAction::Preprocess => cx.emit(TreeEvent::Preprocess(node)),
            MenuAction::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            MenuAction::AppendSearch(clause) => cx.emit(TreeEvent::AppendSearch(clause)),
            MenuAction::SetSearch(query) => cx.emit(TreeEvent::SetSearch(query)),
            MenuAction::ShowInTimeline => cx.emit(TreeEvent::ShowInTimeline(node)),
            MenuAction::CopySubtree => {
                let session = self.session.clone();
                let id = node.id.clone();
                cx.spawn(async move |this, cx| {
                    match session.subtree_text(&id).await {
                        Ok(text) => {
                            this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text))).ok();
                        }
                        Err(err) => eprintln!("subtree_text({id}): {err:#}"),
                    }
                })
                .detach();
            }
            MenuAction::CopyChildren => {
                let session = self.session.clone();
                let id = node.id.clone();
                let sort = self.sort.get(&id).copied().unwrap_or(0);
                cx.spawn(async move |this, cx| {
                    match session.all_children(&id, sort).await {
                        Ok(children) => {
                            let text =
                                children.iter().map(|c| c.title.as_str()).collect::<Vec<_>>().join("\n");
                            this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text))).ok();
                        }
                        Err(err) => eprintln!("children({id}): {err:#}"),
                    }
                })
                .detach();
            }
            MenuAction::Sort(mode) => {
                // Children are fetched sorted, so fold the row and let the
                // next expand ask the bridge again.
                if mode == 0 {
                    self.sort.remove(&node.id);
                } else {
                    self.sort.insert(node.id.clone(), mode);
                }
                if self.model.row(ix).map_or(false, |r| r.expanded) {
                    self.model.collapse(ix);
                    self.expand(ix, cx);
                }
            }
        }
        cx.notify();
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let menu = self.menu.as_ref()?;
        let entries = self.menu_entries(&menu.node, cx);
        let ix = menu.ix;
        let node = menu.node.clone();

        let mut list = div()
            .id("tree-context-menu")
            // The tree dismisses the menu on any mouse-down; without this the
            // menu would vanish on press and the click would never land.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .w(px(250.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(theme.window_background)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .text_size(px(12.));
        for (i, (label, action)) in entries.into_iter().enumerate() {
            if action.is_none() {
                list = list.child(div().h(px(1.)).w_full().my(px(3.)).bg(theme.border));
                continue;
            }
            let node = node.clone();
            list = list.child(
                div()
                    .id(ElementId::NamedInteger("menu".into(), i as u64))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .h(px(24.))
                    .rounded(px(5.))
                    .cursor(CursorStyle::PointingHand)
                    .text_color(theme.text)
                    .hover(|s| s.bg(theme.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        this.invoke_entry(ix, node.clone(), i, cx);
                    }))
                    .child(label),
            );
        }
        Some(
            deferred(anchored().position(menu.position).snap_to_window_with_margin(px(8.)).child(list))
                .into_any_element(),
        )
    }

    /// Menu entries are rebuilt on demand, so a click carries an index into
    /// that list rather than the action itself (which is not `Clone`).
    fn invoke_entry(&mut self, ix: usize, node: SharedNode, entry: usize, cx: &mut Context<Self>) {
        let mut entries = self.menu_entries(&node, cx);
        if entry >= entries.len() {
            return;
        }
        let Some(action) = entries.remove(entry).1 else { return };
        self.invoke(ix, node, action, cx);
    }
}

fn ellipsize(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
