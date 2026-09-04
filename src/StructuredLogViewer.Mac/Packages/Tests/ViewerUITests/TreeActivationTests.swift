import AppKit
import XCTest
import ViewerCore
@testable import ViewerUI

/// Space activates the selected row exactly as a double-click does, so a
/// node can be opened without leaving the keyboard.
@MainActor
final class TreeActivationTests: XCTestCase {
    private func makeTree() -> (TreeOutlineView, OutlineController) {
        let engine = SyntheticBinlogEngine(fanout: 8, depth: 3)
        let store = NodeStore(engine: engine, rootSummary: engine.summary(for: "0"))
        let controller = OutlineController(store: store)
        let outlineView = TreeOutlineView()
        outlineView.addTableColumn(NSTableColumn(identifier: NSUserInterfaceItemIdentifier("node")))
        outlineView.outlineTableColumn = outlineView.tableColumns[0]
        outlineView.controller = controller
        controller.attach(to: outlineView)
        return (outlineView, controller)
    }

    private func keyEvent(_ characters: String, modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifiers,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: false,
            keyCode: 49)!
    }

    func testSpaceActivatesTheSelectedRow() {
        let (outlineView, controller) = makeTree()
        var activated: [String] = []
        controller.onDoubleClick = { activated.append($0.summary.id) }

        outlineView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        let selected = controller.selectedNode
        XCTAssertNotNil(selected)

        outlineView.keyDown(with: keyEvent(" "))

        XCTAssertEqual(activated, [selected!.summary.id])
    }

    func testSpaceOnAnUnfetchedRowDoesNothing() {
        // Rows below the root are placeholders until their page arrives;
        // activating one would open a node that isn't there yet.
        let (outlineView, controller) = makeTree()
        var activated = 0
        controller.onDoubleClick = { _ in activated += 1 }

        outlineView.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
        XCTAssertEqual(controller.selectedNode?.isPlaceholder, true, "expected an unfetched row")

        outlineView.keyDown(with: keyEvent(" "))

        XCTAssertEqual(activated, 0)
    }

    func testSpaceWithNoSelectionDoesNothing() {
        let (outlineView, controller) = makeTree()
        var activated = 0
        controller.onDoubleClick = { _ in activated += 1 }

        outlineView.deselectAll(nil)
        outlineView.keyDown(with: keyEvent(" "))

        XCTAssertEqual(activated, 0)
    }

    func testCommandSpaceIsNotActivation() {
        // ⌘Space belongs to Spotlight; swallowing it would be rude.
        let (outlineView, controller) = makeTree()
        var activated = 0
        controller.onDoubleClick = { _ in activated += 1 }

        outlineView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        outlineView.keyDown(with: keyEvent(" ", modifiers: .command))

        XCTAssertEqual(activated, 0)
    }
}
