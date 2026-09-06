//! MSBuild XML, lexically: a syntax highlighter and a navigable-token
//! scanner, ported from the Mac viewer's `XMLHighlighter` and
//! `MSBuildTokenizer`. Single linear scans over bytes, no XML parse, so
//! malformed and preprocessed content survives. Offsets are UTF-8 bytes;
//! every delimiter matched is ASCII, so slices land on char boundaries.

use crate::model::{SemanticFile, SemanticImport, SemanticLocation, SemanticSkippedImport};
use std::collections::HashMap;
use std::ops::Range;

// ----- highlighting -----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum HighlightKind {
    Text = 0,
    Punctuation,
    ElementName,
    AttributeName,
    AttributeValue,
    Comment,
    Entity,
    Expression,
}

impl HighlightKind {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Punctuation,
            2 => Self::ElementName,
            3 => Self::AttributeName,
            4 => Self::AttributeValue,
            5 => Self::Comment,
            6 => Self::Entity,
            7 => Self::Expression,
            _ => Self::Text,
        }
    }
}

/// One kind per byte: cheap to build, trivial to slice per line, and a
/// later paint (an expression inside a string) simply overwrites.
pub struct Highlights {
    kinds: Vec<u8>,
}

impl Highlights {
    pub fn scan(text: &str) -> Highlights {
        let b = text.as_bytes();
        let n = b.len();
        let mut kinds = vec![HighlightKind::Text as u8; n];
        let mut paint = |kind: HighlightKind, start: usize, end: usize| {
            for k in kinds.iter_mut().take(end.min(n)).skip(start) {
                *k = kind as u8;
            }
        };

        let mut i = 0;
        while i < n {
            let c = b[i];
            if c == b'<' {
                if is_comment_start(b, i) {
                    let end = end_of_comment(b, i);
                    paint(HighlightKind::Comment, i, end);
                    i = end;
                    continue;
                }
                if i + 1 < n && (b[i + 1] == b'!' || b[i + 1] == b'?') {
                    let end = (index_of(b, b'>', i) + 1).min(n);
                    paint(HighlightKind::Punctuation, i, end);
                    i = end;
                    continue;
                }
                i = scan_tag_highlight(b, i, &mut paint);
                continue;
            }
            if c == b'&' {
                if let Some(end) = end_of_entity(b, i) {
                    paint(HighlightKind::Entity, i, end);
                    i = end;
                    continue;
                }
            }
            if let Some(end) = end_of_expression(b, i, n) {
                paint(HighlightKind::Expression, i, end);
                i = end;
                continue;
            }
            i += 1;
        }
        Highlights { kinds }
    }

    /// Runs of one kind within `range`, as ranges relative to `range.start`.
    pub fn runs(&self, range: Range<usize>) -> Vec<(Range<usize>, HighlightKind)> {
        let mut out = Vec::new();
        let slice = &self.kinds[range.start.min(self.kinds.len())..range.end.min(self.kinds.len())];
        let mut start = 0;
        while start < slice.len() {
            let kind = slice[start];
            let mut end = start + 1;
            while end < slice.len() && slice[end] == kind {
                end += 1;
            }
            out.push((start..end, HighlightKind::from_u8(kind)));
            start = end;
        }
        out
    }
}

fn scan_tag_highlight(b: &[u8], start: usize, paint: &mut impl FnMut(HighlightKind, usize, usize)) -> usize {
    let n = b.len();
    let mut i = start + 1;
    if i < n && b[i] == b'/' {
        i += 1;
    }
    paint(HighlightKind::Punctuation, start, i);

    let name_start = i;
    while i < n && is_name_char(b[i]) {
        i += 1;
    }
    paint(HighlightKind::ElementName, name_start, i);

    while i < n && b[i] != b'>' {
        if is_ws(b[i]) {
            i += 1;
            continue;
        }
        if b[i] == b'/' {
            paint(HighlightKind::Punctuation, i, i + 1);
            i += 1;
            continue;
        }
        let attr_start = i;
        while i < n && is_name_char(b[i]) {
            i += 1;
        }
        if i == attr_start {
            i += 1;
            continue;
        }
        paint(HighlightKind::AttributeName, attr_start, i);
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        if i >= n || b[i] != b'=' {
            continue;
        }
        paint(HighlightKind::Punctuation, i, i + 1);
        i += 1;
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        if i >= n || (b[i] != b'"' && b[i] != b'\'') {
            continue;
        }
        let delimiter = b[i];
        paint(HighlightKind::Punctuation, i, i + 1);
        i += 1;
        let value_start = i;
        while i < n && b[i] != delimiter && b[i] != b'<' {
            i += 1;
        }
        if i > value_start {
            paint(HighlightKind::AttributeValue, value_start, i);
            let mut j = value_start;
            while j < i {
                if let Some(end) = end_of_expression(b, j, i) {
                    paint(HighlightKind::Expression, j, end);
                    j = end;
                    continue;
                }
                j += 1;
            }
        }
        if i < n && b[i] == delimiter {
            paint(HighlightKind::Punctuation, i, i + 1);
            i += 1;
        }
    }
    if i < n {
        paint(HighlightKind::Punctuation, i, i + 1);
        return i + 1;
    }
    n
}

