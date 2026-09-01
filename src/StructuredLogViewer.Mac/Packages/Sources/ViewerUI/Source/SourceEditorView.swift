import AppKit
import SwiftUI
import ViewerCore

/// Read-only source editor: NSTextView with a line-number ruler, native
/// find bar (⌘F), XML highlighting for build files, and go-to-line with a
/// brief flash. Highlighting for large files happens off the main thread.
///
/// For MSBuild files it is also semantically live: Cmd-hover underlines
/// `$(properties)`, `@(items)`, target names and import paths, hovering
/// shows what the build recorded for them, and Cmd-click navigates.
struct SourceEditorView: NSViewRepresentable {
    let tab: SourceTab
    let sources: SourceController

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true

        // Built by hand rather than via NSTextView.scrollableTextView() so the
        // view is our subclass and stays on TextKit 1, which the ruler's
        // layoutManager geometry depends on.
        let storage = NSTextStorage()
        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(
            containerSize: NSSize(width: scrollView.contentSize.width, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)

        let textView = SemanticTextView(
            frame: NSRect(origin: .zero, size: scrollView.contentSize),
            textContainer: container)
        textView.isEditable = false
        textView.isSelectable = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.font = XMLHighlighter.font
        textView.textContainerInset = NSSize(width: 4, height: 4)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.minSize = NSSize(width: 0, height: scrollView.contentSize.height)
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.autoresizingMask = [.width]
        textView.backgroundColor = .textBackgroundColor
        textView.semanticDelegate = context.coordinator

        scrollView.documentView = textView

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
        coordinator.sources = sources
        coordinator.tabId = tab.id
        coordinator.semantics = tab.semantics

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

    static func dismantleNSView(_ nsView: NSScrollView, coordinator: Coordinator) {
        coordinator.semanticDismiss()
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
    final class Coordinator: NSObject, SemanticTextViewDelegate {
        weak var textView: SemanticTextView?
        weak var ruler: LineNumberRulerView?
        var sources: SourceController?
        var tabId: String?
        var semantics: SourceSemanticIndex?
        var loadedTabId: String?
        var loadedContentLength: Int = -1
        var lastGotoToken: Int = -1
        private var highlightTask: Task<Void, Never>?
        private var hoverTask: Task<Void, Never>?
        private var popover: NSPopover?
        private var pendingGotoLine: Int?

        /// The user clicked a token, so the popover stays put until they
        /// dismiss it — long paths are for reading and copying, not glimpsing.
        private var isPinned = false
        private var dismissMonitor: Any?
        private var resignObserver: NSObjectProtocol?

        /// How long the pointer must rest on a token before quick info shows.
        private static let hoverDelay = Duration.milliseconds(450)

        /// A long reassignment chain would otherwise make the popover taller
        /// than the screen, which AppKit resolves by detaching it entirely.
        private static let maxQuickInfoHeight: CGFloat = 420

        func load(content: String, isXML: Bool) {
            guard let textView else { return }
            highlightTask?.cancel()
            semanticDismiss()
            textView.resetSemanticState()

            // Show plain text immediately; large XML gets highlighted in the
            // background and swapped in when ready.
            textView.textStorage?.setAttributedString(NSAttributedString(
                string: content,
                attributes: [
                    .font: XMLHighlighter.font,
                    // Same base colour the highlighter uses, so swapping in
                    // the highlighted text isn't a visible flash.
                    .foregroundColor: XMLHighlighter.Palette.standard.text,
                ]))
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

        // MARK: - SemanticTextViewDelegate

        func semanticToken(at offset: Int) -> MSBuildToken? {
            semantics?.token(at: offset)
        }

        func semanticHover(_ token: MSBuildToken?, at rect: NSRect) {
            // A pinned popover is the user's; hovering elsewhere must not
            // replace or close it.
            guard !isPinned else { return }
            hoverTask?.cancel()

            guard let token, let sources, let tabId else {
                // Leaving the token towards the popover is how you reach its
                // scrollbar and its text, so that must not dismiss it.
                if !pointerIsOverPopover {
                    closePopover()
                }
                return
            }

            hoverTask = Task { [weak self] in
                try? await Task.sleep(for: Self.hoverDelay)
                guard !Task.isCancelled else { return }
                guard let info = await sources.quickInfo(for: token, in: tabId) else { return }
                guard !Task.isCancelled else { return }
                self?.showQuickInfo(info, at: rect, pinned: false)
            }
        }

        func semanticPin(_ token: MSBuildToken, at rect: NSRect) {
            hoverTask?.cancel()
            guard let sources, let tabId else { return }

            hoverTask = Task { [weak self] in
                guard let info = await sources.quickInfo(for: token, in: tabId) else { return }
                guard !Task.isCancelled else { return }
                self?.showQuickInfo(info, at: rect, pinned: true)
            }
        }

        func semanticActivate(_ token: MSBuildToken, at rect: NSRect) {
            semanticDismiss()
            guard let sources, let tabId, let textView else { return }
            sources.navigate(token: token, in: tabId) { [weak self] locations in
                self?.presentChoices(locations, at: rect, in: textView)
            }
        }

        func semanticDismiss() {
            hoverTask?.cancel()
            hoverTask = nil
            closePopover()
        }

        private var pointerIsOverPopover: Bool {
            guard let window = popover?.contentViewController?.view.window else { return false }
            return window.frame.contains(NSEvent.mouseLocation)
        }

        // MARK: - transient UI

        private func showQuickInfo(_ info: SemanticQuickInfo, at rect: NSRect, pinned: Bool) {
            guard let textView, textView.window != nil else { return }

            // Always a fresh popover, sized before it is shown. NSPopover
            // positions from the content size it has at show time, so content
            // that settles afterwards — as intrinsically-sized SwiftUI does —
            // leaves the popover floating away from the token. Content taller
            // than the screen detaches the same way, so the height is clamped
            // and the overflow scrolls.
            closePopover()

            let content = QuickInfoView(info: info, isPinned: pinned)
            let natural = NSHostingController(rootView: content).sizeThatFits(
                in: NSSize(width: QuickInfoView.width, height: CGFloat.greatestFiniteMagnitude))
            let size = NSSize(
                width: QuickInfoView.width,
                height: min(natural.height, Self.maxQuickInfoHeight))

            let controller = NSHostingController(
                rootView: ScrollView(.vertical) { content }.frame(width: size.width, height: size.height))
            controller.preferredContentSize = size
            controller.view.frame = NSRect(origin: .zero, size: size)

            let popover = NSPopover()
            // Dismissal is ours: the pointer leaving, a scroll, a click or a
            // keystroke. AppKit's transient behaviours fight hover popovers,
            // and a pinned one has to survive the very click that pinned it.
            popover.behavior = .applicationDefined
            popover.animates = false
            popover.contentSize = size
            popover.contentViewController = controller
            self.popover = popover
            isPinned = pinned
            popover.show(relativeTo: rect, of: textView, preferredEdge: .maxY)

            if pinned {
                // Selecting text inside the popover needs its window to be
                // key; a hover popover deliberately stays out of the way.
                popover.contentViewController?.view.window?.makeKey()
                watchForDismissal()
            }
        }

        /// A pinned popover outlives the pointer, so it needs an explicit way
        /// out: any click or keystroke that isn't inside it, or the app losing
        /// focus. Clicks in the editor already come back through the delegate;
        /// this covers the rest of the window.
        private func watchForDismissal() {
            dismissMonitor = NSEvent.addLocalMonitorForEvents(
                matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown, .keyDown]
            ) { [weak self] event in
                guard let self, let window = self.popover?.contentViewController?.view.window else {
                    return event
                }
                if event.window !== window {
                    self.closePopover()
                }
                return event
            }

            resignObserver = NotificationCenter.default.addObserver(
                forName: NSApplication.didResignActiveNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.closePopover() }
            }
        }

        private func closePopover() {
            if let dismissMonitor {
                NSEvent.removeMonitor(dismissMonitor)
            }
            dismissMonitor = nil

            if let resignObserver {
                NotificationCenter.default.removeObserver(resignObserver)
            }
            resignObserver = nil

            isPinned = false
            popover?.performClose(nil)
            popover = nil
        }

        /// Several destinations (an SDK expanding to .props and .targets, a
        /// target overridden in more than one file) become a menu at the token.
        private func presentChoices(_ locations: [SemanticLocation], at rect: NSRect, in view: NSView) {
            let menu = NSMenu()
            menu.autoenablesItems = false

            for location in locations {
                let item = NSMenuItem(
                    title: location.label ?? location.path ?? "(unnamed)",
                    action: #selector(chooseDestination(_:)),
                    keyEquivalent: "")
                item.target = self
                item.representedObject = location
                item.toolTip = location.detail ?? location.path
                item.isEnabled = location.available != false || location.nodeId != nil
                menu.addItem(item)
            }

            menu.popUp(positioning: nil, at: NSPoint(x: rect.minX, y: rect.maxY), in: view)
        }

        @objc private func chooseDestination(_ sender: NSMenuItem) {
            guard let location = sender.representedObject as? SemanticLocation else { return }
            sources?.go(to: location)
        }
    }
}

/// Minimal line-number ruler for NSTextView.
final class LineNumberRulerView: NSRulerView {
    private weak var textView: NSTextView?
    private var lineStarts: [Int] = []
    /// Text length `lineStarts` was built from; -1 when there is no index.
    private var indexedLength = -1
    private var pendingThickness: CGFloat?
    private var rebuildScheduled = false

