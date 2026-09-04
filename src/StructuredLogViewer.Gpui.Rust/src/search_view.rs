//! The two query panes of the sidebar, which differ only in what they ask
//! the bridge: **Search Log** (the whole build) and **Properties and items**
//! (one Project or ProjectEvaluation). Both are a query field, a debounced
//! cancel-previous search, and the grouped result tree with highlight spans.
//! Clicking a real result asks the workspace to reveal it in the build tree.

use crate::engine::Session;
use crate::model::{SearchResponse, SearchTreeNode, SharedNode};
use crate::styling::style_for;
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use gpui::{
    App, ClickEvent, Context, CursorStyle, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, HighlightStyle, StyledText, Task, UniformListScrollHandle, Window, div, prelude::*,
    px, uniform_list,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(300);
const MIN_QUERY: usize = 3;
const MAX_RESULTS: usize = 500;
const ROW_HEIGHT: f32 = 22.;

pub enum SearchEvent {
    Reveal(String),
}

/// What the pane searches. The result tree is identical either way.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// The whole build, through the log's search index.
    Log,
    /// Properties, items and assignments under one project or evaluation.
    PropertiesAndItems,
}

struct ResultRow {
    path: String,
    depth: usize,
    node: Arc<SearchTreeNode>,
    has_children: bool,
}

pub struct SearchView {
    session: Arc<Session>,
    mode: SearchMode,
    /// Properties and items only: the project or evaluation in scope.
    context: Option<SharedNode>,
    input: Entity<TextInput>,
    response: Option<SearchResponse>,
    rows: Vec<ResultRow>,
    collapsed: HashSet<String>,
    selected: Option<usize>,
    searching: bool,
    error: Option<String>,
    generation: u64,
    current_op: i64,
    pending: Option<Task<()>>,
    scroll: UniformListScrollHandle,
    watermark_scroll: gpui::ScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
}

impl EventEmitter<SearchEvent> for SearchView {}

impl Focusable for SearchView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl SearchView {
    pub fn new(session: Arc<Session>, mode: SearchMode, cx: &mut Context<Self>) -> Self {
        let placeholder = match mode {
            SearchMode::Log => "Search ($error, $task csc, …)",
            SearchMode::PropertiesAndItems => "Search properties and items",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        cx.subscribe(&input, |this, _input, event, cx| match event {
            InputEvent::Changed => this.schedule(true, cx),
            InputEvent::Submitted => this.schedule(false, cx),
        })
        .detach();
        SearchView {
            session,
            mode,
            context: None,
            input,
            response: None,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            selected: None,
            searching: false,
            error: None,
            generation: 0,
            current_op: 0,
            pending: None,
            scroll: UniformListScrollHandle::new(),
            watermark_scroll: gpui::ScrollHandle::new(),
            scrollbars: crate::scrollbar::Scrollbars::new(),
        }
    }

    pub fn run_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.to_string();
        self.input.update(cx, |input, cx| input.set_text(query, cx));
        self.schedule(false, cx);
    }

    /// Adds a clause to the current query, which is how the build tree's
    /// context menu narrows a search to a subtree.
    pub fn append_query(&mut self, clause: &str, cx: &mut Context<Self>) {
        let current = self.input.read(cx).text().trim().to_string();
        let combined =
            if current.is_empty() { clause.to_string() } else { format!("{current} {clause}") };
        self.run_query(&combined, cx);
    }