// ----- navigable tokens -----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// `$(Name)` — including the identifier part of `$(Name.Method())`.
    Property,
    /// `@(Name)`.
    Item,
    /// The `Name` attribute value of a `<Target>` element.
    TargetDefinition,
    /// One entry of DependsOnTargets / BeforeTargets / AfterTargets / …
    TargetReference,
    /// The `Project` attribute value of an `<Import>` element.
    ImportPath,
    /// An `Sdk="..."` attribute: its imports are logged with no line.
    SdkReference,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub name: String,
    pub range: Range<usize>,
    /// 1-based.
    pub line: usize,
}

impl Token {
    /// The token as written, e.g. `$(OutputPath)` or `Target Build`.
    pub fn title(&self) -> String {
        match self.kind {
            TokenKind::Property => format!("$({})", self.name),
            TokenKind::Item => format!("@({})", self.name),
            TokenKind::TargetDefinition | TokenKind::TargetReference => format!("Target {}", self.name),
            TokenKind::ImportPath | TokenKind::SdkReference => self.name.clone(),
        }
    }

    /// The engine's symbol kind, or None for tokens that resolve locally
    /// from the recorded import edges.
    pub fn symbol_kind(&self) -> Option<&'static str> {
        match self.kind {
            TokenKind::Property => Some("property"),
            TokenKind::Item => Some("item"),
            TokenKind::TargetDefinition | TokenKind::TargetReference => Some("target"),
            TokenKind::ImportPath | TokenKind::SdkReference => None,
        }
    }
}

const TARGET_LIST_ATTRIBUTES: [&str; 6] =
    ["dependsontargets", "beforetargets", "aftertargets", "initialtargets", "defaulttargets", "targets"];

pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

pub fn line_at(offset: usize, line_starts: &[usize]) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i + 1,
        Err(i) => i,
    }
}

pub fn tokenize(text: &str) -> Vec<Token> {
    let b = text.as_bytes();
    let starts = line_starts(text);
    let mut tokens = Vec::new();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i] == b'<' {
            if is_comment_start(b, i) {
                i = end_of_comment(b, i);
                continue;
            }
            if i + 1 < n && (b[i + 1] == b'!' || b[i + 1] == b'?') {
                i = index_of(b, b'>', i) + 1;
                continue;
            }
            i = scan_tag_tokens(text, i, &starts, &mut tokens);
            continue;
        }
        if let Some(t) = expression_token(text, i, n, &starts) {
            i = t.range.end;
            tokens.push(t);
            continue;
        }
        i += 1;
    }
    tokens.sort_by_key(|t| t.range.start);
    tokens
}

fn scan_tag_tokens(text: &str, start: usize, starts: &[usize], tokens: &mut Vec<Token>) -> usize {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = start + 1;
    if i < n && b[i] == b'/' {
        i += 1;
    }
    let name_start = i;
    while i < n && is_name_char(b[i]) {
        i += 1;
    }
    let element = text[name_start..i].to_ascii_lowercase();

    while i < n && b[i] != b'>' {
        if is_ws(b[i]) || b[i] == b'/' {
            i += 1;
            continue;
        }
        let attr_start = i;
        while i < n && is_name_char(b[i]) {
            i += 1;
        }
        if i == attr_start {
            i += 1;
            continue;
        }
        let attribute = text[attr_start..i].to_ascii_lowercase();
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        if i >= n || b[i] != b'=' {
            continue;
        }
        i += 1;
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        if i >= n || (b[i] != b'"' && b[i] != b'\'') {
            continue;
        }
        let delimiter = b[i];
        i += 1;
        let value_start = i;
        while i < n && b[i] != delimiter && b[i] != b'<' {
            i += 1;
        }
        let value_end = i;
        if i < n && b[i] == delimiter {
            i += 1;
        }
        scan_attribute_value(text, &element, &attribute, value_start, value_end, starts, tokens);
    }
    if i < n { i + 1 } else { n }
}

