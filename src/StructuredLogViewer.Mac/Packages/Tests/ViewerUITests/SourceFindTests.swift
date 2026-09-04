import AppKit
import XCTest
import ViewerCore
@testable import ViewerUI

/// ⌘F has to reach the source editor from wherever focus happens to be —
/// usually the build tree, which is not in the editor's responder chain.
@MainActor
final class SourceFindTests: XCTestCase {
    /// A window shaped like the real one: the editor nested a few levels
    /// down, with a sibling branch that has no editor in it.
    private func makeWindow(withEditor: Bool = true) -> (NSWindow, NSScrollView?, SemanticTextView?) {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 600),
            styleMask: [.titled],
            backing: .buffered,
            defer: false)
        let root = NSView(frame: window.contentLayoutRect)
        window.contentView = root

        // The "sidebar" branch: focusable, and nowhere near the editor.
        let sidebar = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 600))
        sidebar.addSubview(NSTextField(labelWithString: "tree"))
        root.addSubview(sidebar)

        guard withEditor else { return (window, nil, nil) }

        let storage = NSTextStorage(string: "<Project>\n  <Import Project=\"A.props\" />\n</Project>")
        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 560, height: 600))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)

        let editor = SemanticTextView(
            frame: NSRect(x: 0, y: 0, width: 560, height: 600), textContainer: container)
        editor.isEditable = false
        editor.isSelectable = true
        editor.usesFindBar = true
        editor.isIncrementalSearchingEnabled = true

        let scrollView = NSScrollView(frame: NSRect(x: 200, y: 0, width: 600, height: 600))
        scrollView.documentView = editor

        // Nested, so a shallow search wouldn't find it.
        let well = NSView(frame: scrollView.frame)
        well.addSubview(scrollView)
        root.addSubview(well)

        return (window, scrollView, editor)
    }

    func testEditorIsFoundAnywhereInTheWindow() {
        let (window, _, editor) = makeWindow()
        XCTAssertIdentical(SourceFind.editor(in: window), editor)
    }

    func testWindowWithNoEditorReportsNone() {
        let (window, _, _) = makeWindow(withEditor: false)
        XCTAssertNil(SourceFind.editor(in: window))
        XCTAssertFalse(SourceFind.perform(.showFindInterface, in: window))
    }

    func testFindOpensTheFindBar() {
        let (window, scrollView, _) = makeWindow()
        XCTAssertEqual(scrollView?.isFindBarVisible, false)

        XCTAssertTrue(SourceFind.perform(.showFindInterface, in: window))

        XCTAssertEqual(scrollView?.isFindBarVisible, true)
    }

    func testFindWorksWhileFocusIsElsewhere() {
        // The case that motivated this: you clicked a node in the tree to
        // open its source, so focus is still in the tree.
        let (window, scrollView, editor) = makeWindow()
        let sidebarField = window.contentView!.subviews[0].subviews[0]
        window.makeFirstResponder(sidebarField)
        XCTAssertNotIdentical(window.firstResponder, editor)

        XCTAssertTrue(SourceFind.perform(.showFindInterface, in: window))

        XCTAssertEqual(scrollView?.isFindBarVisible, true)
    }

    func testHidingTheFindBarClosesIt() {
        let (window, scrollView, _) = makeWindow()
        SourceFind.perform(.showFindInterface, in: window)
        XCTAssertEqual(scrollView?.isFindBarVisible, true)

        SourceFind.perform(.hideFindInterface, in: window)

        XCTAssertEqual(scrollView?.isFindBarVisible, false)
    }

    func testUseSelectionForFindSeedsTheSearchFromTheDocument() {
        let (window, _, editor) = makeWindow()
        let range = (editor!.string as NSString).range(of: "A.props")
        editor?.setSelectedRange(range)

        XCTAssertTrue(SourceFind.perform(.setSearchString, in: window))

        // NSTextFinder puts the selection on the find pasteboard.
        let pasteboard = NSPasteboard(name: .find)
        XCTAssertEqual(pasteboard.string(forType: .string), "A.props")
    }
}
