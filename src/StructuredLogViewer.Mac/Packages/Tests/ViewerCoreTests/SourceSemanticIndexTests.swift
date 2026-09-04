import XCTest
@testable import ViewerCore

final class SourceSemanticIndexTests: XCTestCase {
    private let text = """
    <Project Sdk="Microsoft.NET.Sdk">
      <Import Project="Shared.props" />
      <Import Project="Missing.props" />
      <PropertyGroup><Out>$(BaseOut)</Out></PropertyGroup>
    </Project>
    """

    private func index(
        imports: [SemanticImport],
        skipped: [SemanticSkippedImport] = []
    ) -> SourceSemanticIndex {
        SourceSemanticIndex(
            text: text,
            file: SemanticFile(
                path: "a.csproj",
                evaluationId: "7",
                imports: imports,
                skippedImports: skipped))
    }

    private func offset(of substring: String) -> Int {
        (text as NSString).range(of: substring).location
    }

    func testImportWithARecordedEdgeNavigatesToIt() {
        let index = index(imports: [
            SemanticImport(line: 2, importedPath: "/repo/Shared.props", available: true)
        ])

        let token = index.token(at: offset(of: "Shared.props"))
        XCTAssertEqual(token?.kind, .importPath)

        guard case .open(let location) = index.importNavigation(for: token!) else {
            return XCTFail("expected a single destination")
        }
        XCTAssertEqual(location.path, "/repo/Shared.props")
    }

    func testImportTheBuildNeverMentionedIsNotTracked() {
        // Line 3's <Import> appears in neither list — the build has nothing
        // to say about it, so it gets no token at all rather than a link
        // that goes nowhere.
        let index = index(imports: [
            SemanticImport(line: 2, importedPath: "/repo/Shared.props", available: true)
        ])
        XCTAssertNil(index.token(at: offset(of: "Missing.props")))
    }

    func testSkippedImportIsTrackedButNotNavigable() {
        let index = index(
            imports: [SemanticImport(line: 2, importedPath: "/repo/Shared.props", available: true)],
            skipped: [SemanticSkippedImport(
                line: 3,
                fileSpec: "Missing.props",
                reason: "Not imported due to false condition",
                condition: "'$(Flag)' == 'true'",
                evaluatedCondition: "'' == 'true'")])

        // Tracked, so hovering explains it...
        let token = index.token(at: offset(of: "Missing.props"))
        XCTAssertEqual(token?.kind, .importPath)

        // ...but no underline and no destination.
        XCTAssertFalse(index.isNavigable(token!))
        guard case .none(let reason) = index.importNavigation(for: token!) else {
            return XCTFail("a skipped import has nowhere to go")
        }
        XCTAssertEqual(reason, "Condition '$(Flag)' == 'true' evaluated as '' == 'true' → false")
    }

    func testTakenImportStaysNavigable() {
        let index = index(
            imports: [SemanticImport(line: 2, importedPath: "/repo/Shared.props", available: true)],
            skipped: [SemanticSkippedImport(line: 3, fileSpec: "Missing.props", reason: "gone")])

        XCTAssertTrue(index.isNavigable(index.token(at: offset(of: "Shared.props"))!))
    }

    func testSkipWithoutAConditionFallsBackToTheLoggedReason() {
        let index = index(
            imports: [],
            skipped: [SemanticSkippedImport(
                line: 3,
                fileSpec: "Missing.props",
                reason: "Not imported due to no matching files")])

        let token = index.token(at: offset(of: "Missing.props"))!
        guard case .none(let reason) = index.importNavigation(for: token) else {
            return XCTFail("a skipped import has nowhere to go")
        }
        XCTAssertEqual(reason, "Not imported due to no matching files")
        XCTAssertEqual(index.skippedImports(for: token).count, 1)
    }

    func testSkippedImportsAreOrderedBySourcePosition() {
        let index = index(
            imports: [],
            skipped: [
                SemanticSkippedImport(line: 3, column: 3, fileSpec: "b"),
                SemanticSkippedImport(line: 2, column: 3, fileSpec: "a"),
                // Line 0 is an implicit SDK expansion: no element to anchor to.
                SemanticSkippedImport(line: 0, fileSpec: "implicit"),
            ])

        XCTAssertEqual(index.skippedImports.map(\.fileSpec), ["a", "b"])
    }

