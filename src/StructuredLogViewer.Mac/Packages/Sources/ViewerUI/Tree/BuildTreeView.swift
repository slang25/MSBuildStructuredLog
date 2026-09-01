import AppKit
import SwiftUI
import ViewerCore

/// Context-menu / action surface for tree rows, wired by the main window
/// to the session's controllers.
@MainActor
public struct NodeMenuActions {
    public var isFavorite: (NodeSummary) -> Bool = { _ in false }
    public var toggleFavorite: ((NodeSummary) -> Void)?
    public var viewSource: ((NodeSummary) -> Void)?
    public var preprocess: ((NodeSummary) -> Void)?
    public var copySubtree: ((NodeSummary) -> Void)?
    public var searchInSubtree: ((NodeSummary) -> Void)?
    public var revealInTree: ((NodeSummary) -> Void)?
    public var sortChildren: ((NodeRef, ChildSortMode) -> Void)?

    public init() {}

    func menu(for node: NodeRef) -> NSMenu {
        let summary = node.summary
        let menu = NSMenu()

        menu.addItem(makeItem("Copy", key: "c") {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(summary.title, forType: .string)
        })

        if let copySubtree {
            menu.addItem(makeItem("Copy Subtree") { copySubtree(summary) })
        }

        if summary.hasSource, let viewSource {
            menu.addItem(.separator())
            menu.addItem(makeItem("View Source") { viewSource(summary) })
        }

        if summary.canPreprocess, let preprocess {
            menu.addItem(makeItem("Preprocess") { preprocess(summary) })
        }

        // The search DSL addresses nodes as $<index>, which only exists for
        // timed nodes (plain numeric ids, no '/' path form).
        if let searchInSubtree, summary.hasChildren, !summary.id.contains("/") {
            menu.addItem(.separator())
            menu.addItem(makeItem("Search in Subtree") { searchInSubtree(summary) })
        }

        if let revealInTree {
            menu.addItem(makeItem("Reveal in Build Tree") { revealInTree(summary) })
        }

        if let toggleFavorite {
            menu.addItem(.separator())
            let title = isFavorite(summary) ? "Remove from Favorites" : "Add to Favorites"
            menu.addItem(makeItem(title) { toggleFavorite(summary) })
        }

        if let sortChildren, summary.hasChildren, summary.childCount > 1 {
            menu.addItem(.separator())
            let sortMenu = NSMenu()
            let currentMode = node.sortMode
            for (title, mode) in [("Natural Order", ChildSortMode.natural), ("By Name", .byName), ("By Duration", .byDuration)] {
                let item = makeItem(title) { sortChildren(node, mode) }
                item.state = currentMode == mode ? .on : .off
                sortMenu.addItem(item)
            }
            let sortItem = NSMenuItem(title: "Sort Children", action: nil, keyEquivalent: "")
            sortItem.submenu = sortMenu
            menu.addItem(sortItem)
        }

        return menu
    }

    private func makeItem(_ title: String, key: String = "", action: @escaping () -> Void) -> NSMenuItem {
        let item = ClosureMenuItem(title: title, keyEquivalent: key, handler: action)
        return item
    }
}

final class ClosureMenuItem: NSMenuItem {
    private let handler: () -> Void

    init(title: String, keyEquivalent: String, handler: @escaping () -> Void) {
        self.handler = handler
        super.init(title: title, action: #selector(invoke), keyEquivalent: keyEquivalent)
        target = self
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    @objc private func invoke() {
        handler()
    }
}

/// NSOutlineView subclass that right-click-selects the clicked row and
/// asks the controller's menu actions for a context menu.
final class TreeOutlineView: NSOutlineView {
    weak var controller: OutlineController?

    override func menu(for event: NSEvent) -> NSMenu? {
        let point = convert(event.locationInWindow, from: nil)
        let row = self.row(at: point)
        guard row >= 0, let node = item(atRow: row) as? NodeRef, !node.isPlaceholder else {
            return nil
        }

        if !selectedRowIndexes.contains(row) {
            selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }

        return controller?.menuActions?.menu(for: node)
    }

    override func keyDown(with event: NSEvent) {
        // ⌘C copies the selected row's text.
        if event.modifierFlags.contains(.command),
           event.charactersIgnoringModifiers == "c",
           let node = controller?.selectedNode,
           !node.isPlaceholder {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(node.summary.title, forType: .string)
            return
        }

        super.keyDown(with: event)
    }
}

/// The main build tree (and the Files pane tree): an NSOutlineView over a
/// NodeStore. SwiftUI `List(children:)` is disqualified at this scale —
/// it materializes child arrays and has no reveal/scroll-to or
/// type-select.
public struct BuildTreeView: NSViewRepresentable {
    private let store: NodeStore
    private let revealRequest: BuildSession.RevealRequest?
    private let menuActions: NodeMenuActions?
    private let onSelect: (NodeSummary?) -> Void
    private let onDoubleClick: (NodeSummary) -> Void

    public init(
        store: NodeStore,
        revealRequest: BuildSession.RevealRequest? = nil,
        menuActions: NodeMenuActions? = nil,
        onSelect: @escaping (NodeSummary?) -> Void = { _ in },
        onDoubleClick: @escaping (NodeSummary) -> Void = { _ in }
    ) {
        self.store = store
        self.revealRequest = revealRequest
        self.menuActions = menuActions
        self.onSelect = onSelect
        self.onDoubleClick = onDoubleClick
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(store: store)
    }

    public func makeNSView(context: Context) -> NSScrollView {
        let outlineView = TreeOutlineView()
        outlineView.headerView = nil
        outlineView.rowSizeStyle = .custom
        outlineView.style = .plain
        outlineView.usesAlternatingRowBackgroundColors = false
        outlineView.allowsMultipleSelection = false
        outlineView.autoresizesOutlineColumn = false
        outlineView.indentationPerLevel = NodeStyling.indentPerLevel

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("node"))
        column.resizingMask = .autoresizingMask
        outlineView.addTableColumn(column)
        outlineView.outlineTableColumn = column

        let controller = context.coordinator.controller
        controller.menuActions = menuActions
        controller.onSelect = { [onSelect] node in
            onSelect(node?.isPlaceholder == false ? node?.summary : nil)
        }
        controller.onDoubleClick = { [onDoubleClick] node in
            onDoubleClick(node.summary)
        }
        outlineView.controller = controller
        controller.attach(to: outlineView)

        let scrollView = NSScrollView()
        scrollView.documentView = outlineView
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        return scrollView
    }

    public func updateNSView(_ nsView: NSScrollView, context: Context) {
        let controller = context.coordinator.controller
        controller.menuActions = menuActions
        controller.onSelect = { [onSelect] node in
            onSelect(node?.isPlaceholder == false ? node?.summary : nil)
        }
        controller.onDoubleClick = { [onDoubleClick] node in
            onDoubleClick(node.summary)
        }

        if let request = revealRequest, request.token != context.coordinator.lastRevealToken {
            context.coordinator.lastRevealToken = request.token
            controller.reveal(id: request.nodeId)
        }
    }

    @MainActor
    public final class Coordinator {
        let controller: OutlineController
        var lastRevealToken: UUID?

        init(store: NodeStore) {
            self.controller = OutlineController(store: store)
        }
    }
}
