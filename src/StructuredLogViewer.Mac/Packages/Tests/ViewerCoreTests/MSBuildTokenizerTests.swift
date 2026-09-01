import XCTest
@testable import ViewerCore

final class MSBuildTokenizerTests: XCTestCase {
    private func tokens(_ text: String) -> [MSBuildToken] {
        MSBuildTokenizer.tokenize(text)
    }

    private func names(_ text: String, _ kind: MSBuildToken.Kind) -> [String] {
        tokens(text).filter { $0.kind == kind }.map(\.name)
    }

    /// The span must cover exactly the text the user sees underlined.
    private func slice(_ text: String, _ token: MSBuildToken) -> String {
        (text as NSString).substring(with: token.range)
    }

    func testPropertyAndItemReferences() {
        let text = "<PropertyGroup><Out>$(BaseOut)\\@(Thing)</Out></PropertyGroup>"
        XCTAssertEqual(names(text, .property), ["BaseOut"])
        XCTAssertEqual(names(text, .item), ["Thing"])
    }

    func testPropertyFunctionKeepsOnlyTheIdentifier() {
        // $(Foo.Substring(0,1)) is a call on the property Foo — underlining
        // the whole expression would claim a property named "Foo.Substring".
        let text = "<X>$(Foo.Substring(0,1))</X>"
        let token = tokens(text).first { $0.kind == .property }
        XCTAssertEqual(token?.name, "Foo")
        XCTAssertEqual(token.map { slice(text, $0) }, "Foo")
    }

    func testStaticPropertyFunctionIsNotAProperty() {
        XCTAssertEqual(names("<X>$([System.IO.Path]::GetFullPath('a'))</X>", .property), [])
    }

    func testSpansCoverExactlyTheName() {
        let text = "<X>$(Alpha)</X>"
        let token = tokens(text).first { $0.kind == .property }
        XCTAssertNotNil(token)
        XCTAssertEqual(slice(text, token!), "Alpha")
    }

    func testTargetDefinitionAndReferences() {
        let text = """
        <Target Name="Build" DependsOnTargets="Compile;Link">
        </Target>
        """
        XCTAssertEqual(names(text, .targetDefinition), ["Build"])
        XCTAssertEqual(names(text, .targetReference), ["Compile", "Link"])
    }

    func testTargetListEntriesAreTrimmedAndEmptiesDropped() {
        let text = #"<Target Name="A" AfterTargets=" One ; ; Two ">"#
        let references = tokens(text).filter { $0.kind == .targetReference }
        XCTAssertEqual(references.map(\.name), ["One", "Two"])
        for reference in references {
            XCTAssertEqual(slice(text, reference), reference.name)
        }
    }

    func testExpressionInTargetListIsAPropertyNotATarget() {
        // DependsOnTargets="$(BuildDependsOn);Link" depends on whatever the
        // property expands to — there is no target called "$(BuildDependsOn)".
        let text = #"<Target Name="A" DependsOnTargets="$(BuildDependsOn);Link">"#
        XCTAssertEqual(names(text, .property), ["BuildDependsOn"])
        XCTAssertEqual(names(text, .targetReference), ["Link"])
    }

    func testTargetNameThatIsAnExpressionIsNotADefinition() {
        let text = #"<Target Name="$(Generated)">"#
        XCTAssertEqual(names(text, .targetDefinition), [])
        XCTAssertEqual(names(text, .property), ["Generated"])
    }

    func testImportAndSdkAttributesAreDistinguished() {
        let text = """
        <Project Sdk="Microsoft.NET.Sdk">
          <Import Project="$(Dir)\\Shared.props" />
          <Import Sdk="My.Sdk" Project="Sdk.props" />
        </Project>
        """
        XCTAssertEqual(names(text, .importPath), ["$(Dir)\\Shared.props", "Sdk.props"])
        XCTAssertEqual(names(text, .sdkReference), ["Microsoft.NET.Sdk", "My.Sdk"])

        // The whole attribute is one link; the `$(Dir)` inside it does not
        // also become a property token, which would overlap the import span.
        XCTAssertEqual(names(text, .property), [])
    }

    func testTokensNeverOverlap() {
        // Lookup is a binary search over start offsets, so overlapping spans
        // would make the token under the cursor ambiguous.
        let text = """
        <Project Sdk="$(TheSdk)" DefaultTargets="$(First);Second">
          <Import Project="$(Dir)\\$(Name).props" Condition="'$(X)' != ''" />
          <Target Name="T" DependsOnTargets="$(A);B;$(C)">$(Body)@(Items)</Target>
        </Project>
        """
        var previousEnd = 0
        for token in tokens(text) {
            XCTAssertGreaterThanOrEqual(token.range.location, previousEnd, "overlap at \(token)")
            XCTAssertGreaterThan(token.range.length, 0, "empty span at \(token)")
            previousEnd = NSMaxRange(token.range)
        }
        XCTAssertLessThanOrEqual(previousEnd, (text as NSString).length)
    }

    func testCommentsAreSkipped() {
        let text = """
        <!-- <Target Name="Ghost" /> $(NotReal) -->
        <Target Name="Real" />
        """
        XCTAssertEqual(names(text, .targetDefinition), ["Real"])
        XCTAssertEqual(names(text, .property), [])
    }

    func testLineNumbersAreOneBased() {
        let text = "<Project>\n  <Import Project=\"a.props\" />\n</Project>"
        let token = tokens(text).first { $0.kind == .importPath }
        XCTAssertEqual(token?.line, 2)
    }

    func testTokensAreSortedByOffset() {
        let text = """
        <Project Sdk="A">
          <PropertyGroup><P>$(One)$(Two)</P></PropertyGroup>
          <Target Name="T" DependsOnTargets="X;Y" />
        </Project>
        """
        let offsets = tokens(text).map(\.range.location)
        XCTAssertEqual(offsets, offsets.sorted())
    }

    func testMalformedMarkupTerminates() {
        // Unclosed tags and stray delimiters must not spin the scanner.
        for text in ["<Target Name=", "<Import Project='unterminated", "<<<>>>", "<!-- unterminated", "<a b=\"", "$("] {
            _ = tokens(text)
        }
    }

    func testEmptyAndPlainText() {
        XCTAssertTrue(tokens("").isEmpty)
        XCTAssertTrue(tokens("just some words").isEmpty)
    }
}