fn scan_attribute_value(
    text: &str,
    element: &str,
    attribute: &str,
    start: usize,
    end: usize,
    starts: &[usize],
    tokens: &mut Vec<Token>,
) {
    if end <= start {
        return;
    }
    let mut local: Vec<Token> = Vec::new();
    let mut i = start;
    while i < end {
        if let Some(t) = expression_token(text, i, end, starts) {
            i = t.range.end;
            local.push(t);
            continue;
        }
        i += 1;
    }
    let has_expression = !local.is_empty();

    if element == "target" && attribute == "name" && !has_expression {
        local.push(make_token(text, TokenKind::TargetDefinition, start, end, starts));
    } else if TARGET_LIST_ATTRIBUTES.contains(&attribute) {
        append_target_references(text, start, end, starts, &mut local);
    } else if let Some(kind) = import_kind(element, attribute) {
        // The whole attribute is one link; the recorded edge already names
        // the file any `$(...)` inside it expanded to.
        local.clear();
        local.push(make_token(text, kind, start, end, starts));
    }
    local.sort_by_key(|t| t.range.start);
    tokens.extend(local);
}

fn import_kind(element: &str, attribute: &str) -> Option<TokenKind> {
    match (element, attribute) {
        ("import", "project") => Some(TokenKind::ImportPath),
        ("import", "sdk") | ("project", "sdk") | ("sdk", "name") => Some(TokenKind::SdkReference),
        _ => None,
    }
}

fn append_target_references(text: &str, start: usize, end: usize, starts: &[usize], local: &mut Vec<Token>) {
    let b = text.as_bytes();
    let expressions: Vec<Range<usize>> = local.iter().map(|t| t.range.clone()).collect();
    let mut segment_start = start;
    let mut i = start;
    while i <= end {
        if i == end || b[i] == b';' {
            let mut from = segment_start;
            let mut to = i;
            while from < to && is_ws(b[from]) {
                from += 1;
            }
            while to > from && is_ws(b[to - 1]) {
                to -= 1;
            }
            let overlaps = expressions.iter().any(|e| e.start < to && e.end > from);
            if to > from && !overlaps {
                local.push(make_token(text, TokenKind::TargetReference, from, to, starts));
            }
            segment_start = i + 1;
        }
        i += 1;
    }
}

/// `$(Name` / `@(Name` at `offset`, as a token over just the name.
fn expression_token(text: &str, offset: usize, limit: usize, starts: &[usize]) -> Option<Token> {
    let b = text.as_bytes();
    if offset + 2 >= limit {
        return None;
    }
    let prefix = b[offset];
    if prefix != b'$' && prefix != b'@' {
        return None;
    }
    if b[offset + 1] != b'(' {
        return None;
    }
    let mut i = offset + 2;
    while i < limit && is_identifier_char(b[i]) {
        i += 1;
    }
    if i == offset + 2 {
        return None;
    }
    let kind = if prefix == b'$' { TokenKind::Property } else { TokenKind::Item };
    Some(make_token(text, kind, offset + 2, i, starts))
}

fn make_token(text: &str, kind: TokenKind, start: usize, end: usize, starts: &[usize]) -> Token {
    Token { kind, name: text[start..end].to_string(), range: start..end, line: line_at(start, starts) }
}

// ----- shared byte helpers -----

fn is_comment_start(b: &[u8], i: usize) -> bool {
    i + 3 < b.len() && b[i + 1] == b'!' && b[i + 2] == b'-' && b[i + 3] == b'-'
}

fn end_of_comment(b: &[u8], start: usize) -> usize {
    let n = b.len();
    let mut i = start + 4;
    while i + 2 < n {
        if b[i] == b'-' && b[i + 1] == b'-' && b[i + 2] == b'>' {
            return i + 3;
        }
        i += 1;
    }
    n
}

fn end_of_entity(b: &[u8], start: usize) -> Option<usize> {
    let n = b.len();
    let mut i = start + 1;
    while i < n && i - start < 12 {
        if b[i] == b';' {
            return Some(i + 1);
        }
        if !is_name_char(b[i]) && b[i] != b'#' {
            return None;
        }
        i += 1;
    }
    None
}

