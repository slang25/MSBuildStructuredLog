import XCTest
@testable import ViewerCore

@MainActor
final class SourceControllerPresentTests: XCTestCase {
    func testPresentAsksTheUIToRevealTheDocumentWell() {
        let controller = SourceController()
        controller.open(kind: .generated, title: "t", path: "p", content: "x", line: nil)
        let before = controller.presentationToken

        controller.present()

        XCTAssertEqual(controller.presentationToken, before + 1)
    }

    func testPresentDoesNothingWithNoOpenTab() {
        // Nothing to reveal; bumping the token would pop an empty inspector.
        let controller = SourceController()
        let before = controller.presentationToken

        controller.present()

        XCTAssertEqual(controller.presentationToken, before)
    }

    func testPresentKeepsTheSelectedTab() {
        let controller = SourceController()
        controller.open(kind: .generated, title: "a", path: "a", content: "x", line: nil)
        controller.open(kind: .generated, title: "b", path: "b", content: "y", line: nil)
        let selected = controller.selectedTabId

        controller.present()

        XCTAssertEqual(controller.selectedTabId, selected)
    }
}
