//! The bridge's JSON payloads (see `StructuredLogViewer.NativeBridge/Dto.cs`
//! and `include/mslog.h`). camelCase on the wire, nulls omitted.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub has_children: bool,
    #[serde(default)]
    pub child_count: usize,
    #[serde(default)]
    pub is_low_relevance: bool,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub has_source: bool,
    #[serde(default)]
    pub can_preprocess: bool,
    #[serde(default)]
    pub child_index: Option<usize>,
    #[serde(default)]
    pub props: Option<HashMap<String, String>>,
}

impl NodeSummary {
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .as_ref()
            .and_then(|p| p.get(key))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetails {
    pub node: NodeSummary,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub full_text: Option<String>,
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub source_line: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenPage {
    pub parent_id: String,
    pub total: usize,
    pub offset: usize,
    pub count: usize,
    #[serde(default)]
    pub children: Vec<NodeSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ancestors {
    /// Root first, the requested node itself last.
    pub chain: Vec<NodeSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub text: String,
    #[serde(default)]
    pub is_highlight: bool,
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTreeNode {
    #[serde(default)]
    pub node: Option<NodeSummary>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub highlights: Option<Vec<Highlight>>,
    #[serde(default)]
    pub children: Option<Vec<SearchTreeNode>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub result_count: usize,
    #[serde(default)]
    pub overflow: bool,
    pub elapsed_ms: f64,
    #[serde(default)]
    pub roots: Vec<SearchTreeNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub root_id: String,
    pub succeeded: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub node_count: usize,
    #[serde(default)]
    pub has_source_archive: bool,
    #[serde(default, rename = "msBuildVersion")]
    pub msbuild_version: Option<String>,
    pub file_path: String,
    pub file_size: u64,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    pub file_path: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub code: String,
    pub message: String,
}

pub type SharedNode = Arc<NodeSummary>;

pub fn format_duration(ms: f64) -> String {
    if ms >= 60_000.0 {
        let minutes = (ms / 60_000.0).floor();
        let seconds = (ms - minutes * 60_000.0) / 1000.0;
        format!("{}:{:06.3}", minutes as u64, seconds)
    } else if ms >= 1000.0 {
        format!("{:.3} s", ms / 1000.0)
    } else {
        format!("{:.0} ms", ms)
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

// ----- semantics (Cmd-click / quick info over MSBuild source) -----

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticContext {
    pub evaluation_id: String,
    #[serde(default)]
    pub project_file: Option<String>,
    pub label: String,
    #[serde(default)]
    pub is_project_file: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticImport {
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub column: usize,
    pub imported_path: String,
    #[serde(default = "default_true")]
    pub available: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSkippedImport {
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub column: usize,
    #[serde(default)]
    pub file_spec: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub evaluated_condition: Option<String>,
}

impl SemanticSkippedImport {
    pub fn has_condition(&self) -> bool {
        self.condition.is_some() && self.evaluated_condition.is_some()
    }

    /// Full explanation for quick info and errors.
    pub fn explanation(&self) -> String {
        match (&self.condition, &self.evaluated_condition) {
            (Some(c), Some(e)) => format!("Condition {c} evaluated as {e} → false"),
            _ => self.reason.clone().unwrap_or_else(|| "Not imported.".into()),
        }
    }

    /// Terse end-of-element note: the condition is already on screen, only
    /// what it expanded to is news.
    pub fn annotation(&self) -> String {
        match &self.evaluated_condition {
            Some(e) => format!("{e} → false"),
            None => self.reason.clone().unwrap_or_else(|| "Not imported.".into()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTargetDefinition {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFile {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub evaluation_id: Option<String>,
    #[serde(default)]
    pub contexts: Vec<SemanticContext>,
    #[serde(default)]
    pub contexts_total: usize,
    #[serde(default)]
    pub imports: Vec<SemanticImport>,
    #[serde(default)]
    pub skipped_imports: Vec<SemanticSkippedImport>,
    #[serde(default)]
    pub targets: Vec<SemanticTargetDefinition>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLocation {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub line: Option<usize>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default = "default_true")]
    pub available: bool,
}

impl SemanticLocation {
    pub fn is_reachable(&self) -> bool {
        self.available || self.node_id.is_some()
    }

    pub fn title(&self) -> String {
        self.label.clone().or_else(|| self.path.clone()).unwrap_or_default()
    }

    /// The secondary line, when it says something the label doesn't.
    pub fn secondary(&self) -> Option<String> {
        [self.path.as_ref(), self.detail.as_ref()]
            .into_iter()
            .flatten()
            .find(|c| !c.is_empty() && Some(*c) != self.label.as_ref())
            .cloned()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFact {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSymbol {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub found: bool,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub definitions: Vec<SemanticLocation>,
    #[serde(default)]
    pub executions: Vec<SemanticLocation>,
    #[serde(default)]
    pub facts: Vec<SemanticFact>,
}

// ----- timeline -----

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBlock {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub indent: usize,
    #[serde(default)]
    pub has_error: bool,
}

impl TimelineBlock {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineLane {
    pub node_id: i64,
    #[serde(default)]
    pub max_indent: usize,
    #[serde(default)]
    pub blocks: Vec<TimelineBlock>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    #[serde(default)]
    pub start_time: Option<String>,
    pub duration_ms: f64,
    #[serde(default)]
    pub lanes: Vec<TimelineLane>,
}

// ----- embedded source files (Files / Find in Files) -----

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    #[serde(default)]
    pub lines: usize,
    #[serde(default)]
    pub length: usize,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileList {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSpan {
    #[serde(default)]
    pub start: usize,
    #[serde(default)]
    pub length: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMatch {
    /// 1-based.
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub spans: Vec<MatchSpan>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMatches {
    pub path: String,
    #[serde(default)]
    pub matches: Vec<FileMatch>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchResponse {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub total_matches: usize,
    #[serde(default)]
    pub overflow: bool,
    #[serde(default)]
    pub files: Vec<FileMatches>,
}
