import AppKit
import SwiftUI
import ViewerCore

/// Single source of truth for node row appearance: SF Symbol + tint per
/// node kind, mirroring the Avalonia viewer's icon set (Resources.xaml
/// strokes: target=MediumPurple, task=DodgerBlue, item=MediumAquamarine,
/// folder=Goldenrod, import=Sienna, error=red, warning=gold, ...).
public enum NodeStyling {
    public struct Style {
        public let symbolName: String
        public let color: NSColor
    }

    public static func style(for summary: NodeSummary) -> Style {
        switch summary.nodeKind {
        case .build:
            switch summary.state {
            case .failed: return Style(symbolName: "hammer.circle.fill", color: .systemRed)
            default: return Style(symbolName: "hammer.circle.fill", color: .systemGreen)
            }
        case .project:
            return Style(symbolName: "shippingbox.fill", color: projectTint(summary))
        case .projectEvaluation:
            return Style(symbolName: "shippingbox", color: projectTint(summary))
        case .target:
            return Style(symbolName: "scope", color: .systemPurple)
        case .task:
            return Style(symbolName: "gearshape.fill", color: .systemBlue)
        case .addItem:
            return Style(symbolName: "plus.square.fill", color: .systemTeal)
        case .removeItem:
            return Style(symbolName: "minus.square.fill", color: .systemRed)
        case .item, .taskParameterItem:
            return Style(symbolName: "doc.on.doc", color: .systemMint)
        case .metadata:
            return Style(symbolName: "tag", color: .systemTeal)
        case .property, .taskParameterProperty:
            return Style(symbolName: "p.square", color: .systemBlue)
        case .parameter:
            return Style(symbolName: "arrow.right.square", color: .systemBlue)
        case .folder:
            return Style(symbolName: "folder.fill", color: NSColor(named: "folderTint") ?? .systemYellow)
        case .message, .timedMessage:
            return Style(symbolName: "text.bubble", color: .tertiaryLabelColor)
        case .criticalBuildMessage:
            return Style(symbolName: "exclamationmark.bubble", color: .systemOrange)
        case .error:
            return Style(symbolName: "xmark.circle.fill", color: .systemRed)
        case .warning:
            return Style(symbolName: "exclamationmark.triangle.fill", color: .systemYellow)
        case .note:
            return Style(symbolName: "note.text", color: .tertiaryLabelColor)
        case .importNode:
            return Style(symbolName: "arrow.down.doc.fill", color: .systemBrown)
        case .noImport:
            return Style(symbolName: "arrow.down.doc", color: .systemRed)
        case .entryTarget:
            return Style(symbolName: "smallcircle.filled.circle", color: .systemPurple)
        case .package:
            return Style(symbolName: "cube.fill", color: NSColor(calibratedRed: 0, green: 0.28, blue: 0.5, alpha: 1))
        case .fileCopy:
            return fileCopyStyle(summary)
        case .sourceFile:
            return Style(symbolName: "doc.text", color: .systemBlue)
        case .sourceFileLine:
            return Style(symbolName: "text.alignleft", color: .tertiaryLabelColor)
        case .evaluationProfileEntry, .timedNode:
            return Style(symbolName: "clock", color: .tertiaryLabelColor)
        case .msBuildServerNode:
            return Style(symbolName: "server.rack", color: .systemIndigo)
        case .proxy, .unknown:
            return Style(symbolName: "folder", color: .tertiaryLabelColor)
        }
    }

    /// Target/task rows show a failure/skip accent in the viewer.
    public static func stateAccent(for summary: NodeSummary) -> NSColor? {
        switch summary.state {
        case .failed: return .systemRed
        case .skipped: return .tertiaryLabelColor
        default: return nil
        }
    }

