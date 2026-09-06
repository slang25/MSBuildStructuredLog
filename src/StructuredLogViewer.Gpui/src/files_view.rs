//! The two source-archive panes of the sidebar, shown only when the log
//! carries embedded files:
//!
//! - **Files** — every archived file as a folder tree, single-child folder
//!   chains collapsed the way the WPF viewer's `CompressTree` does, with a
//!   filter box over the full path.
//! - **Find in Files** — a debounced substring search across those files,
//!   grouped by file, each hit carrying its line number and match spans.
//!
//! Both open into the document well; the workspace turns the event into a
//! `SourceWell::open_file`.

use crate::engine::Session;
use crate::model::{FileEntry, FileSearchResponse};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use gpui::{
    App, ClickEvent, Context, CursorStyle, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, StyledText, Task, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const ROW_HEIGHT: f32 = 22.;
const INDENT: f32 = 14.;
const DEBOUNCE: Duration = Duration::from_millis(300);
const MIN_TERM: usize = 3;
const MAX_RESULTS: usize = 500;

/// Both panes ask for the same thing: show this file, at this line.
pub enum FilesEvent {
    Open { path: String, line: Option<usize> },
}

// ===================================================================
// Files: the archive as a tree
// ===================================================================

/// One node of the archive tree. Folders own children; files own an entry.
struct TreeNode {
    name: String,
    /// The archive path — of the file, or of the folder prefix.
    path: String,
    lines: usize,
    file: bool,
    children: Vec<TreeNode>,
}

/// Trie under construction: children in insertion order, indexed by name.
struct Builder {
    name: String,
    children: Vec<Builder>,
    index: std::collections::HashMap<String, usize>,
    entry: Option<FileEntry>,
}

impl Builder {
    fn new(name: String) -> Builder {
        Builder { name, children: Vec::new(), index: std::collections::HashMap::new(), entry: None }
    }

    fn child(&mut self, name: &str) -> &mut Builder {
        let ix = match self.index.get(name) {
            Some(ix) => *ix,
            None => {
                self.children.push(Builder::new(name.to_string()));
                let ix = self.children.len() - 1;
                self.index.insert(name.to_string(), ix);
                ix
            }
        };
        &mut self.children[ix]
    }
}

/// Windows archive paths keep backslashes; POSIX ones keep slashes. The
/// first path with a drive letter or no leading slash decides which one
/// compressed folder names are joined with.
fn separator(files: &[FileEntry]) -> char {
    match files.first() {
        Some(f) if f.path.contains(':') || (!f.path.starts_with('\\') && !f.path.starts_with('/')) => '\\',
        _ => '/',
    }
}

fn build_tree(files: &[FileEntry]) -> Vec<TreeNode> {
    let sep = separator(files);
    let mut root = Builder::new(String::new());
    for entry in files {
        let mut node = &mut root;
        let parts: Vec<&str> = entry.path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        let Some((last, folders)) = parts.split_last() else { continue };
        for part in folders {
            node = node.child(part);
        }
        node = node.child(last);
        node.entry = Some(entry.clone());
    }
    root.children.into_iter().map(|b| finish(b, String::new(), sep)).collect()
}

/// Turns a builder into a `TreeNode`, folding a folder that has exactly one
/// sub-folder and nothing else into `parent/child`.
fn finish(mut builder: Builder, prefix: String, sep: char) -> TreeNode {
    let mut name = builder.name;
    while builder.entry.is_none() && builder.children.len() == 1 && builder.children[0].entry.is_none() {
        let only = builder.children.remove(0);
        name = format!("{name}{sep}{}", only.name);
        builder.children = only.children;
    }
    let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}{sep}{name}") };
    let mut children: Vec<TreeNode> =
        builder.children.into_iter().map(|b| finish(b, path.clone(), sep)).collect();
    // Folders first, then files, each alphabetically — the AppKit and
    // Explorer convention, and what the WPF tree ends up showing.
    children.sort_by(|a, b| {
        a.file.cmp(&b.file).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    match builder.entry {
        Some(entry) => TreeNode { name, path: entry.path, lines: entry.lines, file: true, children },
        None => TreeNode { name, path, lines: 0, file: false, children },
    }
}

struct FileRow {
    depth: usize,
    name: String,
    path: String,
    lines: usize,
    file: bool,
    expanded: bool,
}

pub struct FilesView {
    session: Arc<Session>,
    filter: Entity<TextInput>,
    tree: Vec<TreeNode>,
    rows: Vec<FileRow>,
    total: usize,
    /// Folder paths the user opened. Ignored while a filter is active.
    expanded: HashSet<String>,
    selected: Option<usize>,
    loading: bool,
    error: Option<String>,
    scroll: UniformListScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
}

impl EventEmitter<FilesEvent> for FilesView {}

impl Focusable for FilesView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.filter.read(cx).focus_handle(cx)
    }
}

