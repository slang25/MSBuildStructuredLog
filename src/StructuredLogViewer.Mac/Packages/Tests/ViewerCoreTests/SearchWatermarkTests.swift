import XCTest
@testable import ViewerCore

final class SearchWatermarkTests: XCTestCase {
    func testLinkRoundTripsAwkwardQueries() throws {
        for query in SearchWatermark.examples + SearchWatermark.nodeKinds + ["$secret not(username)", "start<\"2023-11-23\""] {
            let url = try XCTUnwrap(URL(string: SearchLink.url(for: query)))
            XCTAssertEqual(SearchLink.query(from: url), query)
        }
    }

    func testForeignURLsAreIgnored() throws {
        XCTAssertNil(SearchLink.query(from: try XCTUnwrap(URL(string: "https://example.com"))))
    }

    func testRenderReplacesPlaceholdersOnly() {
        let rendered = SearchWatermark.render("Append [[$time]] to sort. Keep [brackets] alone.")
        XCTAssertEqual(
            rendered,
            "Append [$time](\(SearchLink.url(for: "$time"))) to sort. Keep [brackets] alone.")
    }

    func testRenderTrimsTrailingSpaceFromLabel() {
        // "$property " keeps its trailing space in the query (so the caret
        // lands after it) but not in the visible label.
        let rendered = SearchWatermark.render("[[$property ]]")
        XCTAssertTrue(rendered.hasPrefix("[$property]("), rendered)
        XCTAssertEqual(SearchLink.query(from: URL(string: String(rendered.dropFirst("[$property](".count).dropLast()))!), "$property ")
    }

    /// Every watermark paragraph must survive markdown parsing, and each
    /// placeholder must come out the other side as a tappable link.
    func testAllProseParsesAsMarkdownWithLinks() throws {
        let blocks = [
            SearchWatermark.matching,
            SearchWatermark.clauses,
            SearchWatermark.modifiers,
            SearchWatermark.propertiesAndItems,
            SearchWatermark.examples.map { " • \(SearchWatermark.link($0))" }.joined(separator: "\n"),
            SearchWatermark.nodeKinds.map { SearchWatermark.link("\($0) ") }.joined(separator: ", "),
        ]

        for block in blocks {
            let attributed = try AttributedString(
                markdown: SearchWatermark.render(block),
                options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))

            let queries = attributed.runs.compactMap { run in run.link.flatMap(SearchLink.query(from:)) }
            XCTAssertFalse(queries.isEmpty, "no links in block: \(block.prefix(40))")
            XCTAssertFalse(String(attributed.characters).contains("[["), "unrendered placeholder in: \(block.prefix(40))")
        }
    }

    func testExampleQueriesAreLinkedFromTheExamplesBlock() throws {
        let markdown = SearchWatermark.examples.map { " • \(SearchWatermark.link($0))" }.joined(separator: "\n")
        let attributed = try AttributedString(
            markdown: SearchWatermark.render(markdown),
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))

        let queries = attributed.runs.compactMap { run in run.link.flatMap(SearchLink.query(from:)) }
        XCTAssertEqual(queries, SearchWatermark.examples)
    }
}
