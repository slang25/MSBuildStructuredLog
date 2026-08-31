import AppKit
import SwiftUI
import ViewerCore

/// The Project References view (the WPF viewer's tab of the same name):
/// projects laid out in layers by dependency height — top-level projects
/// at the top, leaf dependencies at the bottom. Like the WPF control,
/// edges draw on selection: blue lines to the projects the selection
/// references, orange lines from the projects referencing it.
/// Double-click jumps to a `$projectreference project(...)` search.
struct ProjectGraphHostView: View {
    let session: BuildSession
    let onSearch: (String) -> Void

    @State private var graph: ProjectGraph?
    @State private var errorMessage: String?
    @State private var selection: Int?

    var body: some View {
        Group {
            if let graph {
                if graph.vertices.isEmpty {
                    ContentUnavailableView(
                        "No Project References",
                        systemImage: "point.3.connected.trianglepath.dotted",
                        description: Text("This build has no project-to-project references."))
                } else {
                    VStack(spacing: 0) {
                        ProjectGraphScrollView(
                            graph: graph,
                            selection: $selection,
                            onDoubleClick: { vertex in
                                onSearch("$projectreference project(\(vertex.title))")
                            })
                        Divider()
                        statusBar(graph)
                    }
                }
            } else if let errorMessage {
                ContentUnavailableView(errorMessage, systemImage: "exclamationmark.triangle")
            } else {
                ProgressView("Building project graph…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            guard graph == nil, let engine = session.engine else { return }
            do {
                graph = try await engine.projectGraph()
            } catch {
                errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    private func statusBar(_ graph: ProjectGraph) -> some View {
        HStack(spacing: 10) {
            Text("\(graph.vertices.count) project\(graph.vertices.count == 1 ? "" : "s")")
                .foregroundStyle(.secondary)

            if let selection, graph.vertices.indices.contains(selection) {
                let vertex = graph.vertices[selection]
                let incoming = ProjectGraphModel.incomingCounts(of: graph)[selection]
                Text("\(vertex.title): references \(vertex.outgoing?.count ?? 0), referenced by \(incoming)")
                    .lineLimit(1)
            } else {
                Text("Click a project to see its references; double-click to search for them.")
                    .foregroundStyle(.tertiary)
            }

            Spacer()

            HStack(spacing: 8) {
                edgeLegend(.systemBlue, "references")
                edgeLegend(.systemOrange, "referenced by")
            }
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
    }

    private func edgeLegend(_ color: NSColor, _ label: String) -> some View {
        HStack(spacing: 3) {
            Rectangle()
                .fill(Color(nsColor: color))
                .frame(width: 12, height: 2)
            Text(label)
                .foregroundStyle(.secondary)
        }
    }
}

/// Precomputed geometry: chips per layer (layer = maxHeight − height, so
/// sources sit in the top row) with measured widths.
enum ProjectGraphModel {
    static let chipFont = NSFont.systemFont(ofSize: 12, weight: .medium)
    static let chipHeight: CGFloat = 26
    static let chipPaddingX: CGFloat = 10
    static let chipGap: CGFloat = 12
    static let layerGap: CGFloat = 56
    static let margin: CGFloat = 24

    struct Chip {
        let index: Int
        let vertex: ProjectGraphVertex
        var frame: NSRect
    }

    struct Layout {
        let chips: [Chip]
        let size: NSSize
        /// chips indexed by vertex index for edge geometry.
        let byVertexIndex: [Int: Int]
    }

    static func incomingCounts(of graph: ProjectGraph) -> [Int] {
        var counts = Array(repeating: 0, count: graph.vertices.count)
        for vertex in graph.vertices {
            for target in vertex.outgoing ?? [] where counts.indices.contains(target) {
                counts[target] += 1
            }
        }
        return counts
    }

    static func layout(_ graph: ProjectGraph, minWidth: CGFloat) -> Layout {
        let maxHeight = graph.vertices.map(\.height).max() ?? 0

        var layers: [[Int]] = Array(repeating: [], count: maxHeight + 1)
        for (index, vertex) in graph.vertices.enumerated() {
            let layer = maxHeight - vertex.height
            layers[layer].append(index)
        }

        // Measure chips and compute layer widths.
        let attributes: [NSAttributedString.Key: Any] = [.font: chipFont]
        let widths: [CGFloat] = graph.vertices.map { vertex in
            let textWidth = (vertex.title as NSString).size(withAttributes: attributes).width
            return ceil(textWidth) + chipPaddingX * 2
        }

        var layerWidths: [CGFloat] = []
        for layer in layers {
            let width = layer.reduce(0) { $0 + widths[$1] } + CGFloat(max(layer.count - 1, 0)) * chipGap
            layerWidths.append(width)
        }

        let contentWidth = max((layerWidths.max() ?? 0) + margin * 2, minWidth)

        var chips: [Chip] = []
        var byVertexIndex: [Int: Int] = [:]
        var y = margin
        for (layerIndex, layer) in layers.enumerated() {
            var x = (contentWidth - layerWidths[layerIndex]) / 2
            for vertexIndex in layer {
                let frame = NSRect(x: x, y: y, width: widths[vertexIndex], height: chipHeight)
                byVertexIndex[vertexIndex] = chips.count
                chips.append(Chip(index: vertexIndex, vertex: graph.vertices[vertexIndex], frame: frame))
                x += widths[vertexIndex] + chipGap
            }
            y += chipHeight + layerGap
        }

        let contentHeight = y - layerGap + margin
        return Layout(chips: chips, size: NSSize(width: contentWidth, height: max(contentHeight, 200)), byVertexIndex: byVertexIndex)
    }
}

struct ProjectGraphScrollView: NSViewRepresentable {
    let graph: ProjectGraph
    @Binding var selection: Int?
    let onDoubleClick: (ProjectGraphVertex) -> Void

    func makeNSView(context: Context) -> NSScrollView {
        let content = ProjectGraphContentView()
        content.onSelect = { index in
            DispatchQueue.main.async { selection = index }
        }
        content.onDoubleClick = onDoubleClick

        let scrollView = LayoutReportingScrollView()
        scrollView.documentView = content
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.backgroundColor = .textBackgroundColor
        scrollView.onLayout = { [weak scrollView, weak content] in
            guard let scrollView, let content else { return }
            content.relayout(minWidth: scrollView.contentSize.width)
        }
        context.coordinator.contentView = content
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let content = context.coordinator.contentView else { return }
        content.onDoubleClick = onDoubleClick
        content.setGraph(graph, minWidth: scrollView.contentSize.width)
        content.selectedIndex = selection
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        weak var contentView: ProjectGraphContentView?
    }
}

final class ProjectGraphContentView: NSView {
    private var graph: ProjectGraph?
    private var layout: ProjectGraphModel.Layout?
    private var lastMinWidth: CGFloat = 0
    private var pendingMinWidth: CGFloat?
    private var relayoutScheduled = false

    var onSelect: ((Int?) -> Void)?
    var onDoubleClick: ((ProjectGraphVertex) -> Void)?

    var selectedIndex: Int? {
        didSet {
            if selectedIndex != oldValue {
                needsDisplay = true
            }
        }
    }

    override var isFlipped: Bool { true }

    func setGraph(_ graph: ProjectGraph, minWidth: CGFloat) {
        if self.graph == nil {
            self.graph = graph
            relayout(minWidth: minWidth)
        }
    }

    /// Recomputes chip layout and resizes the view — never synchronously:
    /// callers can be inside an AppKit layout/constraint pass (split
    /// divider drags), where resizing re-enters layout and crashes.
    func relayout(minWidth: CGFloat) {
        pendingMinWidth = minWidth
        guard !relayoutScheduled else { return }
        relayoutScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.relayoutScheduled = false
            self.applyRelayout()
        }
    }

    private func applyRelayout() {
        guard let graph, let minWidth = pendingMinWidth, minWidth > 50 else { return }

        // Mid-divider-drag widths churn every event; the scroll view
        // reports layout again when the live resize ends.
        if let scrollView = enclosingScrollView, scrollView.inLiveResize {
            return
        }

        guard abs(minWidth - lastMinWidth) > 1 else { return }
        pendingMinWidth = nil
        lastMinWidth = minWidth
        layout = ProjectGraphModel.layout(graph, minWidth: minWidth)
        if let layout {
            setFrameSize(layout.size)
        }
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let layout, let graph, let context = NSGraphicsContext.current?.cgContext else { return }

        // Edges first (under the chips), only for the selection.
        if let selectedIndex,
           let selectedChipIndex = layout.byVertexIndex[selectedIndex] {
            let selectedChip = layout.chips[selectedChipIndex]

            context.setLineWidth(1.5)

            // Outgoing: bottom of selection → top of referenced project.
            if let outgoing = graph.vertices[selectedIndex].outgoing {
                context.setStrokeColor(NSColor.systemBlue.cgColor)
                for target in outgoing {
                    guard let targetChipIndex = layout.byVertexIndex[target] else { continue }
                    let targetFrame = layout.chips[targetChipIndex].frame
                    context.move(to: CGPoint(x: selectedChip.frame.midX, y: selectedChip.frame.maxY))
                    context.addLine(to: CGPoint(x: targetFrame.midX, y: targetFrame.minY))
                }
                context.strokePath()
            }

            // Incoming: bottom of referencing project → top of selection.
            context.setStrokeColor(NSColor.systemOrange.cgColor)
            for (index, vertex) in graph.vertices.enumerated() {
                guard vertex.outgoing?.contains(selectedIndex) == true,
                      let sourceChipIndex = layout.byVertexIndex[index] else { continue }
                let sourceFrame = layout.chips[sourceChipIndex].frame
                context.move(to: CGPoint(x: sourceFrame.midX, y: sourceFrame.maxY))
                context.addLine(to: CGPoint(x: selectedChip.frame.midX, y: selectedChip.frame.minY))
            }
            context.strokePath()
        }

        let neighborIndices = neighborsOfSelection()

        for chip in layout.chips {
            guard chip.frame.insetBy(dx: -2, dy: -2).intersects(dirtyRect) else { continue }

            let isSelected = chip.index == selectedIndex
            let isNeighbor = neighborIndices.contains(chip.index)

            let path = CGPath(
                roundedRect: chip.frame,
                cornerWidth: 6, cornerHeight: 6, transform: nil)

            let fill: NSColor = isSelected
                ? NSColor.controlAccentColor.withAlphaComponent(0.28)
                : (isNeighbor
                    ? NSColor.controlAccentColor.withAlphaComponent(0.12)
                    : NSColor.quaternarySystemFill)
            context.setFillColor(fill.cgColor)
            context.addPath(path)
            context.fillPath()

            let border: NSColor = isSelected
                ? .controlAccentColor
                : (isNeighbor ? NSColor.controlAccentColor.withAlphaComponent(0.6) : .separatorColor)
            context.setStrokeColor(border.cgColor)
            context.setLineWidth(isSelected ? 1.5 : 1)
            context.addPath(path)
            context.strokePath()

            let attributes: [NSAttributedString.Key: Any] = [
                .font: ProjectGraphModel.chipFont,
                .foregroundColor: NSColor.labelColor,
            ]
            let textSize = (chip.vertex.title as NSString).size(withAttributes: attributes)
            (chip.vertex.title as NSString).draw(
                at: NSPoint(
                    x: chip.frame.midX - textSize.width / 2,
                    y: chip.frame.midY - textSize.height / 2),
                withAttributes: attributes)
        }
    }

    private func neighborsOfSelection() -> Set<Int> {
        guard let graph, let selectedIndex else { return [] }
        var neighbors = Set(graph.vertices[selectedIndex].outgoing ?? [])
        for (index, vertex) in graph.vertices.enumerated() {
            if vertex.outgoing?.contains(selectedIndex) == true {
                neighbors.insert(index)
            }
        }
        return neighbors
    }

    private func chip(at point: NSPoint) -> ProjectGraphModel.Chip? {
        layout?.chips.first { $0.frame.contains(point) }
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let chip = chip(at: point) else {
            selectedIndex = nil
            onSelect?(nil)
            return
        }

        if event.clickCount == 2 {
            onDoubleClick?(chip.vertex)
            return
        }

        selectedIndex = chip.index
        onSelect?(chip.index)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas {
            removeTrackingArea(area)
        }
        addTrackingArea(NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .activeInKeyWindow],
            owner: self))
    }

    override func mouseMoved(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let chip = chip(at: point) {
            let vertex = chip.vertex
            toolTip = "\(vertex.value)\nReferences: \(vertex.outgoing?.count ?? 0)"
        } else {
            toolTip = nil
        }
    }
}