impl FilesView {
    pub fn new(session: Arc<Session>, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextInput::new("Filter files", cx));
        cx.subscribe(&filter, |this, _input, event, cx| match event {
            InputEvent::Changed | InputEvent::Submitted => {
                this.rebuild_rows(cx);
                cx.notify();
            }
        })
        .detach();

        let mut this = FilesView {
            session,
            filter,
            tree: Vec::new(),
            rows: Vec::new(),
            total: 0,
            expanded: HashSet::new(),
            selected: None,
            loading: true,
            error: None,
            scroll: UniformListScrollHandle::new(),
            scrollbars: crate::scrollbar::Scrollbars::new(),
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = session.files_list().await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(list) => {
                        this.total = list.total.max(list.files.len());
                        this.tree = build_tree(&list.files);
                        // One level open is enough orientation without
                        // unfolding thousands of rows.
                        for node in &this.tree {
                            if !node.file {
                                this.expanded.insert(node.path.clone());
                            }
                        }
                        this.rebuild_rows(cx);
                    }
                    Err(err) => this.error = Some(format!("{err:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter.read(cx).text().trim().to_lowercase();
        let mut rows = Vec::new();
        for node in &self.tree {
            emit(node, 0, &filter, &self.expanded, &mut rows);
        }
        self.rows = rows;
        self.selected = None;
    }

    fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        let Some(row) = self.rows.get(ix) else { return };
        if row.file {
            cx.emit(FilesEvent::Open { path: row.path.clone(), line: None });
        } else {
            let path = row.path.clone();
            if !self.expanded.remove(&path) {
                self.expanded.insert(path);
            }
            self.rebuild_rows(cx);
        }
        cx.notify();
    }
}

/// Appends `node`'s visible rows, returning whether anything survived.
/// Without a filter the tree shows as the user folded it; with one, only
/// files whose path matches survive and every folder above one is opened.
fn emit(node: &TreeNode, depth: usize, filter: &str, expanded: &HashSet<String>, rows: &mut Vec<FileRow>) -> bool {
    if node.file {
        if !filter.is_empty() && !node.path.to_lowercase().contains(filter) {
            return false;
        }
        rows.push(FileRow {
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            lines: node.lines,
            file: true,
            expanded: false,
        });
        return true;
    }

    let open = filter.is_empty().then(|| expanded.contains(&node.path)).unwrap_or(true);
    let at = rows.len();
    rows.push(FileRow {
        depth,
        name: node.name.clone(),
        path: node.path.clone(),
        lines: 0,
        file: false,
        expanded: open,
    });
    if !open {
        // Folded: its contents are not on screen, but the folder is.
        return true;
    }
    let mut any = false;
    for child in &node.children {
        any |= emit(child, depth + 1, filter, expanded, rows);
    }
    if !any && !filter.is_empty() {
        rows.truncate(at);
    }
    any || filter.is_empty()
}

impl Render for FilesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let count = self.rows.len();

