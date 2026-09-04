import AppKit
import ViewerCore

@MainActor
protocol SemanticTextViewDelegate: AnyObject {
    /// The token the editor tracks at a character offset, if any.
    func semanticToken(at offset: Int) -> MSBuildToken?

    /// Whether Cmd-clicking this token goes anywhere. A tracked token that
    /// isn't navigable still shows quick info; it just gets no underline or
    /// link cursor.
    func semanticIsNavigable(_ token: MSBuildToken) -> Bool

    /// Cmd-click on a token.
    func semanticActivate(_ token: MSBuildToken, at rect: NSRect)

    /// A plain click on a token: keep its quick info open so it can be read,
    /// scrolled and copied instead of vanishing with the pointer.
    func semanticPin(_ token: MSBuildToken, at rect: NSRect)

    /// The token under the pointer changed (nil when it left one).
    func semanticHover(_ token: MSBuildToken?, at rect: NSRect)

    /// Anything that should dismiss transient UI: scrolling, typing, a
    /// plain click.
    func semanticDismiss()
}

/// Read-only `NSTextView` with Cmd-click navigation: Cmd-hover underlines
/// the token under the pointer and switches to the link cursor, Cmd-click
/// follows it. Plain clicks still just place the caret.
final class SemanticTextView: NSTextView {
    weak var semanticDelegate: SemanticTextViewDelegate?

    /// Notes drawn in the margin past the end of a line — the evaluated form
    /// of a skipped import's condition. Deliberately not part of the text
    /// storage: every offset the tokenizer, find bar and ruler hold stays
    /// valid, ⌘A/⌘C round-trips the file exactly as the build saw it, and the
    /// find bar never matches text the document doesn't contain. The pill
    /// they're drawn in is what says so on screen.
    var annotations: [SourceAnnotation] = [] {
        didSet {
            guard annotations != oldValue else { return }
            needsDisplay = true
        }
    }

    /// Closest a note may sit to the code before it is dropped instead.
    private static let minimumAnnotationGap: CGFloat = 6

    /// Gap between the end of the element and its note.
    private static let annotationInset: CGFloat = 12

    /// Breathing room between a note's text and the edge of its pill.
    private static let annotationPadding = NSSize(width: 7, height: 2)