    /// Sized off the code font so the numbers read as part of the same
    /// document rather than as footnotes beside it.
    private static let labelFont = NSFont.monospacedDigitSystemFont(
        ofSize: XMLHighlighter.font.pointSize - 1.5, weight: .regular)
    private static let digitWidth = ("0" as NSString)
        .size(withAttributes: [.font: labelFont]).width
    /// Gap between the numbers and the text, and between them and the edge.
    private static let labelInset: CGFloat = 7

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

        // AppKit redraws the ruler as the document scrolls, but not on every
        // frame of an elastic overscroll — following the clip view directly
        // keeps the numbers welded to their lines throughout the bounce.
        if let clipView = textView.enclosingScrollView?.contentView {
            clipView.postsBoundsChangedNotifications = true
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(clipViewDidScroll),
                name: NSView.boundsDidChangeNotification,
                object: clipView)
        }
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    @objc private func textDidChange() {
        invalidateLineIndex()
    }

    @objc private func clipViewDidScroll() {
        needsDisplay = true
    }

    /// Rebuilds the index now. Only call this from a plain run-loop turn —
    /// it can change `ruleThickness`, which re-tiles the scroll view.
    func invalidateLineIndex() {
        rebuildLineIndex()
        needsDisplay = true
    }

    private func rebuildLineIndex() {
        rebuildScheduled = false

        guard let text = textView?.string as NSString? else {
            lineStarts = []
            indexedLength = -1
            return
        }

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
        indexedLength = text.length

        let digits = max(3, String(starts.count).count)
        applyRuleThickness(
            (CGFloat(digits) * Self.digitWidth).rounded(.up) + Self.labelInset * 2)
    }