    private static func projectTint(_ summary: NodeSummary) -> NSColor {
        let ext = summary.props?["extension"]?.lowercased() ?? ""
        switch ext {
        case ".csproj": return NSColor(calibratedRed: 0.22, green: 0.54, blue: 0.20, alpha: 1)
        case ".vbproj": return NSColor(calibratedRed: 0.0, green: 0.33, blue: 0.61, alpha: 1)
        case ".fsproj": return NSColor(calibratedRed: 0.41, green: 0.16, blue: 0.47, alpha: 1)
        case ".vcxproj", ".cppproj": return NSColor(calibratedRed: 0.64, green: 0.25, blue: 0.69, alpha: 1)
        case ".sln", ".slnx", ".slnf": return NSColor(calibratedRed: 0.40, green: 0.13, blue: 0.47, alpha: 1)
        case ".esproj": return .systemYellow
        default: return .systemGray
        }
    }

    private static func fileCopyStyle(_ summary: NodeSummary) -> Style {
        switch summary.props?["copyKind"] {
        case "Source": return Style(symbolName: "arrow.up.doc", color: .systemBlue)
        case "Destination": return Style(symbolName: "arrow.down.doc", color: .systemGreen)
        case "SourceAndDestination": return Style(symbolName: "arrow.up.arrow.down.circle", color: .systemTeal)
        default: return Style(symbolName: "doc.on.clipboard", color: .systemBlue)
        }
    }

    // MARK: - row text

    // Row metrics for the main tree: comfortable macOS list sizing
    // (13pt system text, 24pt rows, 18pt icons).
    public static let rowHeight: CGFloat = 24
    public static let iconSize: CGFloat = 18
    public static let indentPerLevel: CGFloat = 16
    public static let rowFontSize: CGFloat = NSFont.systemFontSize
    public static let rowFont = NSFont.systemFont(ofSize: rowFontSize)
    public static let rowBoldFont = NSFont.boldSystemFont(ofSize: rowFontSize)
    public static let durationFont = NSFont.monospacedDigitSystemFont(ofSize: rowFontSize - 1, weight: .regular)

    /// Rows are one fixed-height line; attributed strings need the
    /// truncation carried in their own paragraph style.
    static let singleLineStyle: NSParagraphStyle = {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        return style
    }()

    /// Attributed row text for a plain tree node. Newlines in multi-line
    /// message text collapse so the row stays a single line.
    public static func rowText(for summary: NodeSummary) -> NSAttributedString {
        let text = NSMutableAttributedString()
        let baseColor: NSColor = summary.isLowRelevance ? .tertiaryLabelColor : .labelColor

        text.append(NSAttributedString(string: singleLine(summary.title), attributes: [
            .font: rowFont,
            .foregroundColor: baseColor,
            .paragraphStyle: singleLineStyle,
        ]))

        if let duration = summary.durationMs, duration > 0 {
            text.append(NSAttributedString(string: "  " + formatDuration(milliseconds: duration), attributes: [
                .font: durationFont,
                .foregroundColor: NSColor.secondaryLabelColor,
                .paragraphStyle: singleLineStyle,
            ]))
        }

        return text
    }

    static func singleLine(_ text: String) -> String {
        guard text.contains(where: \.isNewline) else { return text }
        return text.split(whereSeparator: \.isNewline).joined(separator: " ⏎ ")
    }

    /// Attributed row text for a search result with bold, tinted highlight
    /// spans (and dimmed "time" style spans).
    public static func highlightedText(_ highlights: [Highlight], isLowRelevance: Bool = false) -> NSAttributedString {
        let text = NSMutableAttributedString()
        let baseColor: NSColor = isLowRelevance ? .tertiaryLabelColor : .labelColor

        for span in highlights {
            if span.style == "time" {
                text.append(NSAttributedString(string: singleLine(span.text), attributes: [
                    .font: durationFont,
                    .foregroundColor: NSColor.secondaryLabelColor,
                    .paragraphStyle: singleLineStyle,
                ]))
            } else if span.isHighlight == true {
                text.append(NSAttributedString(string: singleLine(span.text), attributes: [
                    .font: rowBoldFont,
                    .foregroundColor: NSColor.controlAccentColor,
                    .paragraphStyle: singleLineStyle,
                ]))
            } else {
                text.append(NSAttributedString(string: singleLine(span.text), attributes: [
                    .font: rowFont,
                    .foregroundColor: baseColor,
                    .paragraphStyle: singleLineStyle,
                ]))
            }
        }

        return text
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
