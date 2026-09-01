import AppKit
import ViewerCore

@MainActor
protocol SemanticTextViewDelegate: AnyObject {
    /// The navigable token at a character offset, if any.
    func semanticToken(at offset: Int) -> MSBuildToken?

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

        if event.modifierFlags.contains(.command), let token = clicked {
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

        setUnderline(commandDown ? token.range : nil)
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