    /// `ruleThickness` re-tiles the enclosing scroll view, so it must never
    /// be assigned from inside a draw or layout pass. Assigning it on the
    /// next run-loop turn keeps that true no matter who invalidated us.
    private func applyRuleThickness(_ desired: CGFloat) {
        // Compare against the value already queued, not the live one — a
        // second rebuild before the queued assignment lands must win.
        guard abs((pendingThickness ?? ruleThickness) - desired) > 0.5 else { return }
        pendingThickness = desired
        DispatchQueue.main.async { [weak self] in
            guard let self, let pending = self.pendingThickness else { return }
            self.pendingThickness = nil
            if abs(self.ruleThickness - pending) > 0.5 {
                self.ruleThickness = pending
            }
        }
    }

    private func scheduleRebuild() {
        guard !rebuildScheduled else { return }
        rebuildScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self, self.rebuildScheduled else { return }
            self.invalidateLineIndex()
        }
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView, let layoutManager = textView.layoutManager,
              let container = textView.textContainer else { return }

        // Never build the index here: it can change `ruleThickness`, and
        // re-tiling the scroll view mid-draw re-enters AppKit layout.
        let length = (textView.string as NSString).length
        guard indexedLength == length, !lineStarts.isEmpty else {
            scheduleRebuild()
            return
        }

        let visibleRect = textView.visibleRect
        let glyphRange = layoutManager.glyphRange(forBoundingRect: visibleRect, in: container)
        let charRange = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)

        let attributes: [NSAttributedString.Key: Any] = [
            .font: Self.labelFont,
            .foregroundColor: NSColor.tertiaryLabelColor,
        ]

        // Binary search the first visible line.
        var low = 0
        var high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= charRange.location { low = mid } else { high = mid - 1 }
        }

        // Line fragments are in text-container space; `textContainerOrigin`
        // lifts them into the text view, and `convert` carries them across to
        // the ruler. Subtracting the scroll offset by hand as well would count
        // it twice — which is what made the numbers slide away from their
        // lines the further you scrolled, and drift again on elastic bounce.
        let containerOrigin = textView.textContainerOrigin
        let glyphCount = layoutManager.numberOfGlyphs
        // A wrapped logical line owns several fragments; the number belongs to
        // the first, which may sit above the visible rect. Keep drawing past
        // the bottom edge by one line so the last row is never left bare.
        let bottom = visibleRect.maxY

        var line = low
        while line < lineStarts.count {
            let charIndex = lineStarts[line]
            guard charIndex < length else { break }
            let lineGlyph = layoutManager.glyphIndexForCharacter(at: charIndex)
            guard lineGlyph < glyphCount else { break }
            let lineRect = layoutManager.lineFragmentRect(forGlyphAt: lineGlyph, effectiveRange: nil)
            if lineRect.minY + containerOrigin.y > bottom { break }

            let inTextView = NSPoint(x: 0, y: lineRect.minY + containerOrigin.y)
            let y = convert(inTextView, from: textView).y

            let label = "\(line + 1)" as NSString
            let size = label.size(withAttributes: attributes)
            label.draw(
                at: NSPoint(
                    x: ruleThickness - size.width - Self.labelInset,
                    y: (y + (lineRect.height - size.height) / 2).rounded()),
                withAttributes: attributes)
            line += 1
        }
    }
}
