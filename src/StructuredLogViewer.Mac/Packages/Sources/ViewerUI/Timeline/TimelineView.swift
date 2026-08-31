import AppKit
import SwiftUI
import ViewerCore

/// The Tracing view (the WPF viewer's newer Timeline/Tracing tab): one
/// lane per MSBuild worker node, blocks nested flame-chart style.
/// Nesting depth is computed client-side over the *visible* blocks, so
/// the kind filters (Evaluations/Projects/Targets/Tasks, with counts —
/// same as WPF's Tracing menu) re-compact the chart instead of leaving
/// gaps. Custom-drawn NSView with dirty-rect culling; pinch or ⌥-scroll
/// zooms; clicking a block reveals the node in the build tree.
struct TimelineHostView: View {
    let session: BuildSession
    let onRevealNode: (String) -> Void

    @State private var timeline: BuildTimeline?
    @State private var errorMessage: String?
    @State private var pxPerMs: Double = 0
    @State private var filter = TracingFilter()

    var body: some View {
        Group {
            if let timeline {
                if timeline.lanes.isEmpty {
                    ContentUnavailableView(
                        "No Timing Data",
                        systemImage: "chart.bar.xaxis",
                        description: Text("This binlog has no timed nodes."))
                } else {
                    VStack(spacing: 0) {
                        TimelineScrollView(
                            timeline: timeline,
                            filter: filter,
                            pxPerMs: $pxPerMs,
                            onBlockClick: { onRevealNode($0.id) })
                        Divider()
                        controls(timeline)
                    }
                }
            } else if let errorMessage {
                ContentUnavailableView(errorMessage, systemImage: "exclamationmark.triangle")
            } else {
                ProgressView("Building timeline…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            guard timeline == nil, let engine = session.engine else { return }
            do {
                timeline = try await engine.timeline()
            } catch {
                errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    private func controls(_ timeline: BuildTimeline) -> some View {
        let counts = TracingFilter.counts(of: timeline)
        return HStack(spacing: 10) {
            Text("\(timeline.lanes.count) node\(timeline.lanes.count == 1 ? "" : "s")")
                .foregroundStyle(.secondary)
            Text(NodeStyling.formatDuration(milliseconds: timeline.durationMs))
                .foregroundStyle(.secondary)

            Menu {
                Toggle("Show Evaluations (\(counts.evaluations))", isOn: $filter.showEvaluations)
                Toggle("Show Projects (\(counts.projects))", isOn: $filter.showProjects)
                Toggle("Show Targets (\(counts.targets))", isOn: $filter.showTargets)
                Toggle("Show Tasks (\(counts.tasks))", isOn: $filter.showTasks)
                if counts.other > 0 {
                    Toggle("Show Other (\(counts.other))", isOn: $filter.showOther)
                }
            } label: {
                Label("Filter", systemImage: filter.isDefault ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()

            Spacer()

            legend

            Button {
                pxPerMs /= 1.5
            } label: {
                Image(systemName: "minus.magnifyingglass")
            }
            .help("Zoom out")

            Button {
                pxPerMs = 0 // sentinel: fit width
            } label: {
                Image(systemName: "arrow.left.and.right.square")
            }
            .help("Fit to window")

            Button {
                pxPerMs *= 1.5
            } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            .help("Zoom in")
        }
        .buttonStyle(.borderless)
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
    }

    private var legend: some View {
        HStack(spacing: 8) {
            legendDot(TimelineRender.color(kind: "ProjectEvaluation", hasError: false), "Evaluation")
            legendDot(TimelineRender.color(kind: "Project", hasError: false), "Project")
            legendDot(TimelineRender.color(kind: "Target", hasError: false), "Target")
            legendDot(TimelineRender.color(kind: "Task", hasError: false), "Task")
            legendDot(TimelineRender.color(kind: "Task", hasError: true), "Failed")
        }
        .padding(.trailing, 8)
    }

    private func legendDot(_ color: NSColor, _ label: String) -> some View {
        HStack(spacing: 3) {
            RoundedRectangle(cornerRadius: 2)
                .fill(Color(nsColor: color))
                .frame(width: 8, height: 8)
            Text(label)
                .foregroundStyle(.secondary)
        }
    }
}

/// Which block kinds are visible (WPF Tracing's Show Evaluations /
/// Projects / Targets / Tasks menu).
struct TracingFilter: Equatable {
    var showEvaluations = true
    var showProjects = true
    var showTargets = true
    var showTasks = true
    var showOther = true

    var isDefault: Bool {
        showEvaluations && showProjects && showTargets && showTasks && showOther
    }

    enum Category {
        case evaluation, project, target, task, other
    }

    static func category(of kind: String) -> Category {
        switch kind {
        case "ProjectEvaluation": return .evaluation
        case "Project": return .project
        case "Target": return .target
        case "Task": return .task
        default: return .other
        }
    }

    func allows(_ kind: String) -> Bool {
        switch Self.category(of: kind) {
        case .evaluation: return showEvaluations
        case .project: return showProjects
        case .target: return showTargets
        case .task: return showTasks
        case .other: return showOther
        }
    }

    struct Counts {
        var evaluations = 0
        var projects = 0
        var targets = 0
        var tasks = 0
        var other = 0
    }

    static func counts(of timeline: BuildTimeline) -> Counts {
        var counts = Counts()
        for lane in timeline.lanes {
            for block in lane.blocks {
                switch category(of: block.kind) {
                case .evaluation: counts.evaluations += 1
                case .project: counts.projects += 1
                case .target: counts.targets += 1
                case .task: counts.tasks += 1
                case .other: counts.other += 1
                }
            }
        }
        return counts
    }
}

/// Geometry + palette shared by the view and hit testing.
enum TimelineRender {
    static let laneHeaderHeight: CGFloat = 22
    static let blockRowHeight: CGFloat = 18
    static let laneGap: CGFloat = 10
    static let leftPadding: CGFloat = 8
    static let rulerHeight: CGFloat = 22
    static let minVisiblePx: CGFloat = 0.5

    static func color(kind: String, hasError: Bool) -> NSColor {
        if hasError {
            return .systemRed
        }

        switch kind {
        case "Project": return NSColor.systemGreen
        case "ProjectEvaluation": return NSColor.systemTeal
        case "Target": return NSColor.systemPurple
        case "Task": return NSColor.systemBlue
        default: return NSColor.systemGray
        }
    }

    struct PlacedBlock {
        let block: TimelineBlock
        let indent: Int
    }

    struct LaneLayout {
        let laneId: Int
        let yOffset: CGFloat
        let height: CGFloat
        /// Sorted by start, indent computed over visible blocks only.
        let rows: [PlacedBlock]
    }

    /// Applies the kind filter and re-computes nesting from time spans
    /// (see TimelineNesting): filtered-out levels compact away, as do the
    /// empty indent levels the raw tree depth would leave.
    static func layoutLanes(_ timeline: BuildTimeline, filter: TracingFilter) -> [LaneLayout] {
        var result: [LaneLayout] = []
        var y = rulerHeight

        for lane in timeline.lanes {
            let visible = lane.blocks.filter { filter.allows($0.kind) }
            guard !visible.isEmpty else { continue }

            let (placed, maxIndent) = TimelineNesting.place(visible)
            let rows = placed.map { PlacedBlock(block: $0.block, indent: $0.indent) }

            let height = laneHeaderHeight + CGFloat(maxIndent + 1) * blockRowHeight
            result.append(LaneLayout(laneId: lane.nodeId, yOffset: y, height: height, rows: rows))
            y += height + laneGap
        }

        return result
    }
}

struct TimelineScrollView: NSViewRepresentable {
    let timeline: BuildTimeline
    let filter: TracingFilter
    @Binding var pxPerMs: Double
    let onBlockClick: (TimelineBlock) -> Void

    func makeNSView(context: Context) -> NSScrollView {
        let content = TimelineContentView()
        content.onBlockClick = onBlockClick
        content.onZoomChanged = { newValue in
            DispatchQueue.main.async { pxPerMs = newValue }
        }

        let scrollView = LayoutReportingScrollView()
        scrollView.documentView = content
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.allowsMagnification = false
        scrollView.drawsBackground = true
        scrollView.backgroundColor = .textBackgroundColor
        scrollView.onLayout = {
            context.coordinator.scheduleApply()
        }
        context.coordinator.contentView = content
        context.coordinator.scrollView = scrollView
        return scrollView
    }

    // updateNSView can run inside an AppKit constraint-update pass (e.g.
    // while a split divider drags); mutating the document view there
    // re-enters layout and has crashed AppKit. Record the desired state
    // and apply it on a plain run-loop turn instead.
    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.contentView?.onBlockClick = onBlockClick
        coordinator.timeline = timeline
        coordinator.pendingFilter = filter
        coordinator.pendingPxPerMs = pxPerMs
        coordinator.setPxPerMs = { value in DispatchQueue.main.async { pxPerMs = value } }

        if pxPerMs <= 0 {
            // Fit-to-window sentinel.
            coordinator.needsFit = true
        }

        coordinator.scheduleApply()
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        weak var contentView: TimelineContentView?
        weak var scrollView: NSScrollView?
        var timeline: BuildTimeline?
        var lastFilter: TracingFilter?
        var pendingFilter: TracingFilter?
        var pendingPxPerMs: Double?
        var needsFit = false
        var setPxPerMs: ((Double) -> Void)?

        private var applyScheduled = false

        func scheduleApply() {
            guard !applyScheduled else { return }
            applyScheduled = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.applyScheduled = false
                self.apply()
            }
        }

        private func apply() {
            guard let content = contentView, let scrollView, let timeline else { return }

            if let filter = pendingFilter,
               content.lanes.isEmpty || content.totalDurationMs != timeline.durationMs || lastFilter != filter {
                lastFilter = filter
                content.configure(timeline: timeline, filter: filter)
            }

            // While a divider drag is live-resizing us, hold off — the
            // end of the resize triggers another layout report.
            guard !scrollView.inLiveResize else { return }

            if needsFit {
                let visibleWidth = scrollView.contentSize.width - TimelineRender.leftPadding * 2
                guard visibleWidth > 50 else { return }

                needsFit = false
                let fitted = Double(visibleWidth) / max(timeline.durationMs, 1)
                content.setZoom(CGFloat(fitted))
                setPxPerMs?(fitted)
            } else if let px = pendingPxPerMs, px > 0, abs(content.pxPerMs - CGFloat(px)) > 0.000001 {
                content.setZoom(CGFloat(px))
            }
        }
    }
}

final class LayoutReportingScrollView: NSScrollView {
    var onLayout: (() -> Void)?

    private var layoutReportScheduled = false

    override func layout() {
        super.layout()
        scheduleLayoutReport()
    }

    override func viewDidEndLiveResize() {
        super.viewDidEndLiveResize()
        // Callers defer geometry work while inLiveResize; make sure a
        // report lands once the divider drag / window resize finishes.
        scheduleLayoutReport()
    }

    // Both callers resize their document view in response. Doing that
    // while AppKit is still inside a layout pass re-enters layout — and
    // during a split-divider drag the pass runs from the nested event
    // loop in -[NSSplitView mouseDown:], where re-entrancy has crashed
    // the app. Report once the pass has unwound instead.
    private func scheduleLayoutReport() {
        guard onLayout != nil, !layoutReportScheduled else { return }
        layoutReportScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.layoutReportScheduled = false
            self.onLayout?()
        }
    }
}

final class TimelineContentView: NSView {
    private(set) var lanes: [TimelineRender.LaneLayout] = []
    private(set) var totalDurationMs: Double = 0
    private(set) var pxPerMs: CGFloat = 0.2

    var onBlockClick: ((TimelineBlock) -> Void)?
    var onZoomChanged: ((Double) -> Void)?

    private var totalHeight: CGFloat = 0

    override var isFlipped: Bool { true }

    func configure(timeline: BuildTimeline, filter: TracingFilter) {
        totalDurationMs = timeline.durationMs
        lanes = TimelineRender.layoutLanes(timeline, filter: filter)
        totalHeight = (lanes.last.map { $0.yOffset + $0.height } ?? 0) + TimelineRender.laneGap
        resizeToFit()
        needsDisplay = true
    }

    func setZoom(_ newPxPerMs: CGFloat) {
        let clamped = min(max(newPxPerMs, 0.0001), 500)
        guard clamped != pxPerMs else { return }

        // Keep the time under the viewport's center stable while zooming.
        let anchorX = visibleRect.midX
        let anchorMs = Double((anchorX - TimelineRender.leftPadding) / pxPerMs)

        pxPerMs = clamped
        resizeToFit()

        let newAnchorX = CGFloat(anchorMs) * pxPerMs + TimelineRender.leftPadding
        let newOriginX = max(0, newAnchorX - visibleRect.width / 2)
        scroll(NSPoint(x: newOriginX, y: visibleRect.minY))
        needsDisplay = true
    }

    private func resizeToFit() {
        let width = CGFloat(totalDurationMs) * pxPerMs + TimelineRender.leftPadding * 2
        setFrameSize(NSSize(width: max(width, 200), height: max(totalHeight, 200)))
    }

    private func xFor(ms: Double) -> CGFloat {
        CGFloat(ms) * pxPerMs + TimelineRender.leftPadding
    }

    private func msFor(x: CGFloat) -> Double {
        Double((x - TimelineRender.leftPadding) / pxPerMs)
    }

    // MARK: - drawing

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }

        drawRuler(dirtyRect, context)

        let labelAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 10),
            .foregroundColor: NSColor.labelColor,
        ]
        let headerAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 10, weight: .semibold),
            .foregroundColor: NSColor.secondaryLabelColor,
        ]

        let visibleStartMs = msFor(x: dirtyRect.minX) - 1
        let visibleEndMs = msFor(x: dirtyRect.maxX) + 1

        for layout in lanes {
            let laneRect = NSRect(x: 0, y: layout.yOffset, width: bounds.width, height: layout.height)
            guard laneRect.intersects(dirtyRect) else { continue }

            // Lane header (drawn at the visible left edge so it stays put
            // during horizontal scrolling).
            let title = layout.laneId == 0 ? "Evaluation" : "Node \(layout.laneId)"
            (title as NSString).draw(
                at: NSPoint(x: visibleRect.minX + TimelineRender.leftPadding, y: layout.yOffset + 4),
                withAttributes: headerAttributes)

            NSColor.separatorColor.setFill()
            NSRect(x: dirtyRect.minX, y: layout.yOffset + layout.height + TimelineRender.laneGap / 2,
                   width: dirtyRect.width, height: 1).fill()

            let rowsTop = layout.yOffset + TimelineRender.laneHeaderHeight

            for placed in layout.rows {
                let block = placed.block
                if block.start > visibleEndMs {
                    break // sorted by start; nothing further can be visible
                }
                if block.end < visibleStartMs {
                    continue
                }

                let x0 = xFor(ms: block.start)
                let x1 = xFor(ms: block.end)
                let width = max(x1 - x0, TimelineRender.minVisiblePx)
                let y = rowsTop + CGFloat(placed.indent) * TimelineRender.blockRowHeight
                let rect = NSRect(x: x0, y: y + 1, width: width, height: TimelineRender.blockRowHeight - 2)

                let color = TimelineRender.color(kind: block.kind, hasError: block.hasError)
                context.setFillColor(color.withAlphaComponent(0.55).cgColor)
                let path = CGPath(
                    roundedRect: rect,
                    cornerWidth: min(2, rect.width / 2),
                    cornerHeight: 2,
                    transform: nil)
                context.addPath(path)
                context.fillPath()

                if rect.width > 3 {
                    context.setStrokeColor(color.cgColor)
                    context.setLineWidth(0.5)
                    context.addPath(path)
                    context.strokePath()
                }

                if rect.width > 40, let text = block.text, !text.isEmpty {
                    let textRect = rect.insetBy(dx: 4, dy: 1)
                    drawTruncated(text, in: textRect, attributes: labelAttributes)
                }
            }
        }
    }

    private func drawTruncated(_ text: String, in rect: NSRect, attributes: [NSAttributedString.Key: Any]) {
        let attributed = NSAttributedString(string: text, attributes: attributes)
        attributed.draw(with: rect, options: [.truncatesLastVisibleLine, .usesLineFragmentOrigin])
    }

    private func drawRuler(_ dirtyRect: NSRect, _ context: CGContext) {
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 9, weight: .regular),
            .foregroundColor: NSColor.tertiaryLabelColor,
        ]

        // Pick a tick step of 1/2/5×10^n ms that yields ≥80px spacing.
        let minSpacingPx: CGFloat = 80
        var stepMs: Double = 1
        while CGFloat(stepMs) * pxPerMs < minSpacingPx {
            let magnitude = pow(10, floor(log10(stepMs)))
            let mantissa = stepMs / magnitude
            stepMs = mantissa < 2 ? 2 * magnitude : (mantissa < 5 ? 5 * magnitude : 10 * magnitude)
        }

        let firstTick = max(0, floor(msFor(x: dirtyRect.minX) / stepMs) * stepMs)
        var tick = firstTick
        NSColor.separatorColor.setFill()
        while tick <= totalDurationMs {
            let x = xFor(ms: tick)
            if x > dirtyRect.maxX { break }

            NSRect(x: x, y: TimelineRender.rulerHeight - 4, width: 1, height: 4).fill()
            NSRect(x: x, y: TimelineRender.rulerHeight, width: 0.5, height: bounds.height).fill()

            let label = tick >= 1000
                ? String(format: "%.6g s", tick / 1000)
                : String(format: "%.6g ms", tick)
            (label as NSString).draw(at: NSPoint(x: x + 3, y: 4), withAttributes: attributes)
            tick += stepMs
        }
    }

    // MARK: - interaction

    private func placedBlock(at point: NSPoint) -> TimelineBlock? {
        for layout in lanes {
            guard point.y >= layout.yOffset + TimelineRender.laneHeaderHeight,
                  point.y < layout.yOffset + layout.height else { continue }

            let indent = Int((point.y - layout.yOffset - TimelineRender.laneHeaderHeight) / TimelineRender.blockRowHeight)
            let ms = msFor(x: point.x)

            // Sorted by start; the last starting block at this indent that
            // still spans the point (or its minimum-width sliver) wins.
            var hit: TimelineBlock?
            for placed in layout.rows {
                if placed.block.start > ms { break }
                if placed.indent == indent,
                   xFor(ms: placed.block.end) + TimelineRender.minVisiblePx >= point.x {
                    hit = placed.block
                }
            }

            return hit
        }

        return nil
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let block = placedBlock(at: point) {
            onBlockClick?(block)
        } else {
            super.mouseDown(with: event)
        }
    }

    override func magnify(with event: NSEvent) {
        let newZoom = pxPerMs * (1 + event.magnification)
        setZoom(newZoom)
        onZoomChanged?(Double(pxPerMs))
    }

    override func scrollWheel(with event: NSEvent) {
        // ⌥-scroll zooms, like most timeline tools.
        if event.modifierFlags.contains(.option) {
            let factor = event.scrollingDeltaY > 0 ? 1.1 : 1 / 1.1
            setZoom(pxPerMs * CGFloat(factor))
            onZoomChanged?(Double(pxPerMs))
            return
        }

        super.scrollWheel(with: event)
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
        if let block = placedBlock(at: point) {
            toolTip = tooltipText(block)
        } else {
            toolTip = nil
        }
    }

    private func tooltipText(_ block: TimelineBlock) -> String {
        var lines: [String] = []
        lines.append("\(block.kind) \(block.text ?? "")")
        lines.append("Duration: \(NodeStyling.formatDuration(milliseconds: block.duration))")
        lines.append(String(format: "Start: +%@", NodeStyling.formatDuration(milliseconds: block.start)))
        if block.hasError {
            lines.append("Contains errors")
        }
        return lines.joined(separator: "\n")
    }
}
