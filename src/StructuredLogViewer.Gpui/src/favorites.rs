//! Favorites: the nodes the user pinned out of the build tree, and the
//! sidebar pane that lists them. The WPF viewer collects these from a
//! right-click menu; here the build tree's ⌘D (and the star on the selected
//! row) toggles membership.
//!
//! `Favorites` is a plain entity shared by the tree — which needs to know
//! what is starred while it renders — and the pane, which observes it.

use crate::model::SharedNode;
use crate::styling::style_for;
use crate::theme::Theme;
use gpui::{ClickEvent, Context, CursorStyle, Entity, EventEmitter, ScrollHandle, Window, div, prelude::*, px};

#[derive(Default)]
pub struct Favorites {
    nodes: Vec<SharedNode>,
}

impl Favorites {
    pub fn contains(&self, id: &str) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    pub fn nodes(&self) -> &[SharedNode] {
        &self.nodes
    }

    /// Returns whether the node is a favorite afterwards.
    pub fn toggle(&mut self, node: SharedNode, cx: &mut Context<Self>) -> bool {
        let added = match self.nodes.iter().position(|n| n.id == node.id) {
            Some(ix) => {
                self.nodes.remove(ix);
                false
            }
            None => {
                self.nodes.push(node);
                true
            }
        };
        cx.notify();
        added
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        self.nodes.retain(|n| n.id != id);
        cx.notify();
    }
}

pub enum FavoritesEvent {
    Reveal(String),
}

pub struct FavoritesView {
    favorites: Entity<Favorites>,
    scroll: ScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
}

impl EventEmitter<FavoritesEvent> for FavoritesView {}

impl FavoritesView {
    pub fn new(favorites: Entity<Favorites>, cx: &mut Context<Self>) -> Self {
        cx.observe(&favorites, |_, _, cx| cx.notify()).detach();
        FavoritesView { favorites, scroll: ScrollHandle::new(), scrollbars: crate::scrollbar::Scrollbars::new() }
    }
}

impl Render for FavoritesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let nodes: Vec<SharedNode> = self.favorites.read(cx).nodes().to_vec();

        let body: gpui::AnyElement = if nodes.is_empty() {
            div()
                .p(px(12.))
                .text_size(px(12.))
                .text_color(theme.text_tertiary)
                .child("Select a node in the build tree and press ⌘D — or click the star on the selected row — to keep it here.")
                .into_any_element()
        } else {
            div()
                .id("favorites")
                .size_full()
                .overflow_y_scroll()
                .relative()
                .track_scroll(&self.scroll)
                .child(self.scrollbars.render(&self.scroll, &theme))
                .children(nodes.into_iter().enumerate().map(|(ix, node)| {
                    let style = style_for(&node, &theme);
                    let reveal_id = node.id.clone();
                    let remove_id = node.id.clone();
                    div()
                        .id(ix)
                        .h(px(22.))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(10.))
                        .pr(px(6.))
                        .text_size(px(12.))
                        .text_color(theme.text)
                        .hover(|s| s.bg(theme.hover))
                        .cursor(CursorStyle::PointingHand)
                        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
                            cx.emit(FavoritesEvent::Reveal(reveal_id.clone()))
                        }))
                        .child(div().w(px(18.)).flex_none().flex().justify_center().child(crate::icons::render(style.icon, &theme, theme.sidebar_background)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(node.title.clone()),
                        )
                        .child(
                            div()
                                .id("remove")
                                .w(px(18.))
                                .flex_none()
                                .flex()
                                .justify_center()
                                .text_size(px(11.))
                                .text_color(theme.text_tertiary)
                                .hover(|s| s.text_color(theme.error))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                    cx.stop_propagation();
                                    let id = remove_id.clone();
                                    this.favorites.update(cx, |favorites, cx| favorites.remove(&id, cx));
                                }))
                                .child("✕"),
                        )
                }))
                .into_any_element()
        };

        div().flex().flex_col().size_full().bg(theme.sidebar_background).child(div().flex_1().min_h_0().child(body))
    }
}
