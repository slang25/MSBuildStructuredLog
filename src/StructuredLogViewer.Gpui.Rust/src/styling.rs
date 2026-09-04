//! Per-kind row appearance and row text composition, following the WPF
//! viewer's `themes/Generic.xaml` (which the Mac viewer's `NodeStyling` /
//! `NodeRowText` also follow). `icons.rs` draws the shapes and owns the
//! palette; this decides which shape and which tone each kind gets.

use crate::icons::{CopyEnd, NodeIcon, Tone};
use crate::model::{NodeSummary, format_duration};
use crate::theme::Theme;
use gpui::{Hsla, rgb};

pub struct NodeStyle {
    pub icon: NodeIcon,
    /// The kind's accent, used for the leading type label on a row.
    pub color: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn style_for(node: &NodeSummary, theme: &Theme) -> NodeStyle {
    let style = |icon: NodeIcon, tone: Tone| NodeStyle { icon, color: tone.accent(theme) };
    let chip = |tone: Tone| style(NodeIcon::Chip(tone), tone);
    // WPF paints a failed node's icon with the error brush whatever its kind.
    let failed = node.state == "failed";
    match node.kind.as_str() {
        "Build" => chip(if failed { Tone::Error } else { Tone::Success }),
        "Project" | "ProjectEvaluation" => {
            let tint = project_tint(node, theme);
            NodeStyle {
                icon: NodeIcon::Document { tint, evaluation: node.kind == "ProjectEvaluation" },
                color: tint,
            }
        }
        "Target" | "EntryTarget" => chip(if failed { Tone::Error } else { Tone::Target }),
        "Task" => chip(if failed { Tone::Error } else { Tone::Task }),
        "AddItem" | "TaskParameterItem" => style(NodeIcon::Plus(Tone::Item), Tone::Item),
        "RemoveItem" => style(NodeIcon::Minus(Tone::Item), Tone::Item),
        "Item" => chip(Tone::Item),
        "Metadata" => chip(Tone::Metadata),
        "Property" | "TaskParameterProperty" | "Parameter" => chip(Tone::Property),
        "Folder" | "TimedNode" | "EvaluationProfileEntry" => chip(Tone::Folder),
        "Error" => chip(Tone::Error),
        "Warning" | "CriticalBuildMessage" => chip(Tone::Warning),
        "Import" => chip(Tone::Import),
        "NoImport" => chip(Tone::NoImport),
        "Package" => style(NodeIcon::Package, Tone::NuGet),
        "FileCopy" => {
            let end = match node.prop("copyKind") {
                Some("Source") => CopyEnd::Source,
                Some("Destination") => CopyEnd::Destination,
                _ => CopyEnd::Both,
            };
            style(NodeIcon::Copy(end, Tone::Target), Tone::Target)
        }
        "MSBuildServerNode" => chip(Tone::Server),
        // Message, TimedMessage, Note, SourceFile, SourceFileLine and
        // anything the bridge grows later.
        _ => chip(Tone::Message),
    }
}

pub fn state_accent(node: &NodeSummary, theme: &Theme) -> Option<Hsla> {
    match node.state.as_str() {
        "failed" => Some(theme.error),
        "skipped" => Some(theme.text_tertiary),
        _ => None,
    }
}

/// The tint Visual Studio gives each project type, lightened for a dark
/// row: `#1F801F` and `#00539C` disappear against near-black.
fn project_tint(node: &NodeSummary, theme: &Theme) -> Hsla {
    let (light, dark) = match node.prop("extension").map(|e| e.to_ascii_lowercase()).as_deref() {
        Some(".csproj") => (0x1f801f, 0x5cc05c),
        Some(".vbproj") => (0x00539c, 0x5a9be0),
        Some(".fsproj") => (0x682878, 0xb37bc4),
        Some(".vcxproj") | Some(".cppproj") => (0xa43fb1, 0xcb84d6),
        Some(".sln") | Some(".slnx") | Some(".slnf") => (0x672079, 0xb37bc4),
        Some(".esproj") => return theme.warning,
        _ => (0x5a5a5f, 0xa0a0a8),
    };
    c(if theme.dark { dark } else { light })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegmentStyle {
    /// Type name a row leads with, tinted like its glyph.
    KindLabel,
    /// The row's own text.
    Primary,
    /// Supporting detail, e.g. " at (49;3)".
    Secondary,
    /// A short fact on a highlight fill (NoImport reason).
    Chip,
    /// Project target framework(s) on a tinted fill.
    Badge,
    /// Initial targets (`→ Rebuild`), tinted like a target.
    Targets,
    /// Elapsed time, monospaced digits.
    Duration,
}

pub struct Segment {
    pub text: String,
    pub style: SegmentStyle,
}

pub fn segments(node: &NodeSummary) -> Vec<Segment> {
    let mut out = Vec::with_capacity(4);
    let seg = |text: &str, style| Segment { text: text.to_string(), style };
    match node.kind.as_str() {
        "Import" => {
            out.push(seg("Import", SegmentStyle::KindLabel));
            out.push(seg(&node.title, SegmentStyle::Primary));
            push_location(node, &mut out);
        }
        "NoImport" => {
            out.push(seg("NoImport", SegmentStyle::KindLabel));
            out.push(seg(&node.title, SegmentStyle::Primary));
            push_location(node, &mut out);
            if let Some(reason) = node.prop("reason") {
                out.push(seg(reason, SegmentStyle::Chip));
            }
        }
        "Project" | "ProjectEvaluation" => match node.name.as_deref().filter(|n| !n.is_empty()) {
            None => out.push(seg(&node.title, SegmentStyle::Primary)),
            Some(name) => {
                out.push(seg(name, SegmentStyle::Primary));
                if let Some(a) = node.prop("adornment") {
                    out.push(seg(a, SegmentStyle::Badge));
                }
                if let Some(t) = node.prop("targetsText") {
                    out.push(seg(t, SegmentStyle::Targets));
                }
                if let Some(e) = node.prop("evaluationText") {
                    out.push(seg(e, SegmentStyle::Secondary));
                }
            }
        },
        _ => out.push(seg(&node.title, SegmentStyle::Primary)),
    }
    if let Some(ms) = node.duration_ms.filter(|ms| *ms > 0.0) {
        out.push(Segment { text: format_duration(ms), style: SegmentStyle::Duration });
    }
    out
}

fn push_location(node: &NodeSummary, out: &mut Vec<Segment>) {
    if let (Some(line), Some(col)) = (node.prop("line"), node.prop("column")) {
        out.push(Segment { text: format!("at ({line};{col})"), style: SegmentStyle::Secondary });
    }
}
