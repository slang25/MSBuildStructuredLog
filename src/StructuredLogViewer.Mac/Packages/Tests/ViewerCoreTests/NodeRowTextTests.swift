import XCTest
@testable import ViewerCore

/// Import and NoImport rows must say as much as the WPF and Avalonia
/// viewers do: type label, path, source position, and — for a NoImport —
/// why the build passed it over.
final class NodeRowTextTests: XCTestCase {
    private func summary(
        kind: String,
        title: String,
        props: [String: String]? = nil,
        durationMs: Double? = nil
    ) -> NodeSummary {
        NodeSummary(id: "1", kind: kind, title: title, durationMs: durationMs, props: props)
    }

    func testImportRowCarriesItsLocation() {
        let segments = NodeRowText.segments(for: summary(
            kind: "Import",
            title: "/sdk/Microsoft.Common.props",
            props: ["line": "49", "column": "3"]))

        XCTAssertEqual(segments.map(\.text), ["Import", "/sdk/Microsoft.Common.props", "at (49;3)"])
        XCTAssertEqual(segments.map(\.style), [.kindLabel, .primary, .secondary])
    }

    func testNoImportRowCarriesItsReasonAsAChip() {
        let segments = NodeRowText.segments(for: summary(
            kind: "NoImport",
            title: "$(CustomBeforeDirectoryBuildProps)",
            props: [
                "line": "32",
                "column": "3",
                "reason": "Not imported due to false condition; ('$(X)' != '') was evaluated as ('' != '').",
            ]))

        XCTAssertEqual(segments.map(\.style), [.kindLabel, .primary, .secondary, .chip])
        XCTAssertEqual(segments.first?.text, "NoImport")
        XCTAssertEqual(segments[2].text, "at (32;3)")
        XCTAssertTrue(segments[3].text.hasPrefix("Not imported due to false condition"))
    }

    func testImplicitImportKeepsItsZeroPosition() {
        // MSBuild reports SDK expansions at (0;0) — there is no element to
        // point at. The viewers show it anyway, so the position of every
        // import row lines up in the column.
        let segments = NodeRowText.segments(for: summary(
            kind: "Import",
            title: "/sdk/Sdk.props",
            props: ["line": "0", "column": "0"]))

        XCTAssertEqual(segments.last?.text, "at (0;0)")
    }

    func testImportWithoutAPositionOmitsIt() {
        let segments = NodeRowText.segments(for: summary(kind: "Import", title: "/sdk/Sdk.props"))
        XCTAssertEqual(segments.map(\.text), ["Import", "/sdk/Sdk.props"])
    }

    func testNoImportWithoutAReasonOmitsTheChip() {
        let segments = NodeRowText.segments(for: summary(
            kind: "NoImport",
            title: "Missing.props",
            props: ["line": "4", "column": "3"]))

        XCTAssertFalse(segments.contains { $0.style == .chip })
    }

    // MARK: - project rows

    private func projectSummary(props: [String: String]) -> NodeSummary {
        NodeSummary(
            id: "1",
            kind: "Project",
            // The engine glues the parts into title for copy and tooltips;
            // the row is built from the parts, not by splitting this.
            title: "JustSaying.csproj netstandard2.0;net8.0 → Rebuild",
            name: "JustSaying.csproj",
            props: props)
    }

    func testProjectRowBadgesItsTargetFrameworks() {
        let segments = NodeRowText.segments(for: projectSummary(props: [
            "adornment": "netstandard2.0;net8.0",
            "targetsText": "→ Rebuild",
            "evaluationText": "id:62",
            "evaluationNodeId": "38",
        ]))

        XCTAssertEqual(segments.map(\.style), [.primary, .badge, .targets])
        XCTAssertEqual(segments.map(\.text), ["JustSaying.csproj", "netstandard2.0;net8.0", "→ Rebuild"])
    }

    func testProjectRowLeavesTheEvaluationIdToTheRowsLink() {
        // It is rendered as a clickable link by the cell, so repeating it in
        // the text would show it twice.
        let segments = NodeRowText.segments(for: projectSummary(props: [
            "adornment": "net10.0", "evaluationText": "id:62", "evaluationNodeId": "38",
        ]))
        XCTAssertFalse(segments.contains { $0.text.contains("id:62") })
    }

    func testUnresolvableEvaluationIdIsShownAsTextRatherThanLost() {
        // No destination to click, but the id still tells you which
        // evaluation produced the project.
        let segments = NodeRowText.segments(for: projectSummary(props: [
            "adornment": "net10.0", "evaluationText": "id:62",
        ]))
        XCTAssertEqual(segments.last?.text, "id:62")
        XCTAssertEqual(segments.last?.style, .secondary)
    }

    func testProjectWithNoAdornmentOrTargetsIsJustItsName() {
        let segments = NodeRowText.segments(for: projectSummary(props: [:]))
        XCTAssertEqual(segments, [NodeRowSegment(text: "JustSaying.csproj", style: .primary)])
    }

    func testProjectWithoutANameFallsBackToTheGluedTitle() {
        let summary = NodeSummary(
            id: "1", kind: "Project", title: "Fallback.csproj net8.0", props: ["adornment": "net8.0"])
        XCTAssertEqual(
            NodeRowText.segments(for: summary),
            [NodeRowSegment(text: "Fallback.csproj net8.0", style: .primary)])
    }

    func testEvaluationRowsBadgeTheirFrameworksToo() {
        let summary = NodeSummary(
            id: "1", kind: "ProjectEvaluation", title: "JustSaying.csproj net8.0 id:62",
            name: "JustSaying.csproj", durationMs: 26,
            props: ["adornment": "net8.0", "evaluationText": "id:62", "evaluationNodeId": "38"])

        XCTAssertEqual(NodeRowText.segments(for: summary).map(\.style), [.primary, .badge, .duration])
    }

    func testOtherKindsAreJustTheirTitle() {
        let segments = NodeRowText.segments(for: summary(kind: "Target", title: "Build"))
        XCTAssertEqual(segments, [NodeRowSegment(text: "Build", style: .primary)])
    }

    func testDurationTrailsTheRow() {
        let segments = NodeRowText.segments(for: summary(
            kind: "Target", title: "Build", durationMs: 1500))

        XCTAssertEqual(segments.map(\.style), [.primary, .duration])
        XCTAssertEqual(segments.last?.text, "1.500 s")
    }
}
