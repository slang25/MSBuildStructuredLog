import AppKit
import XCTest
import ViewerCore
@testable import ViewerUI

/// Renders the editor offscreen and inspects the pixels. The point of the
/// annotation pill is that it is chrome drawn *over* the document rather
/// than text in it, and that distinction only exists on screen — nothing
/// about the text storage can prove it.
@MainActor
final class AnnotationRenderingTests: XCTestCase {
    private let source = """
    <Project>
      <Import Project="A.props" Condition="'$(Flag)' == 'true'" />
    </Project>
    """

    /// A laid-out text view, wide enough that the note has somewhere to go.
    private func makeTextView(annotations: [SourceAnnotation]) -> SemanticTextView {
        let storage = NSTextStorage(string: source)
        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 900, height: 400))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)

        let textView = SemanticTextView(
            frame: NSRect(x: 0, y: 0, width: 900, height: 400), textContainer: container)
        textView.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        textView.textColor = .labelColor
        textView.backgroundColor = .textBackgroundColor
        textView.annotations = annotations
        layoutManager.ensureLayout(for: container)
        return textView
    }

    private func render(_ textView: SemanticTextView) -> NSBitmapImageRep {
        let rep = textView.bitmapImageRepForCachingDisplay(in: textView.bounds)!
        textView.cacheDisplay(in: textView.bounds, to: rep)
        return rep
    }

    /// Guards the harness itself: a blank render would make every pixel
    /// comparison below vacuously true.
    private func assertRendersSomething(_ rep: NSBitmapImageRep, _ what: String) {
        var distinct = Set<String>()
        // pixelsWide/High, not size — size is in points and the backing store
        // is 2x on Retina, so scanning by size covers only a quarter of it.
        for y in stride(from: 0, to: rep.pixelsHigh, by: 3) {
            for x in stride(from: 0, to: rep.pixelsWide, by: 3) {
                if let c = rep.colorAt(x: x, y: y) { distinct.insert(c.description) }
            }
        }
        XCTAssertGreaterThan(distinct.count, 1, "\(what) rendered blank — the harness drew nothing")
    }

    /// Anchor just past the `/>` that closes the Import on line 2.
    private var importAnchor: Int {
        let range = (source as NSString).range(of: "'$(Flag)' == 'true'\" />")
        return range.location + range.length
    }

    func testPillIsDrawnBehindTheNote() {
        let annotated = render(makeTextView(annotations: [
            SourceAnnotation(offset: importAnchor, text: "'' == 'true' → false", line: 2)
        ]))
        let plain = render(makeTextView(annotations: []))

        assertRendersSomething(plain, "the plain editor")
        assertRendersSomething(annotated, "the annotated editor")

        // Somewhere to the right of the element, the annotated render must
        // differ from the plain one — and not only where glyphs land, which
        // is what a bare label would give. A filled pill changes a
        // contiguous run of pixels, including the gaps between letters.
        var differing = 0
        var longestRun = 0
        var run = 0
        for y in 0..<annotated.pixelsHigh {
            for x in 0..<annotated.pixelsWide {
                guard let a = annotated.colorAt(x: x, y: y), let b = plain.colorAt(x: x, y: y) else { continue }
                if a != b {
                    differing += 1
                    run += 1
                    longestRun = max(longestRun, run)
                } else {
                    run = 0
                }
            }
            run = 0
        }

        XCTAssertGreaterThan(differing, 0, "the annotation drew nothing at all")
        XCTAssertGreaterThan(
            longestRun, 60,
            "no unbroken horizontal run — the note has no pill behind it, only glyphs")
    }

    func testPillDoesNotTouchTheTextStorage() {
        // The whole reason the pill is drawn rather than inserted: ⌘A ⌘C
        // must return the file, not the file plus our commentary.
        let textView = makeTextView(annotations: [
            SourceAnnotation(offset: importAnchor, text: "'' == 'true' → false", line: 2)
        ])
        _ = render(textView)

        XCTAssertEqual(textView.string, source)
        XCTAssertFalse(textView.string.contains("false"))

        textView.selectAll(nil)
        let selected = (textView.string as NSString).substring(with: textView.selectedRange())
        XCTAssertEqual(selected, source)
    }

    func testAnnotationsAreClearedWhenTheContentIsReplaced() {
        // Offsets from the old text mean nothing against the new, so a stale
        // note must not survive a document swap.
        let textView = makeTextView(annotations: [
            SourceAnnotation(offset: importAnchor, text: "'' == 'true' → false", line: 2)
        ])
        textView.resetSemanticState()
        XCTAssertTrue(textView.annotations.isEmpty)
    }

    func testNoteWithNowhereToGoIsNotDrawnOverTheCode() {
        // A pane too narrow for the pill drops it rather than painting over
        // the source.
        let textView = makeTextView(annotations: [
            SourceAnnotation(offset: importAnchor, text: String(repeating: "x", count: 400), line: 2)
        ])
        let annotated = render(textView)
        let plain = render(makeTextView(annotations: []))

        for y in 0..<annotated.pixelsHigh {
            for x in 0..<annotated.pixelsWide {
                XCTAssertEqual(
                    annotated.colorAt(x: x, y: y), plain.colorAt(x: x, y: y),
                    "an over-long note should be dropped, not drawn at (\(x), \(y))")
            }
        }
    }
}
