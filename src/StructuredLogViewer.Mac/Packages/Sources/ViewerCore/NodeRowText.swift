import Foundation

/// One styled run of a tree row. The tree renders these as an attributed
/// string; keeping the composition here makes it testable without AppKit.
public struct NodeRowSegment: Equatable, Sendable {
    public enum Style: Equatable, Sendable {
        /// The type name a row leads with, tinted like the row's icon
        /// ("Import", "NoImport"). Matches the viewer's node templates.
        case kindLabel

        /// The row's own text.
        case primary

        /// Supporting detail that shouldn't compete, e.g. " at (49;3)".
        case secondary

        /// A short fact worth a highlight fill — the reason an import was
        /// skipped, which is the whole point of a NoImport row.
        case chip

        /// A project's target framework(s), on its own tinted fill. Reads as
        /// a label on the row rather than part of the project's name, which
        /// is what makes a column of projects scannable by TFM.
        case badge

        /// The initial targets a project was built with (`→ Rebuild`),
        /// tinted like a target row.
        case targets

        /// Elapsed time, in the tabular-figures font.
        case duration
    }

    public var text: String
    public var style: Style

    public init(text: String, style: Style) {
        self.text = text
        self.style = style
    }
}

/// Composes a tree row's runs from a node summary, mirroring the WPF and
/// Avalonia viewers' per-type templates (`themes/Generic.xaml`,
/// `App.xaml`). Only the types that need more than their title appear here;
/// everything else is one primary run.
public enum NodeRowText {
    public static func segments(for summary: NodeSummary) -> [NodeRowSegment] {
        var segments: [NodeRowSegment] = []

        switch summary.nodeKind {
        case .importNode:
            segments.append(NodeRowSegment(text: "Import", style: .kindLabel))
            segments.append(NodeRowSegment(text: summary.title, style: .primary))
            appendLocation(of: summary, to: &segments)

        case .noImport:
            segments.append(NodeRowSegment(text: "NoImport", style: .kindLabel))
            segments.append(NodeRowSegment(text: summary.title, style: .primary))
            appendLocation(of: summary, to: &segments)
            if let reason = summary.props?["reason"], !reason.isEmpty {
                segments.append(NodeRowSegment(text: reason, style: .chip))
            }

        case .project, .projectEvaluation:
            appendProject(summary, to: &segments)

        default:
            segments.append(NodeRowSegment(text: summary.title, style: .primary))
        }

        if let duration = summary.durationMs, duration > 0 {
            segments.append(NodeRowSegment(
                text: NodeRowText.formatDuration(milliseconds: duration),
                style: .duration))
        }

        return segments
    }

    /// A project row is `name`, its target framework badge, and the initial
    /// targets it was asked to build. The engine also glues these into
    /// `title` for copy and tooltips; here they are three runs so each can
    /// be styled. `evaluationText` is deliberately absent — it is a
    /// navigable link, and the row renders it as one.
    private static func appendProject(_ summary: NodeSummary, to segments: inout [NodeRowSegment]) {
        // Falls back to the glued title when the engine sent no name, so a
        // row is never blank.
        guard let name = summary.name, !name.isEmpty else {
            segments.append(NodeRowSegment(text: summary.title, style: .primary))
            return
        }

        segments.append(NodeRowSegment(text: name, style: .primary))

        if let adornment = summary.props?["adornment"], !adornment.isEmpty {
            segments.append(NodeRowSegment(text: adornment, style: .badge))
        }

        if let targets = summary.props?["targetsText"], !targets.isEmpty {
            segments.append(NodeRowSegment(text: targets, style: .targets))
        }

        // The evaluation id is normally the row's trailing link. When the
        // engine couldn't resolve a destination there is nothing to click,
        // so show it as plain text rather than losing it.
        if let evaluationText = summary.props?["evaluationText"],
           !evaluationText.isEmpty,
           summary.props?["evaluationNodeId"] == nil {
            segments.append(NodeRowSegment(text: evaluationText, style: .secondary))
        }
    }

    /// `at (49;3)` — the viewers' `Location` format, semicolon and all.
    /// Emitted whenever the build reported a position, including (0;0) for
    /// the implicit SDK imports that have no element to point at.
    private static func appendLocation(of summary: NodeSummary, to segments: inout [NodeRowSegment]) {
        guard let line = summary.props?["line"], let column = summary.props?["column"] else { return }
        segments.append(NodeRowSegment(text: "at (\(line);\(column))", style: .secondary))
    }

    public static func formatDuration(milliseconds: Double) -> String {
        if milliseconds >= 60_000 {
            let minutes = Int(milliseconds / 60_000)
            let seconds = (milliseconds - Double(minutes) * 60_000) / 1000
            return String(format: "%d:%06.3f", minutes, seconds)
        }
        if milliseconds >= 1000 {
            return String(format: "%.3f s", milliseconds / 1000)
        }
        return String(format: "%.0f ms", milliseconds)
    }
}
