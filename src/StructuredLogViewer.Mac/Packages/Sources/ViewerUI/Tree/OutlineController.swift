import AppKit
import ViewerCore

/// NSOutlineView data source/delegate over a NodeStore. The pull-based
/// data source maps 1:1 onto the store: child counts answered
/// synchronously from summaries, unfetched rows render dimmed
/// placeholders and reload in place when their page arrives.
@MainActor
public final class OutlineController: NSObject {
    public let store: NodeStore
    public weak var outlineView: NSOutlineView?

    public var onSelect: ((NodeRef?) -> Void)?
    public var onDoubleClick: ((NodeRef) -> Void)?
    public var menuActions: NodeMenuActions?

    private static let cellIdentifier = NSUserInterfaceItemIdentifier("NodeCell")

    public init(store: NodeStore) {
        self.store = store
        super.init()

        store.onChildrenUpdated = { [weak self] parent, range in
            self?.childrenUpdated(parent: parent, range: range)
        }
    }

    public func attach(to outlineView: NSOutlineView) {
        self.outlineView = outlineView
        outlineView.dataSource = self
        outlineView.delegate = self
        outlineView.target = self
        outlineView.doubleAction = #selector(handleDoubleClick(_:))
        outlineView.reloadData()
        outlineView.expandItem(store.root)
    }

    /// Expands down to and selects the node with the given engine id.
    public func reveal(id: String) {
        Task { [weak self] in
            guard let self else { return }
            do {
                let chain = try await self.store.reveal(id: id)
                guard let outlineView = self.outlineView else { return }

                for ref in chain.dropLast() {
                    outlineView.expandItem(ref)
                    // Expanding may enqueue page fetches for the next level;
                    // the chain refs themselves are already loaded.
                }

                guard let target = chain.last else { return }
                let row = outlineView.row(forItem: target)
                if row >= 0 {
                    outlineView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
                    outlineView.scrollRowToVisible(row)
                }
            } catch {
                NSSound.beep()
            }
        }
    }

    public var selectedNode: NodeRef? {
        guard let outlineView, outlineView.selectedRow >= 0 else { return nil }
        return outlineView.item(atRow: outlineView.selectedRow) as? NodeRef
    }

    private func childrenUpdated(parent: NodeRef, range: Range<Int>) {
        guard let outlineView else { return }

        // If the parent's slots were rebuilt wholesale (sort change),
        // reload its whole subtree.
        if range.lowerBound == 0 && range.upperBound == parent.childCount && range.count > store.pageSize {
            outlineView.reloadItem(parent, reloadChildren: true)
            return
        }

        for index in range {
            guard index < parent.childSlotsCount, let child = childIfMaterialized(parent, index) else { continue }
            let row = outlineView.row(forItem: child)
            if row >= 0 {
                outlineView.reloadItem(child, reloadChildren: false)
            }
        }
    }

    private func childIfMaterialized(_ parent: NodeRef, _ index: Int) -> NodeRef? {
        parent.materializedChild(at: index)
    }

    @objc private func handleDoubleClick(_ sender: Any?) {
        guard let node = selectedNode, !node.isPlaceholder else { return }
        onDoubleClick?(node)
    }
}

extension OutlineController: NSOutlineViewDataSource {
    public func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        guard let node = item as? NodeRef else { return 1 }
        return store.childCount(of: node)
    }

    public func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let node = item as? NodeRef else { return false }
        return node.hasChildren
    }

    public func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        guard let node = item as? NodeRef else { return store.root }
        return store.child(of: node, at: index)
    }
}

extension OutlineController: NSOutlineViewDelegate {
    public func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? NodeRef else { return nil }

        let cell: NSTableCellView
        if let reused = outlineView.makeView(withIdentifier: Self.cellIdentifier, owner: self) as? NSTableCellView {
            cell = reused
        } else {
            cell = NodeCellView.make(identifier: Self.cellIdentifier)
        }

        if node.isPlaceholder {
            cell.imageView?.image = nil
            cell.textField?.attributedStringValue = NSAttributedString(
                string: "…",
                attributes: [
                    .font: NodeStyling.rowFont,
                    .foregroundColor: NSColor.quaternaryLabelColor,
                ])
            return cell
        }

        let summary = node.summary
        let style = NodeStyling.style(for: summary)
        // Medium weight keeps outline-style glyphs (bubbles, tags, clocks)
        // legible at this size, especially in dark mode.
        let symbolConfiguration = NSImage.SymbolConfiguration(pointSize: NodeStyling.rowFontSize, weight: .medium)
            .applying(.init(paletteColors: [NodeStyling.stateAccent(for: summary) ?? style.color]))
        let image = NSImage(systemSymbolName: style.symbolName, accessibilityDescription: summary.kind)?
            .withSymbolConfiguration(symbolConfiguration)
        cell.imageView?.image = image
        cell.textField?.attributedStringValue = NodeStyling.rowText(for: summary)
        cell.textField?.lineBreakMode = .byTruncatingTail
        cell.toolTip = summary.title.count > 120 || summary.title.contains("\n") ? summary.title : nil
        return cell
    }

    public func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        NodeStyling.rowHeight
    }

    public func outlineViewSelectionDidChange(_ notification: Notification) {
        onSelect?(selectedNode)
    }

    public func outlineView(_ outlineView: NSOutlineView, typeSelectStringFor tableColumn: NSTableColumn?, item: Any) -> String? {
        guard let node = item as? NodeRef, !node.isPlaceholder else { return nil }
        return node.summary.title
    }
}

enum NodeCellView {
    static func make(identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
        let cell = NSTableCellView()
        cell.identifier = identifier

        let imageView = NSImageView()
        imageView.translatesAutoresizingMaskIntoConstraints = false
        imageView.setContentHuggingPriority(.required, for: .horizontal)
        cell.imageView = imageView
        cell.addSubview(imageView)

        let textField = NSTextField(labelWithString: "")
        textField.translatesAutoresizingMaskIntoConstraints = false
        textField.lineBreakMode = .byTruncatingTail
        textField.maximumNumberOfLines = 1
        textField.cell?.truncatesLastVisibleLine = true
        textField.allowsDefaultTighteningForTruncation = false
        cell.textField = textField
        cell.addSubview(textField)

        NSLayoutConstraint.activate([
            imageView.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            imageView.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            imageView.widthAnchor.constraint(equalToConstant: NodeStyling.iconSize),
            imageView.heightAnchor.constraint(equalToConstant: NodeStyling.iconSize),
            textField.leadingAnchor.constraint(equalTo: imageView.trailingAnchor, constant: 4),
            textField.trailingAnchor.constraint(lessThanOrEqualTo: cell.trailingAnchor, constant: -2),
            textField.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])

        return cell
    }
}
