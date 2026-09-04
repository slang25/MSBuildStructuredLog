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
            return Style(symbolName: "folder.fill", color: .systemYellow)
        case .message, .timedMessage:
            return Style(symbolName: "text.bubble", color: .secondaryLabelColor)
        case .criticalBuildMessage:
            return Style(symbolName: "exclamationmark.bubble", color: .systemOrange)
        case .error:
            return Style(symbolName: "xmark.circle.fill", color: .systemRed)
        case .warning:
            return Style(symbolName: "exclamationmark.triangle.fill", color: .systemYellow)
        case .note:
            return Style(symbolName: "note.text", color: .secondaryLabelColor)
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
            return Style(symbolName: "text.alignleft", color: .secondaryLabelColor)
        case .evaluationProfileEntry, .timedNode:
            return Style(symbolName: "clock", color: .secondaryLabelColor)
        case .msBuildServerNode:
            return Style(symbolName: "server.rack", color: .systemIndigo)
        case .proxy, .unknown:
            return Style(symbolName: "folder", color: .secondaryLabelColor)
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

    /// Badges sit below the row's text size, as in the other viewers, so a
    /// long TFM list stays a label rather than competing with the name.
    public static let badgeFont = NSFont.systemFont(ofSize: rowFontSize - 2)

    /// Rows are one fixed-height line; attributed strings need the
    /// truncation carried in their own paragraph style.
    static let singleLineStyle: NSParagraphStyle = {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        return style
    }()

    /// The fill behind a NoImport row's reason, matching the viewers'
    /// NoImportFill resource (BlanchedAlmond light, #474138 dark).
    static let chipFill = NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        return isDark
            ? NSColor(calibratedRed: 0.278, green: 0.255, blue: 0.220, alpha: 1)
            : NSColor(calibratedRed: 1.0, green: 0.922, blue: 0.804, alpha: 1)
    }

    /// A project's target-framework badge, matching the viewers'
    /// TargetFrameworkBackground/Foreground (#E5F0E5 on #274E13). Those
    /// resources have no dark variant upstream — the light pair is unusable
    /// on a dark row — so the dark side inverts the same green.
    static let badgeFill = NSColor(name: nil) { appearance in
        appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            ? NSColor(srgbRed: 0.129, green: 0.196, blue: 0.114, alpha: 1)
            : NSColor(srgbRed: 0.898, green: 0.941, blue: 0.898, alpha: 1)
    }

    static let badgeText = NSColor(name: nil) { appearance in
        appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            ? NSColor(srgbRed: 0.729, green: 0.855, blue: 0.667, alpha: 1)
            : NSColor(srgbRed: 0.153, green: 0.306, blue: 0.075, alpha: 1)
    }

    /// A hair of breathing room inside a badge. An attributed-string
    /// background is a plain rectangle, so the padding has to be characters.
    private static let badgePadding = "\u{2009}"

    /// Attributed row text for a tree node. Import and NoImport rows carry
    /// their type label, source position and skip reason as separate runs,
    /// like the WPF and Avalonia node templates; everything else is its
    /// title. Newlines in multi-line message text collapse so the row stays
    /// a single line.
    public static func rowText(for summary: NodeSummary) -> NSAttributedString {
        let text = NSMutableAttributedString()
        let dimmed = summary.isLowRelevance
        let tint = style(for: summary).color

        for segment in NodeRowText.segments(for: summary) {
            if text.length > 0 {
                text.append(NSAttributedString(
                    string: segment.style == .duration ? "  " : " ",
                    attributes: [.font: rowFont, .paragraphStyle: singleLineStyle]))
            }

            var attributes: [NSAttributedString.Key: Any] = [
                .font: rowFont,
                .paragraphStyle: singleLineStyle,
            ]

            switch segment.style {
            case .kindLabel:
                attributes[.foregroundColor] = dimmed ? tint.withAlphaComponent(0.5) : tint
            case .primary:
                attributes[.foregroundColor] = dimmed ? NSColor.tertiaryLabelColor : NSColor.labelColor
            case .secondary:
                attributes[.foregroundColor] = NSColor.secondaryLabelColor
            case .chip:
                attributes[.foregroundColor] = dimmed ? NSColor.tertiaryLabelColor : NSColor.labelColor
                attributes[.backgroundColor] = dimmed ? chipFill.withAlphaComponent(0.5) : chipFill
            case .badge:
                attributes[.font] = badgeFont
                attributes[.foregroundColor] = dimmed ? badgeText.withAlphaComponent(0.5) : badgeText
                attributes[.backgroundColor] = dimmed ? badgeFill.withAlphaComponent(0.5) : badgeFill
            case .targets:
                attributes[.foregroundColor] = dimmed
                    ? NSColor.systemPurple.withAlphaComponent(0.5)
                    : NSColor.systemPurple
            case .duration:
                attributes[.font] = durationFont
                attributes[.foregroundColor] = NSColor.secondaryLabelColor
            }

            let body = segment.style == .badge
                ? badgePadding + segment.text + badgePadding
                : segment.text
            text.append(NSAttributedString(string: singleLine(body), attributes: attributes))
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
        NodeRowText.formatDuration(milliseconds: milliseconds)
    }
}