    func testSdkAttributeFallsBackToImplicitImports() {
        // MSBuild logs the imports an Sdk attribute expands to with line 0,
        // because there is no <Import> element to point at.
        let index = index(imports: [
            SemanticImport(line: 0, importedPath: "/sdk/Sdk.props", available: true),
            SemanticImport(line: 0, importedPath: "/sdk/Sdk.targets", available: true),
        ])

        let token = index.token(at: offset(of: "Microsoft.NET.Sdk"))
        XCTAssertEqual(token?.kind, .sdkReference)

        guard case .choose(let locations) = index.importNavigation(for: token!) else {
            return XCTFail("expected both SDK halves")
        }
        XCTAssertEqual(locations.compactMap(\.path), ["/sdk/Sdk.props", "/sdk/Sdk.targets"])
    }

    func testUnarchivedImportReportsWhyItCannotOpen() {
        let index = index(imports: [
            SemanticImport(line: 2, importedPath: "/repo/Shared.props", available: false)
        ])

        let token = index.token(at: offset(of: "Shared.props"))
        guard case .none(let reason) = index.importNavigation(for: token!) else {
            return XCTFail("expected no navigation")
        }
        XCTAssertTrue(reason.contains("Shared.props"), reason)
    }

    func testTokenLookupMissesBetweenTokens() {
        let index = index(imports: [])
        XCTAssertNil(index.token(at: offset(of: "PropertyGroup")))
    }

    func testSymbolKindPerTokenKind() {
        let index = index(imports: [])
        let property = index.token(at: offset(of: "BaseOut"))
        XCTAssertEqual(index.symbolKind(for: property!), .property)
    }

    func testNavigationPrefersDefinitionsOverExecutions() {
        let symbol = SemanticSymbol(
            kind: .target,
            name: "Build",
            found: true,
            definitions: [SemanticLocation(path: "/a.targets", line: 9, available: true)],
            executions: [SemanticLocation(nodeId: "42")])

        guard case .open(let location) = SourceSemanticIndex.navigation(for: symbol) else {
            return XCTFail("expected the definition")
        }
        XCTAssertEqual(location.line, 9)
    }

    func testNavigationFallsBackToExecutionsWhenNoDefinitionIsReachable() {
        let symbol = SemanticSymbol(
            kind: .target,
            name: "Build",
            found: true,
            definitions: [SemanticLocation(path: "/gone.targets", line: 9, available: false)],
            executions: [SemanticLocation(nodeId: "42"), SemanticLocation(nodeId: "43")])

        guard case .choose(let locations) = SourceSemanticIndex.navigation(for: symbol) else {
            return XCTFail("expected the executions")
        }
        XCTAssertEqual(locations.compactMap(\.nodeId), ["42", "43"])
    }

    func testTargetDefinitionPrefersItsExecutions() {
        // Cmd-clicking <Target Name="Build"> should go to where it ran —
        // jumping to the line under the pointer would be a no-op.
        let symbol = SemanticSymbol(
            kind: .target,
            name: "Build",
            found: true,
            definitions: [SemanticLocation(path: "/a.targets", line: 9, available: true)],
            executions: [SemanticLocation(label: "Build (20 ms)", nodeId: "42")])

        guard case .open(let location) = SourceSemanticIndex.navigation(for: symbol, preferring: .executions) else {
            return XCTFail("expected the execution")
        }
        XCTAssertEqual(location.nodeId, "42")
    }

    func testTargetThatNeverRanSaysSo() {
        let symbol = SemanticSymbol(
            kind: .target,
            name: "Publish",
            found: true,
            definitions: [],
            executions: [])

        guard case .none(let reason) = SourceSemanticIndex.navigation(for: symbol, preferring: .executions) else {
            return XCTFail("expected no navigation")
        }
        XCTAssertTrue(reason.contains("never ran"), reason)
    }

    func testUnknownSymbolExplainsItself() {
        let symbol = SemanticSymbol(kind: .property, name: "Nope", found: false)
        guard case .none(let reason) = SourceSemanticIndex.navigation(for: symbol) else {
            return XCTFail("expected no navigation")
        }
        XCTAssertTrue(reason.contains("Nope"), reason)
    }

    func testFoundSymbolWithNoLocationsSurfacesTheNote() {
        let symbol = SemanticSymbol(
            kind: .property,
            name: "OutputPath",
            found: true,
            value: "bin/",
            note: "No assignment locations were recorded")

        guard case .none(let reason) = SourceSemanticIndex.navigation(for: symbol) else {
            return XCTFail("expected no navigation")
        }
        XCTAssertEqual(reason, "No assignment locations were recorded")
    }
}