    private var mouseTrackingArea: NSTrackingArea?
    private var underlinedRange: NSRange?
    private var hoveredToken: MSBuildToken?
    private var lastMouseLocation: NSPoint?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let mouseTrackingArea {
            removeTrackingArea(mouseTrackingArea)
        }

        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect, .cursorUpdate],
            owner: self,
            userInfo: nil)
        addTrackingArea(area)
        mouseTrackingArea = area
    }

    // MARK: - pointer

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        lastMouseLocation = event.locationInWindow
        updateHover(commandDown: event.modifierFlags.contains(.command))
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        lastMouseLocation = nil
        setUnderline(nil)
        report(token: nil, rect: .zero)
    }

    override func flagsChanged(with event: NSEvent) {
        super.flagsChanged(with: event)
        // Holding or releasing Command mid-hover flips the affordance.
        updateHover(commandDown: event.modifierFlags.contains(.command))
    }

    override func cursorUpdate(with event: NSEvent) {
        if underlinedRange != nil {
            NSCursor.pointingHand.set()
        } else {
            super.cursorUpdate(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        let clicked = characterOffset(at: event.locationInWindow)
            .flatMap { semanticDelegate?.semanticToken(at: $0) }

        if event.modifierFlags.contains(.command), let token = clicked,
           semanticDelegate?.semanticIsNavigable(token) == true {
            semanticDelegate?.semanticActivate(token, at: boundingRect(for: token.range))
            return
        }

        // super runs a tracking loop until mouse-up, so anything decided after
        // it sees the completed gesture — and isn't undone by the click itself.
        super.mouseDown(with: event)

        // A drag or a double-click was a text selection, not a request for
        // quick info.
        if let token = clicked, event.clickCount == 1, selectedRange().length == 0 {
            semanticDelegate?.semanticPin(token, at: boundingRect(for: token.range))
        } else {
            semanticDelegate?.semanticDismiss()
        }
    }

    override func scrollWheel(with event: NSEvent) {
        semanticDelegate?.semanticDismiss()
        super.scrollWheel(with: event)
    }

    override func keyDown(with event: NSEvent) {
        semanticDelegate?.semanticDismiss()
        super.keyDown(with: event)
    }

    private func updateHover(commandDown: Bool) {
        guard let location = lastMouseLocation,
              let offset = characterOffset(at: location),
              let token = semanticDelegate?.semanticToken(at: offset) else {
            setUnderline(nil)
            report(token: nil, rect: .zero)
            return
        }

        let navigable = semanticDelegate?.semanticIsNavigable(token) ?? true
        setUnderline(commandDown && navigable ? token.range : nil)
        report(token: token, rect: boundingRect(for: token.range))
    }

    private func report(token: MSBuildToken?, rect: NSRect) {
        guard hoveredToken != token else { return }
        hoveredToken = token
        semanticDelegate?.semanticHover(token, at: rect)
    }

    /// Underlines via temporary attributes so the syntax-highlighted text
    /// storage is never touched.
    private func setUnderline(_ range: NSRange?) {
        guard underlinedRange != range, let layoutManager else { return }

        if let previous = underlinedRange {
            layoutManager.removeTemporaryAttribute(.underlineStyle, forCharacterRange: previous)
        }

        underlinedRange = range

        if let range {
            layoutManager.addTemporaryAttributes(
                [.underlineStyle: NSUnderlineStyle.single.rawValue],
                forCharacterRange: range)
            NSCursor.pointingHand.set()
        } else {
            NSCursor.iBeam.set()
        }
    }

    /// Clears hover state after the text is replaced — offsets from the old
    /// content mean nothing against the new.
    func resetSemanticState() {
        underlinedRange = nil
        hoveredToken = nil
        annotations = []
    }

    // MARK: - annotations

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        drawAnnotations(in: dirtyRect)
    }

    private func drawAnnotations(in dirtyRect: NSRect) {
        guard !annotations.isEmpty, let textStorage else { return }

        // A size below the code's, so the pill reads as chrome sitting over
        // the document rather than as a line of it.
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: (font?.pointSize ?? 12) - 1, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor,
        ]

        for annotation in annotations {
            // The anchor sits just past the closing `>`, so measure the
            // character before it — that's the glyph we draw after.
            let anchor = annotation.offset - 1
            guard anchor >= 0, anchor < textStorage.length else { continue }

            let rect = boundingRect(for: NSRange(location: anchor, length: 1))
            guard rect.height > 0, rect.intersects(
                NSRect(x: dirtyRect.minX, y: rect.minY, width: dirtyRect.width, height: rect.height)
            ) else { continue }

            let string = NSAttributedString(string: annotation.text, attributes: attributes)
            let textSize = string.size()
            let pillSize = NSSize(
                width: (textSize.width + Self.annotationPadding.width * 2).rounded(.up),
                height: (textSize.height + Self.annotationPadding.height * 2).rounded(.up))

            guard let x = SourceAnnotations.placement(
                elementEnd: rect.maxX,
                noteWidth: pillSize.width,
                trailingMargin: bounds.maxX - textContainerInset.width,
                inset: Self.annotationInset,
                minimumGap: Self.minimumAnnotationGap) else { continue }

            // Half-pixel inset so the 1pt stroke lands on the pixel grid
            // instead of straddling it.
            let pill = NSRect(
                x: x,
                y: (rect.midY - pillSize.height / 2).rounded(),
                width: pillSize.width,
                height: pillSize.height).insetBy(dx: 0.5, dy: 0.5)

            let path = NSBezierPath(
                roundedRect: pill, xRadius: pill.height / 2, yRadius: pill.height / 2)

            // Same fill the tree gives a NoImport's reason, so the two views
            // say the same thing in the same colour.
            NodeStyling.chipFill.setFill()
            path.fill()
            NSColor.separatorColor.setStroke()
            path.lineWidth = 1
            path.stroke()

            string.draw(at: NSPoint(
                x: pill.minX + Self.annotationPadding.width,
                y: pill.midY - textSize.height / 2))
        }
    }

    // MARK: - geometry

    /// The character under a window point, or nil when the point is past the
    /// end of a line (where AppKit would otherwise snap to the nearest glyph).
    func characterOffset(at windowPoint: NSPoint) -> Int? {
        guard let layoutManager, let textContainer, layoutManager.numberOfGlyphs > 0 else { return nil }

        let point = convert(windowPoint, from: nil)
        let origin = textContainerOrigin
        let inContainer = NSPoint(x: point.x - origin.x, y: point.y - origin.y)

        var fraction: CGFloat = 0
        let glyph = layoutManager.glyphIndex(
            for: inContainer, in: textContainer, fractionOfDistanceThroughGlyph: &fraction)
        guard glyph < layoutManager.numberOfGlyphs else { return nil }

        let rect = layoutManager.boundingRect(
            forGlyphRange: NSRange(location: glyph, length: 1), in: textContainer)
        guard rect.contains(inContainer) else { return nil }

        return layoutManager.characterIndexForGlyph(at: glyph)
    }

    func boundingRect(for range: NSRange) -> NSRect {
        guard let layoutManager, let textContainer else { return .zero }
        let glyphRange = layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        var rect = layoutManager.boundingRect(forGlyphRange: glyphRange, in: textContainer)
        rect.origin.x += textContainerOrigin.x
        rect.origin.y += textContainerOrigin.y
        return rect
    }
}