    /// The Project or ProjectEvaluation the Properties and items pane is
    /// scoped to; re-runs the current query when it moves, like the WPF
    /// viewer does as the tree selection walks between projects.
    pub fn set_context(&mut self, context: Option<SharedNode>, cx: &mut Context<Self>) {
        let same = match (&self.context, &context) {
            (Some(a), Some(b)) => a.id == b.id,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.context = context;
        self.schedule(false, cx);
        cx.notify();
    }

    pub fn context(&self) -> Option<&SharedNode> {
        self.context.as_ref()
    }

    fn is_executable(query: &str) -> bool {
        let t = query.trim();
        t.chars().count() >= MIN_QUERY || t.starts_with('$')
    }

    fn schedule(&mut self, debounced: bool, cx: &mut Context<Self>) {
        self.pending = None;
        self.session.cancel(self.current_op);
        self.current_op = 0;
        self.generation += 1;
        let generation = self.generation;

        let query = self.input.read(cx).text().trim().to_string();
        let context_id = self.context.as_ref().map(|c| c.id.clone());
        if (self.mode == SearchMode::PropertiesAndItems && context_id.is_none()) || !Self::is_executable(&query) {
            self.response = None;
            self.error = None;
            self.searching = false;
            self.rebuild_rows();
            cx.notify();
            return;
        }

        self.searching = true;
        self.error = None;
        cx.notify();

        let session = self.session.clone();
        let op = session.allocate_op();
        self.current_op = op;
        let mode = self.mode;
        self.pending = Some(cx.spawn(async move |this, cx| {
            if debounced {
                cx.background_executor().timer(DEBOUNCE).await;
            }
            let result = match (mode, &context_id) {
                (SearchMode::PropertiesAndItems, Some(id)) => {
                    session.search_properties_and_items(id, &query, MAX_RESULTS, op).await
                }
                _ => session.search(&query, MAX_RESULTS, op).await,
            };
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.searching = false;
                match result {
                    Ok(response) => {
                        this.response = Some(response);
                        this.collapsed.clear();
                        this.selected = None;
                        this.rebuild_rows();
                    }
                    Err(err) if err.to_string() == "cancelled" => {}
                    Err(err) => this.error = Some(format!("{err:#}")),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let Some(response) = &self.response else { return };
        fn walk(rows: &mut Vec<ResultRow>, collapsed: &HashSet<String>, node: &SearchTreeNode, path: String, depth: usize) {
            let has_children = node.children.as_ref().map_or(false, |c| !c.is_empty());
            rows.push(ResultRow { path: path.clone(), depth, node: Arc::new(node.clone()), has_children });
            if !has_children || collapsed.contains(&path) {
                return;
            }
            for (i, child) in node.children.as_ref().unwrap().iter().enumerate() {
                walk(rows, collapsed, child, format!("{path}/{i}"), depth + 1);
            }
        }
        for (i, root) in response.roots.iter().enumerate() {
            walk(&mut self.rows, &self.collapsed, root, i.to_string(), 0);
        }
    }

    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix) else { return };
        let path = row.path.clone();
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        let Some(row) = self.rows.get(ix) else { return };
        if let Some(node) = &row.node.node {
            cx.emit(SearchEvent::Reveal(node.id.clone()));
        } else if row.has_children {
            self.toggle(ix, cx);
        }
        cx.notify();
    }

    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = &self.rows[ix];
        let node = row.node.clone();
        let depth = row.depth;
        let has_children = row.has_children;
        let collapsed = self.collapsed.contains(&row.path);
        let selected = self.selected == Some(ix);

        let surface = if selected { theme.selection_inactive } else { theme.sidebar_background };
        let icon = match &node.node {
            Some(summary) => style_for(summary, theme).icon,
            // Grouping rows (a project, a target) have no node of their own.
            None => crate::icons::NodeIcon::Chip(crate::icons::Tone::Folder),
        };

        // One styled string per row so truncation works on the whole line.
        let mut text = String::new();
        let mut highlights = Vec::new();
        match &node.highlights {
            Some(spans) if !spans.is_empty() => {
                for span in spans {
                    let start = text.len();
                    text.push_str(&span.text);
                    let range = start..text.len();
                    if span.style.as_deref() == Some("time") {
                        highlights.push((range, HighlightStyle { color: Some(theme.text_secondary), ..Default::default() }));
                    } else if span.is_highlight {
                        highlights.push((
                            range,
                            HighlightStyle {
                                color: Some(theme.highlight_text),
                                background_color: Some(theme.highlight_background),
                                font_weight: Some(FontWeight::SEMIBOLD),
                                ..Default::default()
                            },
                        ));
                    }
                }
            }
            _ => text = node.node.as_ref().map(|n| n.title.clone()).or_else(|| node.text.clone()).unwrap_or_default(),
        }
        let text = text.replace('\n', " ");

        let mut el = div()
            .id(ix)
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .pl(px(8.))
            .pr(px(8.))
            .text_size(px(12.))
            .text_color(theme.text)
            .when(selected, |d| d.bg(theme.selection_inactive))
            .when(!selected, |d| d.hover(|s| s.bg(theme.hover)))
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| this.activate(ix, cx)));

        for _ in 0..depth {
            el = el.child(
                div().w(px(12.)).h_full().flex_none().flex().justify_center().child(div().w(px(1.)).h_full().bg(theme.guide)),
            );
        }

        let chevron = if has_children {
            div()
                .id("chevron")
                .w(px(14.))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .text_color(theme.text_secondary)
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    this.toggle(ix, cx);
                }))
                .child(if collapsed { "▶" } else { "▼" })
        } else {
            div().id("chevron").w(px(14.)).h_full().flex_none()
        };

        el.child(chevron)
            .child(div().w(px(20.)).flex_none().flex().justify_center().child(crate::icons::render(icon, theme, surface)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(StyledText::new(text).with_highlights(highlights)),
            )
            .into_any_element()
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let count = self.rows.len();

        let needs_context = self.mode == SearchMode::PropertiesAndItems && self.context.is_none();

        let body: gpui::AnyElement = if needs_context {
            div()
                .p(px(12.))
                .text_size(px(12.))
                .text_color(theme.text_tertiary)
                .child("A project or evaluation must be selected in the build tree.")
                .into_any_element()
        } else if let Some(error) = &self.error {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.error).child(error.clone()).into_any_element()
        } else if let Some(response) = &self.response {
            let status = format!(
                "{} result{}{} · {:.0} ms",
                response.result_count,
                if response.result_count == 1 { "" } else { "s" },
                if response.overflow { " (capped)" } else { "" },
                response.elapsed_ms
            );
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(12.))
                        .h(px(22.))
                        .text_size(px(11.))
                        .text_color(theme.text_secondary)
                        .child(status)
                        .when(self.searching, |d| d.child(div().ml_auto().child("updating…"))),
                )
                .child(
                    div().flex_1().min_h_0().relative().child(self.scrollbars.render(&self.scroll.0.borrow().base_handle, &theme)).child(
                        uniform_list(
                            "search-results",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                                range.filter(|ix| *ix < this.rows.len()).map(|ix| this.render_row(ix, &theme, cx)).collect()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    ),
                )
                .into_any_element()
        } else if self.searching {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.text_secondary).child("Searching…").into_any_element()
        } else {
            self.render_watermark(&theme, cx)
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.sidebar_background)
            .when(self.mode == SearchMode::PropertiesAndItems, |d| d.child(self.render_context_bar(&theme)))
            .child(div().p(px(8.)).child(self.input.clone()))
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl SearchView {
    /// The "Project: …" strip the WPF viewer puts above the Properties and
    /// items field, naming what the query is scoped to.
    fn render_context_bar(&self, theme: &Theme) -> gpui::AnyElement {
        let (label, color) = match &self.context {
            Some(node) => (
                node.prop("projectFile").map(crate::msbuild::file_name).unwrap_or_else(|| node.title.clone()),
                theme.text,
            ),
            None => ("no project selected".to_string(), theme.text_tertiary),
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .h(px(27.))
            .flex_none()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.titlebar_background)
            .text_size(px(11.))
            .child(div().flex_none().text_color(theme.text_secondary).child("Project:"))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(color)
                    .child(label),
            )
            .when_some(self.context.as_ref().and_then(|n| n.prop("targetFramework").map(str::to_owned)), |d, tf| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(theme.badge_background)
                        .text_color(theme.badge_text)
                        .child(tf),
                )
            })
            .into_any_element()
    }

    fn render_watermark(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        match self.mode {
            SearchMode::Log => self.render_log_watermark(theme, cx),
            SearchMode::PropertiesAndItems => self.render_properties_watermark(theme, cx),
        }
    }

    /// Same shape as the log watermark: the syntax in one line, then
    /// clickable examples.
    fn render_properties_watermark(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let examples: [(&str, &str); 6] = [
            ("$property TargetFramework", "one property"),
            ("$item Compile", "one item type"),
            ("$metadata HintPath", "item metadata"),
            ("name=Version", "search names only"),
            ("value=net10.0", "search values only"),
            ("\"OutputPath\"", "exact match, no substring"),
        ];
        self.render_examples(
            "Properties and items for the selected project or evaluation. Quote a term for an exact match;              prefix name= or value= to search one side only.",
            &examples,
            theme,
            cx,
        )
    }

    /// The WPF viewer's search watermark: the whole query language, with
    /// every fragment clickable. `mslog_search_help` is the authority on
    /// what the engine actually accepts, and everything advertised here is
    /// in it.
    fn render_log_watermark(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        const KINDS: [&str; 17] = [
            "$project",
            "$projectevaluation",
            "$target",
            "$task",
            "$error",
            "$warning",
            "$message",
            "$property",
            "$item",
            "$additem",
            "$removeitem",
            "$metadata",
            "$csc",
            "$rar",
            "$import",
            "$noimport",
            "$secret",
        ];
        const SCOPING: [(&str, &str); 4] = [
            ("under(FILTER)", "only nodes with a matching ancestor"),
            ("notunder(FILTER)", "the opposite of under()"),
            ("project(FILTER)", "filter by nearest parent project"),
            ("not(FILTER)", "exclude a subquery"),
        ];
        const MODIFIERS: [(&str, &str); 8] = [
            ("$target skipped=false", "drop skipped targets (true for only those)"),
            ("$task $time", "add durations and sort by them, longest first"),
            ("$project $start", "add start times and sort by them"),
            (r#"start>"2024-01-15 14:30""#, "started after a timestamp (quote it)"),
            ("name=Version value=1.2", "match a property or item field, not the whole row"),
            ("$copy System.Text.Json.dll", "the file-copy map: every copy touching a file"),
            ("$nuget project(App.csproj) Newtonsoft", "NuGet dependencies, packages and their files"),
            ("$projectreference project(App.csproj)", "what a project references, directly or not"),
        ];
        // The WPF viewer's own list, verbatim.
        const EXAMPLES: [(&str, &str); 10] = [
            ("Copying file from ", "every file copy message"),
            ("Resolved file path is ", "assembly resolution results"),
            ("There was a conflict", "assembly binding conflicts"),
            ("Building target completely ", "targets that could not skip"),
            ("is newer than output ", "why a target rebuilt"),
            ("Property reassignment: $(", "properties that were overwritten"),
            ("out-of-date", "incremental build misses"),
            ("$csc under($project Core)", "Csc anywhere under a project named Core"),
            ("$message CompilerServer failed", "compiler server fallbacks"),
            ("$secret not(username)", "detected secrets, minus one false positive"),
        ];

        let para = |text: &str| -> gpui::Div {
            div()
                .text_size(px(12.))
                .line_height(px(17.))
                .text_color(theme.text_secondary)
                .child(text.to_string())
        };
        let heading = |text: &str| -> gpui::Div {
            div()
                .pt(px(4.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_tertiary)
                .child(text.to_string())
        };

        div()
            .id("search-watermark")
            .size_full()
            .overflow_y_scroll()
            .relative()
            .track_scroll(&self.watermark_scroll)
            .child(self.scrollbars.render(&self.watermark_scroll, theme))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .px(px(10.))
                    .py(px(6.))
                    .pb(px(16.))
                    .child(para(
                        "Type in the box above to search; ⌘F focuses it. Words are ANDed, and match \
                         anywhere in a row. Quote a phrase for a literal match, or a single word for a \
                         whole-string one.",
                    ))
                    .child(heading("NODE KINDS"))
                    .child(para("Lead with a kind to narrow the search to it."))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .children(KINDS.iter().enumerate().map(|(i, kind)| {
                                self.query_chip(ElementId::NamedInteger("kind".into(), i as u64), kind, *theme, cx)
                            })),
                    )
                    .child(heading("SCOPE"))
                    .children(SCOPING.iter().enumerate().map(|(i, (query, hint))| {
                        self.query_row(ElementId::NamedInteger("scope".into(), i as u64), query, hint, false, *theme, cx)
                    }))
                    .child(heading("MODIFIERS"))
                    .children(MODIFIERS.iter().enumerate().map(|(i, (query, hint))| {
                        self.query_row(ElementId::NamedInteger("modifier".into(), i as u64), query, hint, true, *theme, cx)
                    }))
                    .child(heading("EXAMPLES"))
                    .children(EXAMPLES.iter().enumerate().map(|(i, (query, hint))| {
                        self.query_row(ElementId::NamedInteger("example".into(), i as u64), query, hint, true, *theme, cx)
                    })),
            )
            .into_any_element()
    }

    /// A kind selector, as a clickable pill.
    fn query_chip(&self, id: ElementId, query: &str, theme: Theme, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let run = query.to_string();
        div()
            .id(id)
            .px(px(6.))
            .h(px(19.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(theme.selection_inactive.opacity(0.6))
            .font_family(crate::theme::MONO)
            .text_size(px(11.))
            .text_color(theme.link)
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.bg(theme.hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                let run = run.clone();
                this.run_query(&run, cx);
            }))
            .child(query.to_string())
    }

    /// A query on one line with what it does underneath. `runnable` is false
    /// for the ones that are a shape rather than a query — clicking those
    /// would only ever return nothing.
    fn query_row(
        &self,
        id: ElementId,
        query: &str,
        hint: &str,
        runnable: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let run = query.to_string();
        div()
            .id(id)
            .flex()
            .flex_col()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(5.))
            .when(runnable, |d| {
                let run = run.clone();
                d.cursor(CursorStyle::PointingHand).hover(|s| s.bg(theme.hover)).on_click(cx.listener(
                    move |this, _: &ClickEvent, _window, cx| {
                        let run = run.clone();
                        this.run_query(&run, cx);
                    },
                ))
            })
            .child(
                div()
                    .font_family(crate::theme::MONO)
                    .text_size(px(11.))
                    .text_color(if runnable { theme.link } else { theme.text })
                    .child(query.to_string()),
            )
            .child(div().text_size(px(11.)).text_color(theme.text_secondary).child(hint.to_string()))
    }

    fn render_examples(
        &self,
        hint: &str,
        examples: &[(&str, &str)],
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
            .py(px(6.))
            .text_size(px(12.))
            .child(div().text_color(theme.text_secondary).pb(px(4.)).child(hint.to_string()))
            .children(examples.iter().copied().enumerate().map(|(i, (query, hint))| {
                let q = query.to_string();
                div()
                    .id(i)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(6.))
                    .h(px(22.))
                    .rounded(px(5.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        let q = q.clone();
                        this.run_query(&q, cx);
                    }))
                    .child(div().w(px(190.)).flex_none().overflow_hidden().whitespace_nowrap().text_ellipsis().font_family(crate::theme::MONO).text_color(theme.link).child(query.to_string()))
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(theme.text_secondary).child(hint.to_string()))
            }))
            .into_any_element()
    }
}
