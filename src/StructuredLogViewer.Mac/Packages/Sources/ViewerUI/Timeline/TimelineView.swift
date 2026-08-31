import AppKit
import SwiftUI
import ViewerCore

/// The Timeline tab (the WPF viewer's Timeline/Tracing view): one lane
/// per MSBuild worker node, blocks nested flame-chart style by depth.
/// Custom-drawn NSView in a scroll view — draws only what intersects the
/// dirty rect, so multi-thousand-block builds stay fluid. Pinch (or the
/// zoom buttons) zooms time; click reveals the node in the build tree.
struct TimelineHostView: View {
    let session: BuildSession
    let onRevealNode: (String) -> Void

    @State private var timeline: BuildTimeline?
    @State private var errorMessage: String?
    @State private var pxPerMs: Double = 0

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
        HStack(spacing: 10) {
            Text("\(timeline.lanes.count) node\(timeline.lanes.count == 1 ? "" : "s")")
                .foregroundStyle(.secondary)
            Text(NodeStyling.formatDuration(milliseconds: timeline.durationMs))
                .foregroundStyle(.secondary)

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

    struct LaneLayout {
        let lane: TimelineLane
        let yOffset: CGFloat
        let height: CGFloat
    }

    static func layoutLanes(_ timeline: BuildTimeline) -> [LaneLayout] {
        var result: [LaneLayout] = []
        var y = rulerHeight
        for lane in timeline.lanes {
            let height = laneHeaderHeight + CGFloat(lane.maxIndent + 1) * blockRowHeight
            result.append(LaneLayout(lane: lane, yOffset: y, height: height))
            y += height + laneGap
        }
        return result
    }
}

struct TimelineScrollView: NSViewRepresentable {
    let timeline: BuildTimeline
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
        scrollView.onLayout = { [weak scrollView, weak content] in
            guard let scrollView, let content else { return }
            context.coordinator.fitIfNeeded(scrollView: scrollView, content: content)
        }
        context.coordinator.contentView = content
        context.coordinator.scrollView = scrollView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        guard let content = coordinator.contentView else { return }
        content.onBlockClick = onBlockClick
        coordinator.timeline = timeline
        coordinator.setPxPerMs = { value in DispatchQueue.main.async { pxPerMs = value } }

        if content.lanes.isEmpty || content.totalDurationMs != timeline.durationMs {
            content.configure(timeline: timeline)
        }

        if pxPerMs <= 0 {
            // Fit-to-window sentinel: wait for a real layout pass — the
            // scroll view has no meaningful width during the first update.
            coordinator.needsFit = true
            coordinator.fitIfNeeded(scrollView: scrollView, content: content)
        } else if abs(content.pxPerMs - CGFloat(pxPerMs)) > 0.000001 {
            content.setZoom(CGFloat(pxPerMs))
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        weak var contentView: TimelineContentView?
        weak var scrollView: NSScrollView?
        var timeline: BuildTimeline?
        var needsFit = false
        var setPxPerMs: ((Double) -> Void)?

        func fitIfNeeded(scrollView: NSScrollView, content: TimelineContentView) {
            guard needsFit, let timeline else { return }
            let visibleWidth = scrollView.contentSize.width - TimelineRender.leftPadding * 2
            guard visibleWidth > 50 else { return }

            needsFit = false
            let fitted = Double(visibleWidth) / max(timeline.durationMs, 1)
            content.setZoom(CGFloat(fitted))
            setPxPerMs?(fitted)
        }
    }
}

final class LayoutReportingScrollView: NSScrollView {
    var onLayout: (() -> Void)?

    override func layout() {
        super.layout()
        onLayout?()
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

    func configure(timeline: BuildTimeline) {
        totalDurationMs = timeline.durationMs
        lanes = TimelineRender.layoutLanes(timeline)
        totalHeight = (lanes.last.map { $0.yOffset + $0.height } ?? 0) + TimelineRender.laneGap
        resizeToFit()
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
            let title = layout.lane.nodeId == 0 ? "Evaluation" : "Node \(layout.lane.nodeId)"
            (title as NSString).draw(
                at: NSPoint(x: visibleRect.minX + TimelineRender.leftPadding, y: layout.yOffset + 4),
                withAttributes: headerAttributes)

            NSColor.separatorColor.setFill()
            NSRect(x: dirtyRect.minX, y: layout.yOffset + layout.height + TimelineRender.laneGap / 2,
                   width: dirtyRect.width, height: 1).fill()

            let rowsTop = layout.yOffset + TimelineRender.laneHeaderHeight

            for block in layout.lane.blocks {
                if block.start > visibleEndMs {
                    break // sorted by start; nothing further can be visible
                }
                if block.end < visibleStartMs {
                    continue
                }

                let x0 = xFor(ms: block.start)
                let x1 = xFor(ms: block.end)
                let width = max(x1 - x0, TimelineRender.minVisiblePx)
                let y = rowsTop + CGFloat(block.indent) * TimelineRender.blockRowHeight
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

    private func block(at point: NSPoint) -> TimelineBlock? {
        for layout in lanes {
            guard point.y >= layout.yOffset + TimelineRender.laneHeaderHeight,
                  point.y < layout.yOffset + layout.height else { continue }

            let indent = Int((point.y - layout.yOffset - TimelineRender.laneHeaderHeight) / TimelineRender.blockRowHeight)
            let ms = msFor(x: point.x)

            // Sorted by start; the last starting block at this indent that
            // still spans `ms` is the visually topmost one.
            var hit: TimelineBlock?
            for block in layout.lane.blocks {
                if block.start > ms { break }
                if block.indent == indent && block.end >= ms {
                    // Also allow hits on the minimum-width sliver of tiny blocks.
                    hit = block
                }
            }

            if hit == nil {
                for block in layout.lane.blocks {
                    if block.start > ms { break }
                    if block.indent == indent && xFor(ms: block.end) + TimelineRender.minVisiblePx >= point.x {
                        hit = block
                    }
                }
            }

            return hit
        }

        return nil
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let block = block(at: point) {
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
        if let block = block(at: point) {
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