/// `$(…)`, `@(…)` or `%(…)` starting at `start`, paren-balanced; bails at a
/// line break or `<` so an unterminated expression can't swallow the file.
fn end_of_expression(b: &[u8], start: usize, limit: usize) -> Option<usize> {
    if start + 2 >= limit {
        return None;
    }
    let prefix = b[start];
    if prefix != b'$' && prefix != b'@' && prefix != b'%' {
        return None;
    }
    if b[start + 1] != b'(' {
        return None;
    }
    let mut depth = 1;
    let mut i = start + 2;
    while i < limit && i - start < 500 {
        let c = b[i];
        if c == b'\n' || c == b'<' {
            return None;
        }
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn index_of(b: &[u8], c: u8, start: usize) -> usize {
    let mut i = start;
    while i < b.len() && b[i] != c {
        i += 1;
    }
    i.min(b.len())
}

fn is_identifier_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b':'
}

fn is_ws(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
}

// ----- semantic index: tokens joined to the build's import edges -----

/// What a Cmd-click on a token should do.
pub enum Navigation {
    Open(SemanticLocation),
    Choose(Vec<SemanticLocation>),
    None(String),
}

pub struct SemanticIndex {
    pub tokens: Vec<Token>,
    pub file: SemanticFile,
    imports_by_line: HashMap<usize, Vec<SemanticImport>>,
    implicit_imports: Vec<SemanticImport>,
    skipped_by_line: HashMap<usize, Vec<SemanticSkippedImport>>,
}

impl SemanticIndex {
    pub fn new(text: &str, file: SemanticFile) -> SemanticIndex {
        let mut imports_by_line: HashMap<usize, Vec<SemanticImport>> = HashMap::new();
        let mut implicit_imports = Vec::new();
        for edge in &file.imports {
            if edge.line > 0 {
                imports_by_line.entry(edge.line).or_default().push(edge.clone());
            } else {
                implicit_imports.push(edge.clone());
            }
        }
        let mut skipped_by_line: HashMap<usize, Vec<SemanticSkippedImport>> = HashMap::new();
        for edge in &file.skipped_imports {
            if edge.line > 0 {
                skipped_by_line.entry(edge.line).or_default().push(edge.clone());
            }
        }
        let tokens = tokenize(text)
            .into_iter()
            .filter(|t| match t.kind {
                TokenKind::ImportPath => {
                    imports_by_line.contains_key(&t.line) || skipped_by_line.contains_key(&t.line)
                }
                TokenKind::SdkReference => {
                    imports_by_line.contains_key(&t.line)
                        || skipped_by_line.contains_key(&t.line)
                        || !implicit_imports.is_empty()
                }
                _ => true,
            })
            .collect();
        SemanticIndex { tokens, file, imports_by_line, implicit_imports, skipped_by_line }
    }

    pub fn is_navigable(&self, token: &Token) -> bool {
        match token.kind {
            TokenKind::ImportPath => self.imports_by_line.contains_key(&token.line),
            TokenKind::SdkReference => {
                self.imports_by_line.contains_key(&token.line) || !self.implicit_imports.is_empty()
            }
            _ => true,
        }
    }

    pub fn skipped_imports_for(&self, token: &Token) -> Vec<SemanticSkippedImport> {
        if !matches!(token.kind, TokenKind::ImportPath | TokenKind::SdkReference) {
            return Vec::new();
        }
        self.skipped_by_line.get(&token.line).cloned().unwrap_or_default()
    }

    /// Every skipped import, in source order, for the editor's inlays.
    pub fn skipped_imports(&self) -> Vec<SemanticSkippedImport> {
        let mut all: Vec<_> = self.file.skipped_imports.iter().filter(|s| s.line > 0).cloned().collect();
        all.sort_by_key(|s| (s.line, s.column));
        all
    }

    pub fn token_at(&self, offset: usize) -> Option<&Token> {
        let ix = self.tokens.partition_point(|t| t.range.end <= offset);
        self.tokens.get(ix).filter(|t| t.range.contains(&offset))
    }

    /// Import destinations, resolved locally from the recorded edges.
    pub fn import_navigation(&self, token: &Token) -> Navigation {
        let mut edges = self.imports_by_line.get(&token.line).cloned().unwrap_or_default();
        if edges.is_empty() && token.kind == TokenKind::SdkReference {
            edges = self.implicit_imports.clone();
        }
        let locations: Vec<SemanticLocation> = edges
            .iter()
            .map(|edge| SemanticLocation {
                path: Some(edge.imported_path.clone()),
                line: Some(1),
                label: Some(file_name(&edge.imported_path)),
                detail: Some(edge.imported_path.clone()),
                node_id: None,
                available: edge.available,
            })
            .collect();
        let reachable: Vec<_> = locations.iter().filter(|l| l.available).cloned().collect();
        if reachable.is_empty() {
            if let Some(skipped) = self.skipped_by_line.get(&token.line).and_then(|s| s.first()) {
                return Navigation::None(skipped.explanation());
            }
            return Navigation::None(match locations.first() {
                None => "This import was not recorded in the build.".into(),
                Some(l) => format!("'{}' is not embedded in this binlog.", l.detail.clone().unwrap_or_default()),
            });
        }
        if reachable.len() == 1 {
            Navigation::Open(reachable.into_iter().next().unwrap())
        } else {
            Navigation::Choose(reachable)
        }
    }
}

/// Which half of a resolved symbol a Cmd-click should follow.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Definitions,
    /// For `<Target Name="X">`: where it ran, not the line under the pointer.
    Executions,
}

