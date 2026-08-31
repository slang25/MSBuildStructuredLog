import AppKit
import SwiftUI
import ViewerCore

/// Read-only source editor: NSTextView with a line-number ruler, native
/// find bar (⌘F), XML highlighting for build files, and go-to-line with a
/// brief flash. Highlighting for large files happens off the main thread.
struct SourceEditorView: NSViewRepresentable {
    let tab: SourceTab

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        let textView = scrollView.documentView as! NSTextView
        textView.isEditable = false
        textView.isSelectable = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.font = XMLHighlighter.font
        textView.textContainerInset = NSSize(width: 4, height: 4)
        textView.autoresizingMask = [.width]
        textView.backgroundColor = .textBackgroundColor

        let ruler = LineNumberRulerView(textView: textView)
        scrollView.verticalRulerView = ruler
        scrollView.hasVerticalRuler = true
        scrollView.rulersVisible = true

        context.coordinator.textView = textView
        context.coordinator.ruler = ruler
        return scrollView
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        if coordinator.loadedTabId != tab.id || coordinator.loadedContentLength != tab.content.count {
            coordinator.loadedTabId = tab.id
            coordinator.loadedContentLength = tab.content.count
            coordinator.load(content: tab.content, isXML: looksLikeXML)
        }

        if let line = tab.gotoLine, coordinator.lastGotoToken != tab.gotoToken {
            coordinator.lastGotoToken = tab.gotoToken
            coordinator.goToLine(line)
        }
    }

    private var looksLikeXML: Bool {
        if tab.kind == .preprocessed { return true }
        let path = tab.path.lowercased()
        for ext in [".xml", ".csproj", ".vbproj", ".fsproj", ".vcxproj", ".props", ".targets", ".proj", ".tasks", ".overridetasks", ".esproj", ".shproj", ".nuspec", ".config", ".pubxml", ".slnx"] {
            if path.hasSuffix(ext) { return true }
        }
        return tab.content.hasPrefix("<?xml") || tab.content.hasPrefix("<Project")
    }

    @MainActor
    final class Coordinator {
        weak var textView: NSTextView?
        weak var ruler: LineNumberRulerView?
        var loadedTabId: String?
        var loadedContentLength: Int = -1
        var lastGotoToken: Int = -1
        private var highlightTask: Task<Void, Never>?
        private var pendingGotoLine: Int?

        func load(content: String, isXML: Bool) {
            guard let textView else { return }
            highlightTask?.cancel()

            // Show plain text immediately; large XML gets highlighted in the
            // background and swapped in when ready.
            textView.textStorage?.setAttributedString(NSAttributedString(
                string: content,
                attributes: [.font: XMLHighlighter.font, .foregroundColor: NSColor.textColor]))
            ruler?.invalidateLineIndex()
            flushPendingGoto()

            guard isXML else { return }

            if content.count < 200_000 {
                textView.textStorage?.setAttributedString(XMLHighlighter.highlight(content))
                ruler?.invalidateLineIndex()
                flushPendingGoto()
            } else {
                highlightTask = Task.detached(priority: .userInitiated) { [weak self] in
                    let highlighted = XMLHighlighter.highlight(content)
                    guard !Task.isCancelled else { return }
                    await MainActor.run { [weak self] in
                        guard let self, let textView = self.textView else { return }
                        let selection = textView.selectedRange()
                        let visible = textView.visibleRect
                        textView.textStorage?.setAttributedString(highlighted)
                        textView.setSelectedRange(selection)
                        textView.scrollToVisible(visible)
                        self.ruler?.invalidateLineIndex()
                    }
                }
            }
        }

        func goToLine(_ line: Int) {
            guard let textView, let text = textView.textStorage?.string else { return }
            guard let range = Self.range(ofLine: line, in: text) else {
                pendingGotoLine = line
                return
            }

            pendingGotoLine = nil
            textView.scrollRangeToVisible(range)
            textView.setSelectedRange(range)
            textView.showFindIndicator(for: range)
        }

        private func flushPendingGoto() {
            if let line = pendingGotoLine {
                goToLine(line)
            }
        }

        static func range(ofLine line: Int, in text: String) -> NSRange? {
            guard line >= 1 else { return nil }
            var current = 1
            var location = 0
            let ns = text as NSString
            while location < ns.length {
                let lineRange = ns.lineRange(for: NSRange(location: location, length: 0))
                if current == line {
                    return lineRange
                }
                current += 1
                location = NSMaxRange(lineRange)
            }
            return nil
        }
    }
}

/// Minimal line-number ruler for NSTextView.
final class LineNumberRulerView: NSRulerView {
    private weak var textView: NSTextView?
    private var lineStarts: [Int] = []

    init(textView: NSTextView) {
        self.textView = textView
        super.init(scrollView: textView.enclosingScrollView, orientation: .verticalRuler)
        clientView = textView
        ruleThickness = 44

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(textDidChange),
            name: NSText.didChangeNotification,
            object: textView)
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    @objc private func textDidChange() {
        invalidateLineIndex()
    }

    func invalidateLineIndex() {
        lineStarts = []
        needsDisplay = true
    }

    private func buildLineIndexIfNeeded() {
        guard lineStarts.isEmpty, let text = textView?.string as NSString? else { return }
        var starts: [Int] = [0]
        var location = 0
        while location < text.length {
            let lineRange = text.lineRange(for: NSRange(location: location, length: 0))
            location = NSMaxRange(lineRange)
            if location < text.length {
                starts.append(location)
            }
        }
        lineStarts = starts
        let digits = max(3, String(starts.count).count)
        ruleThickness = CGFloat(digits) * 8 + 12
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView, let layoutManager = textView.layoutManager,
              let container = textView.textContainer else { return }

        buildLineIndexIfNeeded()
        guard !lineStarts.isEmpty else { return }

        let visibleRect = textView.visibleRect
        let glyphRange = layoutManager.glyphRange(forBoundingRect: visibleRect, in: container)
        let charRange = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)

        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 9.5, weight: .regular),
            .foregroundColor: NSColor.tertiaryLabelColor,
        ]

        // Binary search the first visible line.
        var low = 0
        var high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= charRange.location { low = mid } else { high = mid - 1 }
        }

        var line = low
        while line < lineStarts.count && lineStarts[line] < NSMaxRange(charRange) {
            let charIndex = lineStarts[line]
            let lineGlyph = layoutManager.glyphIndexForCharacter(at: charIndex)
            let lineRect = layoutManager.lineFragmentRect(forGlyphAt: lineGlyph, effectiveRange: nil)
            let y = lineRect.minY - visibleRect.minY + convert(NSPoint.zero, from: textView).y

            let label = "\(line + 1)" as NSString
            let size = label.size(withAttributes: attributes)
            label.draw(
                at: NSPoint(x: ruleThickness - size.width - 6, y: y + (lineRect.height - size.height) / 2),
                withAttributes: attributes)
            line += 1
        }
    }
}
