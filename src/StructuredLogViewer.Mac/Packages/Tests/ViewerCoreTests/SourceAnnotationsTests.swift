import XCTest
@testable import ViewerCore

final class SourceAnnotationsTests: XCTestCase {
    private let text = """
    <Project>
      <Import Project="A.props" Condition="'$(Flag)' == 'true'" />
      <Import Project="B.props" Condition="'$(Other)' != ''">
      </Import>
      <Import Project="C.props" Condition="'$(Gt)' &gt; '1'" />
    </Project>
    """

    private func anchor(after substring: String) -> Int {
        let range = (text as NSString).range(of: substring)
        return range.location + range.length
    }

    func testAnnotationAnchorsPastTheSelfClosingElement() {
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [SemanticSkippedImport(
                line: 2,
                column: 3,
                fileSpec: "A.props",
                condition: "'$(Flag)' == 'true'",
                evaluatedCondition: "'' == 'true'")])

        XCTAssertEqual(annotations.count, 1)
        XCTAssertEqual(annotations[0].offset, anchor(after: "'$(Flag)' == 'true'\" />"))
        XCTAssertEqual(annotations[0].text, "'' == 'true' → false")
        XCTAssertEqual(annotations[0].line, 2)
    }

    func testAnnotationStopsAtTheOpeningTagOfAPairedElement() {
        // The note belongs beside `<Import ...>`, not after `</Import>`.
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [SemanticSkippedImport(
                line: 3,
                column: 3,
                fileSpec: "B.props",
                condition: "'$(Other)' != ''",
                evaluatedCondition: "'' != ''")])

        XCTAssertEqual(annotations[0].offset, anchor(after: "'$(Other)' != ''\">"))
        XCTAssertEqual(annotations[0].line, 3)
    }

    func testQuotedGreaterThanDoesNotEndTheTagEarly() {
        // `&gt;` is entity-escaped in valid XML, but preprocessed and
        // hand-written files are not always valid — a raw `>` inside an
        // attribute must not be mistaken for the end of the element.
        let raw = "<Project>\n  <Import Project=\"C.props\" Condition=\"'$(N)' > '1'\" />\n</Project>"
        let annotations = SourceAnnotations.importAnnotations(
            text: raw,
            skipped: [SemanticSkippedImport(
                line: 2, column: 3, fileSpec: "C.props",
                condition: "'$(N)' > '1'", evaluatedCondition: "'' > '1'")])

        let expected = (raw as NSString).range(of: "'$(N)' > '1'\" />")
        XCTAssertEqual(annotations[0].offset, expected.location + expected.length)
    }

    func testSeveralSkipsOnOneElementJoinIntoOneNote() {
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "A.props", reason: "no matching files"),
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "A2.props", reason: "the file not existing"),
            ])

        XCTAssertEqual(annotations.count, 1)
        XCTAssertEqual(annotations[0].text, "no matching files · the file not existing")
    }

    func testIdenticalReasonsOnOneElementAreNotRepeated() {
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "A.props", reason: "no matching files"),
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "A2.props", reason: "no matching files"),
            ])

        XCTAssertEqual(annotations[0].text, "no matching files")
    }

    func testAnnotationsComeBackInSourceOrder() {
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [
                SemanticSkippedImport(line: 5, column: 3, fileSpec: "C.props", reason: "third"),
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "A.props", reason: "first"),
                SemanticSkippedImport(line: 3, column: 3, fileSpec: "B.props", reason: "second"),
            ])

        XCTAssertEqual(annotations.map(\.text), ["first", "second", "third"])
    }

    func testAPositionOutsideTheFileIsDroppedRatherThanGuessedAt() {
        // A file re-read from disk can disagree with what the build logged.
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [
                SemanticSkippedImport(line: 99, column: 3, fileSpec: "A.props", reason: "past the end"),
                SemanticSkippedImport(line: 1, column: 1, fileSpec: "B.props", reason: "no import here"),
            ])

        // Line 1 is `<Project>`, which does close, so it still anchors; line
        // 99 does not exist at all.
        XCTAssertEqual(annotations.map(\.text), ["no import here"])
    }

    func testAColumnPastTheEndOfItsLineFallsBackToTheLineStart() {
        let annotations = SourceAnnotations.importAnnotations(
            text: text,
            skipped: [SemanticSkippedImport(line: 2, column: 500, fileSpec: "A.props", reason: "wrong column")])

        XCTAssertEqual(annotations.count, 1)
        XCTAssertEqual(annotations[0].offset, anchor(after: "'$(Flag)' == 'true'\" />"))
    }

    func testNoSkipsMeansNoWork() {
        XCTAssertTrue(SourceAnnotations.importAnnotations(text: text, skipped: []).isEmpty)
    }

    // MARK: - placement

    func testNoteSitsJustAfterTheElementWhenThereIsRoom() {
        let x = SourceAnnotations.placement(
            elementEnd: 100, noteWidth: 80, trailingMargin: 400, inset: 12, minimumGap: 6)
        XCTAssertEqual(x, 112)
    }

    func testNoteSlidesLeftRatherThanRunningOffTheEdge() {
        // The editor wraps instead of scrolling horizontally, so anything
        // drawn past the trailing margin is simply never seen.
        // 272 + 12 would put the note's right edge past 360; sliding it to
        // 280 keeps it on screen and still clear of the code.
        let x = SourceAnnotations.placement(
            elementEnd: 272, noteWidth: 80, trailingMargin: 360, inset: 12, minimumGap: 6)
        XCTAssertEqual(x, 280)
    }

    func testNoteIsDroppedRatherThanDrawnOverTheCode() {
        let x = SourceAnnotations.placement(
            elementEnd: 340, noteWidth: 80, trailingMargin: 360, inset: 12, minimumGap: 6)
        XCTAssertNil(x)
    }

    func testNoteExactlyAtTheMinimumGapStillDraws() {
        let x = SourceAnnotations.placement(
            elementEnd: 100, noteWidth: 80, trailingMargin: 186, inset: 12, minimumGap: 6)
        XCTAssertEqual(x, 106)
    }
}
