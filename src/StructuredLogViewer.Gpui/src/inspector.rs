//! Inspector for the selected node: kind, timing, source, per-kind props
//! and the full text, fetched from the bridge when the selection changes.

use crate::engine::Session;
use crate::model::{NodeDetails, format_duration};
use crate::styling::{state_accent, style_for};
use crate::theme::Theme;
use gpui::{ClickEvent, Context, CursorStyle, EventEmitter, FontWeight, ScrollHandle, Window, div, prelude::*, px};
use std::sync::Arc;

pub enum InspectorEvent {
    OpenSource(String),
    OpenPreprocessed(String, String),
}

impl EventEmitter<InspectorEvent> for Inspector {}

pub struct Inspector {
    session: Arc<Session>,
    details: Option<NodeDetails>,
    generation: u64,
    scroll: ScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
}

impl Inspector {
    pub fn new(session: Arc<Session>) -> Self {
        Inspector {
            session,
            details: None,
            generation: 0,
            scroll: ScrollHandle::new(),
            scrollbars: crate::scrollbar::Scrollbars::new(),
        }
    }

    pub fn show(&mut self, node_id: String, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.node(&node_id).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(details) => this.details = Some(details),
                    Err(err) => eprintln!("node({node_id}): {err:#}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.details = None;
        cx.notify();
    }
}

impl Render for Inspector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let Some(details) = &self.details else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(20.))
                .bg(theme.sidebar_background)
                .text_size(px(12.))
                .text_color(theme.text_tertiary)
                .child("Select a node to inspect it.")
                .into_any_element();
        };

        let node = &details.node;
        let style = style_for(node, &theme);

        let fact = |label: &str, value: Option<String>| {
            value.filter(|v| !v.is_empty()).map(|v| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .child(div().w(px(110.)).flex_none().text_color(theme.text_secondary).child(label.to_string()))
                    .child(div().flex_1().min_w_0().font_family(crate::theme::MONO).text_color(theme.text).child(v))
            })
        };

        let mut facts = vec![
            fact("Node id", Some(node.id.clone())),
            fact("Duration", node.duration_ms.filter(|d| *d > 0.).map(format_duration)),
            fact("Start", details.start_time.clone()),
            fact("End", details.end_time.clone()),
            fact("Children", node.has_children.then(|| node.child_count.to_string())),
        ];
        let node_id = node.id.clone();
        let source_label = details.source_file.as_ref().map(|f| match details.source_line {
            Some(line) => format!("{f}:{line}"),
            None => f.clone(),
        });
        let actions = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .text_size(px(11.))
            .when_some(source_label.clone(), |d, label| {
                let id = node_id.clone();
                d.child(
                    div()
                        .id("open-source")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(8.))
                        .h(px(22.))
                        .rounded(px(5.))
                        .bg(theme.accent)
                        .text_color(theme.text_on_accent)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.opacity(0.9))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| cx.emit(InspectorEvent::OpenSource(id.clone()))))
                        .child("View Source")
                        .child(div().text_color(theme.text_on_accent.opacity(0.75)).max_w(px(240.)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(crate::msbuild::file_name(&label))),
                )
            })
            .when(node.can_preprocess, |d| {
                let id = node_id.clone();
                let title = node.prop("projectFile").map(crate::msbuild::file_name).or_else(|| node.name.clone()).unwrap_or_else(|| node.title.clone());
                d.child(
                    div()
                        .id("preprocess")
                        .px(px(8.))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .bg(theme.selection_inactive)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.opacity(0.85))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
                            cx.emit(InspectorEvent::OpenPreprocessed(id.clone(), title.clone()))
                        }))
                        .child("Preprocess"),
                )
            });
        facts.push(fact("Source", source_label));
        if let Some(props) = &node.props {
            let mut keys: Vec<_> = props.keys().collect();
            keys.sort();
            for key in keys {
                facts.push(fact(key, props.get(key).cloned()));
            }
        }

        let full_text = details.full_text.clone().unwrap_or_default();
        let full_text = if full_text.len() > 20_000 {
            let mut end = 20_000;
            while !full_text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\n… (truncated)", &full_text[..end])
        } else {
            full_text
        };

        div()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .overflow_x_hidden()
            .relative()
            .track_scroll(&self.scroll)
            .bg(theme.sidebar_background)
            .child(self.scrollbars.render(&self.scroll, &theme))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(crate::icons::render(style.icon, &theme, theme.sidebar_background))
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(4.))
                                    .text_size(px(11.))
                                    .bg(theme.badge_background)
                                    .text_color(theme.badge_text)
                                    .child(node.kind.clone()),
                            )
                            .when(node.state != "none" && !node.state.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(state_accent(node, &theme).unwrap_or(theme.success))
                                        .child(node.state.clone()),
                                )
                            }),
                    )
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child(node.title.clone()))
                    .child(actions)
                    .child(div().h(px(1.)).w_full().bg(theme.border))
                    .child(div().flex().flex_col().gap(px(4.)).children(facts.into_iter().flatten()))
                    .child(div().h(px(1.)).w_full().bg(theme.border))
                    .child(div().text_size(px(11.)).text_color(theme.text_secondary).child("Full text"))
                    .child(
                        div()
                            .p(px(8.))
                            .rounded(px(6.))
                            .bg(theme.content_background)
                            .border_1()
                            .border_color(theme.border)
                            .font_family(crate::theme::MONO)
                            .text_size(px(11.))
                            .text_color(theme.text)
                            .child(full_text),
                    ),
            )
            .into_any_element()
    }
}