pub fn symbol_navigation(symbol: &crate::model::SemanticSymbol, preference: Preference) -> Navigation {
    if !symbol.found {
        return Navigation::None(format!("No {} '{}' in this evaluation.", symbol.kind, symbol.name));
    }
    let definitions: Vec<_> = symbol.definitions.iter().filter(|d| d.is_reachable()).cloned().collect();
    let executions = symbol.executions.clone();
    let ordered = if preference == Preference::Executions {
        [executions, definitions]
    } else {
        [definitions, executions]
    };
    for candidates in ordered {
        if !candidates.is_empty() {
            return if candidates.len() == 1 {
                Navigation::Open(candidates.into_iter().next().unwrap())
            } else {
                Navigation::Choose(candidates)
            };
        }
    }
    if preference == Preference::Executions && symbol.executions.is_empty() {
        return Navigation::None(format!("'{}' never ran in this evaluation.", symbol.name));
    }
    Navigation::None(
        symbol
            .note
            .clone()
            .unwrap_or_else(|| format!("'{}' has no recorded definition in this evaluation.", symbol.name)),
    )
}

pub fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

// ----- inlays: end-of-element notes for skipped imports -----

/// Notes keyed by 0-based line, anchored to the element's closing `>`
/// (several records can share one element: `Project="a;b"` skips twice).
pub fn import_annotations(text: &str, skipped: &[SemanticSkippedImport]) -> HashMap<usize, String> {
    let mut by_line: HashMap<usize, Vec<String>> = HashMap::new();
    let starts = line_starts(text);
    let b = text.as_bytes();
    for record in skipped.iter().filter(|r| r.line > 0) {
        let Some(start) = offset_for(record.line, record.column, &starts, b) else { continue };
        let Some(anchor) = end_of_tag(b, start) else { continue };
        let line = line_at(anchor.saturating_sub(1), &starts).saturating_sub(1);
        let notes = by_line.entry(line).or_default();
        let note = record.annotation();
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    by_line.into_iter().map(|(line, notes)| (line, notes.join(" · "))).collect()
}

fn offset_for(line: usize, column: usize, starts: &[usize], b: &[u8]) -> Option<usize> {
    if line < 1 || line > starts.len() {
        return None;
    }
    let start = starts[line - 1];
    if column <= 1 {
        return Some(start);
    }
    let line_end = if line < starts.len() { starts[line] } else { b.len() };
    let candidate = start + column - 1;
    Some(if candidate < line_end { candidate } else { start })
}

fn end_of_tag(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < b.len() && b[i] != b'<' {
        if b[i] == b'\n' {
            return None;
        }
        i += 1;
    }
    if i >= b.len() {
        return None;
    }
    let mut delimiter: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match delimiter {
            Some(open) => {
                if c == open {
                    delimiter = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    delimiter = Some(c);
                } else if c == b'>' {
                    return Some(i + 1);
                }
            }
        }
        i += 1;
    }
    None
}