        let body: gpui::AnyElement = if let Some(error) = &self.error {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.error).child(error.clone()).into_any_element()
        } else if self.loading {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.text_secondary).child("Reading the source archive…").into_any_element()
        } else if count == 0 {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.text_tertiary).child("No files match.").into_any_element()
        } else {
            div().size_full().relative().child(self.scrollbars.render(&self.scroll.0.borrow().base_handle, &theme)).child(uniform_list(
                "files-rows",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    range.filter(|ix| *ix < this.rows.len()).map(|ix| this.render_row(ix, &theme, cx)).collect()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full())
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.sidebar_background)
            .child(div().p(px(8.)).child(self.filter.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(12.))
                    .h(px(22.))
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme.text_secondary)
                    .child(format!("{} file{}", self.total, if self.total == 1 { "" } else { "s" })),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl FilesView {
    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = &self.rows[ix];
        let selected = self.selected == Some(ix);

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

        for _ in 0..row.depth {
            el = el.child(
                div().w(px(INDENT)).h_full().flex_none().flex().justify_center().child(div().w(px(1.)).h_full().bg(theme.guide)),
            );
        }

        el.child(
            div()
                .w(px(14.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .text_color(theme.text_secondary)
                .child(if row.file {
                    ""
                } else if row.expanded {
                    "▼"
                } else {
                    "▶"
                }),
        )
        .child(
            div()
                .w(px(16.))
                .flex_none()
                .flex()
                .justify_center()
                .text_size(px(11.))
                .text_color(if row.file { theme.link } else { theme.warning })
                .child(if row.file { "▭" } else { "▪" }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .when(!row.file, |d| d.font_weight(FontWeight::MEDIUM))
                .child(row.name.clone()),
        )
        .when(row.file && row.lines > 0, |d| {
            d.child(
                div()
                    .flex_none()
                    .pl(px(6.))
                    .font_family(crate::theme::MONO)
                    .text_size(px(10.))
                    .text_color(theme.text_tertiary)
                    .child(row.lines.to_string()),
            )
        })
        .into_any_element()
    }
}

// ===================================================================
// Find in Files
// ===================================================================

/// A file header, or one matching line under it.
enum FindRow {
    File { path: String, matches: usize, collapsed: bool },
    Line { path: String, line: usize, text: String, spans: Vec<(usize, usize)> },
}

pub struct FindInFilesView {
    session: Arc<Session>,
    input: Entity<TextInput>,
    response: Option<FileSearchResponse>,
    rows: Vec<FindRow>,
    collapsed: HashSet<String>,
    selected: Option<usize>,
    searching: bool,
    error: Option<String>,
    generation: u64,
    current_op: i64,
    pending: Option<Task<()>>,
    scroll: UniformListScrollHandle,
    scrollbars: crate::scrollbar::Scrollbars,
}

impl EventEmitter<FilesEvent> for FindInFilesView {}

impl Focusable for FindInFilesView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl FindInFilesView {
    pub fn new(session: Arc<Session>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Search in files", cx));
        cx.subscribe(&input, |this, _input, event, cx| match event {
            InputEvent::Changed => this.schedule(true, cx),
            InputEvent::Submitted => this.schedule(false, cx),
        })
        .detach();
        FindInFilesView {
            session,
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
            scrollbars: crate::scrollbar::Scrollbars::new(),
        }
    }

    pub fn run_query(&mut self, term: &str, cx: &mut Context<Self>) {
        let term = term.to_string();
        self.input.update(cx, |input, cx| input.set_text(term, cx));
        self.schedule(false, cx);
    }

    fn schedule(&mut self, debounced: bool, cx: &mut Context<Self>) {
        self.pending = None;
        self.session.cancel(self.current_op);
        self.current_op = 0;
        self.generation += 1;
        let generation = self.generation;

        let term = self.input.read(cx).text().trim().to_string();
        if term.chars().count() < MIN_TERM {
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
        self.pending = Some(cx.spawn(async move |this, cx| {
            if debounced {
                cx.background_executor().timer(DEBOUNCE).await;
            }
            let result = session.files_search(&term, MAX_RESULTS, op).await;
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
        for file in &response.files {
            let collapsed = self.collapsed.contains(&file.path);
            self.rows.push(FindRow::File {
                path: file.path.clone(),
                matches: file.matches.len(),
                collapsed,
            });
            if collapsed {
                continue;
            }
            for hit in &file.matches {
                let spans = hit
                    .spans
                    .iter()
                    .filter_map(|s| {
                        let start = utf16_to_byte(&hit.text, s.start)?;
                        let end = utf16_to_byte(&hit.text, s.start + s.length)?;
                        Some((start, end))
                    })
                    .collect();
                self.rows.push(FindRow::Line {
                    path: file.path.clone(),
                    line: hit.line,
                    text: hit.text.clone(),
                    spans,
                });
            }
        }
    }

    fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        match self.rows.get(ix) {
            Some(FindRow::File { path, .. }) => {
                let path = path.clone();
                if !self.collapsed.remove(&path) {
                    self.collapsed.insert(path);
                }
                self.rebuild_rows();
            }
            Some(FindRow::Line { path, line, .. }) => {
                cx.emit(FilesEvent::Open { path: path.clone(), line: Some(*line) });
            }
            None => {}
        }
        cx.notify();
    }
}

/// C# reports match offsets in UTF-16 code units; Rust needs byte offsets.
fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    if offset == 0 {
        return Some(0);
    }
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
    }
    (units == offset).then(|| text.len())
}

impl Render for FindInFilesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let count = self.rows.len();

        let body: gpui::AnyElement = if let Some(error) = &self.error {
            div().p(px(12.)).text_size(px(12.)).text_color(theme.error).child(error.clone()).into_any_element()
        } else if let Some(response) = &self.response {
            let status = format!(
                "{} match{} in {} file{}{}",
                response.total_matches,
                if response.total_matches == 1 { "" } else { "es" },
                response.files.len(),
                if response.files.len() == 1 { "" } else { "s" },
                if response.overflow { " (capped)" } else { "" },
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
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme.text_secondary)
                        .child(status)
                        .when(self.searching, |d| d.child(div().ml_auto().child("updating…"))),
                )
                .child(
                    div().flex_1().min_h_0().relative().child(self.scrollbars.render(&self.scroll.0.borrow().base_handle, &theme)).child(
                        uniform_list(
                            "find-in-files-rows",
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
            div()
                .p(px(12.))
                .text_size(px(12.))
                .text_color(theme.text_secondary)
                .child("Type 3+ characters to search the text of every file embedded in the log.")
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.sidebar_background)
            .child(div().p(px(8.)).child(self.input.clone()))
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl FindInFilesView {
    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected = self.selected == Some(ix);
        let base = div()
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

        match &self.rows[ix] {
            FindRow::File { path, matches, collapsed } => base
                .child(
                    div()
                        .w(px(14.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .text_color(theme.text_secondary)
                        .child(if *collapsed { "▶" } else { "▼" }),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(crate::msbuild::file_name(path)),
                )
                .child(
                    div()
                        .flex_none()
                        .pl(px(6.))
                        .text_size(px(11.))
                        .text_color(theme.text_tertiary)
                        .child(matches.to_string()),
                )
                .into_any_element(),
            FindRow::Line { line, text, spans, .. } => {
                let trimmed = text.trim_start();
                let shift = text.len() - trimmed.len();
                let highlights = spans
                    .iter()
                    .filter(|(start, _)| *start >= shift)
                    .map(|(start, end)| {
                        (
                            (start - shift)..(end - shift),
                            HighlightStyle {
                                color: Some(theme.highlight_text),
                                background_color: Some(theme.highlight_background),
                                font_weight: Some(FontWeight::SEMIBOLD),
                                ..Default::default()
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                base.child(div().w(px(14.)).flex_none())
                    .child(
                        div()
                            .w(px(44.))
                            .flex_none()
                            .pr(px(8.))
                            .font_family(crate::theme::MONO)
                            .text_size(px(10.))
                            .text_color(theme.text_tertiary)
                            .child(line.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_family(crate::theme::MONO)
                            .text_size(px(11.))
                            .child(StyledText::new(trimmed.to_string()).with_highlights(highlights)),
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> FileEntry {
        FileEntry { path: path.to_string(), lines: 1, length: 1 }
    }

    fn row_paths(rows: &[FileRow]) -> Vec<(usize, String)> {
        rows.iter().map(|r| (r.depth, r.name.clone())).collect()
    }

    #[test]
    fn compresses_single_child_folders_and_sorts_folders_first() {
        let files = [
            entry("/a/b/c/one.props"),
            entry("/a/b/c/two.targets"),
            entry("/a/b/loose.txt"),
        ];
        let tree = build_tree(&files);
        // /a/b is one chain to a single node; under it, folder c precedes
        // the file that sits beside it.
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a/b");
        let names: Vec<_> = tree[0].children.iter().map(|c| (c.name.as_str(), c.file)).collect();
        assert_eq!(names, vec![("c", false), ("loose.txt", true)]);
    }

    #[test]
    fn folded_folders_hide_their_contents() {
        let files = [entry("/a/b/one.props")];
        let tree = build_tree(&files);
        let mut rows = Vec::new();
        emit(&tree[0], 0, "", &HashSet::new(), &mut rows);
        assert_eq!(row_paths(&rows), vec![(0, "a/b".to_string())]);

        let expanded: HashSet<String> = ["a/b".to_string()].into_iter().collect();
        let mut rows = Vec::new();
        emit(&tree[0], 0, "", &expanded, &mut rows);
        assert_eq!(row_paths(&rows), vec![(0, "a/b".to_string()), (1, "one.props".to_string())]);
    }

    #[test]
    fn a_filter_opens_folders_and_drops_the_rest() {
        let files = [entry("/a/keep/one.props"), entry("/a/drop/two.targets")];
        let tree = build_tree(&files);
        let mut rows = Vec::new();
        // No folder is expanded, yet the filter still reaches the match.
        emit(&tree[0], 0, "one", &HashSet::new(), &mut rows);
        assert_eq!(
            row_paths(&rows),
            vec![(0, "a".to_string()), (1, "keep".to_string()), (2, "one.props".to_string())]
        );
    }

    #[test]
    fn a_filter_that_matches_nothing_leaves_no_rows() {
        let files = [entry("/a/keep/one.props")];
        let tree = build_tree(&files);
        let mut rows = Vec::new();
        assert!(!emit(&tree[0], 0, "nothing", &HashSet::new(), &mut rows));
        assert!(rows.is_empty());
    }

    #[test]
    fn utf16_offsets_become_byte_offsets() {
        // "é" is one UTF-16 unit and two bytes; an emoji is two units.
        assert_eq!(utf16_to_byte("héllo", 2), Some(3));
        assert_eq!(utf16_to_byte("🙂x", 2), Some(4));
        assert_eq!(utf16_to_byte("abc", 3), Some(3));
        assert_eq!(utf16_to_byte("abc", 9), None);
    }
}
