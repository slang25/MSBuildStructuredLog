import SwiftUI
import ViewerCore

/// Renders a materialized search-result tree (SearchResponse.roots) with
/// highlight spans. Rows are flattened with manual disclosure state so
/// results start fully expanded (List(children:) starts collapsed).
public struct ResultTreeView: View {
    public struct Row: Identifiable {
        public let id: String
        public let depth: Int
        public let node: SearchTreeNode
        public let hasChildren: Bool
    }

    private let roots: [SearchTreeNode]
    private let onSelect: (NodeSummary) -> Void
    @State private var collapsed: Set<String> = []
    @State private var selectedRowId: String?

    public init(roots: [SearchTreeNode], onSelect: @escaping (NodeSummary) -> Void) {
        self.roots = roots
        self.onSelect = onSelect
    }

    private var rows: [Row] {
        var result: [Row] = []
        func walk(_ node: SearchTreeNode, path: String, depth: Int) {
            let children = node.children ?? []
            result.append(Row(id: path, depth: depth, node: node, hasChildren: !children.isEmpty))
            guard !collapsed.contains(path) else { return }
            for (index, child) in children.enumerated() {
                walk(child, path: "\(path)/\(index)", depth: depth + 1)
            }
        }

        for (index, root) in roots.enumerated() {
            walk(root, path: "\(index)", depth: 0)
        }

        return result
    }

    public var body: some View {
        List(rows, selection: $selectedRowId) { row in
            ResultRowView(row: row, isCollapsed: collapsed.contains(row.id)) {
                toggle(row.id)
            }
            .listRowInsets(EdgeInsets(top: 1, leading: 4, bottom: 1, trailing: 4))
            .contentShape(Rectangle())
            .onTapGesture {
                selectedRowId = row.id
                if let summary = row.node.node {
                    onSelect(summary)
                } else if row.hasChildren {
                    toggle(row.id)
                }
            }
            .tag(row.id)
        }
        .listStyle(.sidebar)
        .environment(\.defaultMinListRowHeight, 18)
    }

    private func toggle(_ id: String) {
        if collapsed.contains(id) {
            collapsed.remove(id)
        } else {
            collapsed.insert(id)
        }
    }
}

struct ResultRowView: View {
    let row: ResultTreeView.Row
    let isCollapsed: Bool
    let onToggle: () -> Void

    var body: some View {
        HStack(spacing: 3) {
            Color.clear.frame(width: CGFloat(row.depth) * 12, height: 1)

            if row.hasChildren {
                Button(action: onToggle) {
                    Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                        .font(.system(size: 8, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 10)
                }
                .buttonStyle(.plain)
            } else {
                Color.clear.frame(width: 10, height: 1)
            }

            if let summary = row.node.node {
                let style = NodeStyling.style(for: summary)
                Image(systemName: style.symbolName)
                    .font(.system(size: 10))
                    .foregroundStyle(Color(nsColor: style.color))
                    .frame(width: 13)
            } else {
                Image(systemName: "folder")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                    .frame(width: 13)
            }

            text
                .lineLimit(1)
                .truncationMode(.tail)

            Spacer(minLength: 0)
        }
        .help(row.node.node?.title ?? row.node.text ?? "")
    }

    private var text: Text {
        if let highlights = row.node.highlights, !highlights.isEmpty {
            var combined = Text("")
            for span in highlights {
                var piece = Text(span.text)
                if span.style == "time" {
                    piece = piece.font(.caption.monospacedDigit()).foregroundColor(.secondary)
                } else if span.isHighlight == true {
                    piece = piece.font(.caption.bold()).foregroundColor(.accentColor)
                } else {
                    piece = piece.font(.caption)
                }
                combined = combined + piece
            }
            return combined
        }

        let fallback = row.node.node?.title ?? row.node.text ?? ""
        return Text(fallback).font(.caption)
    }
}