/// Whether a file is worth running the tokenizer over.
pub fn is_msbuild_file(path: &str, content: &str) -> bool {
    const EXTENSIONS: [&str; 12] = [
        ".csproj", ".vbproj", ".fsproj", ".vcxproj", ".esproj", ".shproj", ".props", ".targets", ".proj", ".tasks",
        ".overridetasks", ".pubxml",
    ];
    let lower = path.to_ascii_lowercase();
    EXTENSIONS.iter().any(|e| lower.ends_with(e)) || content.trim_start().starts_with("<Project")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutDir Condition="'$(OutDir)' == ''">$(BaseOutputPath)bin\</OutDir>
  </PropertyGroup>
  <Import Project="$(CustomProps)" Condition="'$(CustomProps)' != ''" />
  <Target Name="Build" DependsOnTargets="Restore;$(ExtraTargets);Compile" />
</Project>
"#;

    #[test]
    fn tokenizes_properties_imports_and_targets() {
        let tokens = tokenize(SAMPLE);
        let names: Vec<(TokenKind, &str, usize)> =
            tokens.iter().map(|t| (t.kind, t.name.as_str(), t.line)).collect();
        assert!(names.contains(&(TokenKind::SdkReference, "Microsoft.NET.Sdk", 1)));
        assert!(names.contains(&(TokenKind::Property, "OutDir", 3)));
        assert!(names.contains(&(TokenKind::Property, "BaseOutputPath", 3)));
        // The whole Project attribute is one import link; the $(CustomProps)
        // inside it is dropped rather than overlapping.
        assert!(names.contains(&(TokenKind::ImportPath, "$(CustomProps)", 5)));
        assert!(!names.iter().any(|(k, n, l)| *k == TokenKind::Property && *n == "CustomProps" && *l == 5 && false));
        assert!(names.contains(&(TokenKind::TargetDefinition, "Build", 6)));
        assert!(names.contains(&(TokenKind::TargetReference, "Restore", 6)));
        assert!(names.contains(&(TokenKind::TargetReference, "Compile", 6)));
        assert!(names.contains(&(TokenKind::Property, "ExtraTargets", 6)));
        let starts: Vec<usize> = tokens.iter().map(|t| t.range.start).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "tokens sorted by offset");
    }

    #[test]
    fn highlights_expressions_inside_attribute_values() {
        let h = Highlights::scan(SAMPLE);
        let line3 = line_starts(SAMPLE)[2];
        let text = SAMPLE.lines().nth(2).unwrap();
        let runs = h.runs(line3..line3 + text.len());
        let kind_at = |needle: &str| {
            let off = text.find(needle).unwrap();
            runs.iter().find(|(r, _)| r.contains(&off)).map(|(_, k)| *k).unwrap()
        };
        assert_eq!(kind_at("OutDir"), HighlightKind::ElementName);
        assert_eq!(kind_at("Condition"), HighlightKind::AttributeName);
        assert_eq!(kind_at("$(OutDir)"), HighlightKind::Expression);
        assert_eq!(kind_at("== ''"), HighlightKind::AttributeValue);
    }

    #[test]
    fn anchors_skipped_import_notes_to_the_element_line() {
        let skipped = vec![SemanticSkippedImport {
            line: 5,
            column: 3,
            file_spec: Some("$(CustomProps)".into()),
            reason: None,
            condition: Some("'$(CustomProps)' != ''".into()),
            evaluated_condition: Some("'' != ''".into()),
        }];
        let notes = import_annotations(SAMPLE, &skipped);
        assert_eq!(notes.get(&4).map(String::as_str), Some("'' != '' → false"));
    }

    #[test]
    fn index_joins_tokens_to_recorded_edges() {
        let file = SemanticFile {
            imports: vec![SemanticImport { line: 0, column: 0, imported_path: "/sdk/Sdk.props".into(), available: true }],
            skipped_imports: vec![SemanticSkippedImport {
                line: 5,
                column: 3,
                file_spec: None,
                reason: Some("Not imported due to false condition".into()),
                condition: None,
                evaluated_condition: None,
            }],
            ..Default::default()
        };
        let index = SemanticIndex::new(SAMPLE, file);
        let sdk = index.tokens.iter().find(|t| t.kind == TokenKind::SdkReference).unwrap();
        assert!(index.is_navigable(sdk), "implicit SDK imports make the Sdk attribute navigable");
        assert!(matches!(index.import_navigation(sdk), Navigation::Open(_)));
        let import = index.tokens.iter().find(|t| t.kind == TokenKind::ImportPath).unwrap();
        assert!(!index.is_navigable(import), "a skipped import has nowhere to go");
        assert!(matches!(index.import_navigation(import), Navigation::None(_)));
        let off = SAMPLE.find("BaseOutputPath").unwrap();
        assert_eq!(index.token_at(off).map(|t| t.name.as_str()), Some("BaseOutputPath"));
    }
}
