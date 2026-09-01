import AppKit
import SwiftUI

/// Three-pane split (sidebar / content / inspector) on a classic,
/// frame-based NSSplitView with a delegate — no NavigationSplitView, no
/// NSSplitViewController.
///
/// Why not NavigationSplitView: on macOS 26 its plumbing crashes during
/// divider drags (it invalidates its platform host inside AppKit's
/// constraint-update pass — three identical crash reports).
/// Why not NSSplitViewController: its Auto Layout collapse machinery
/// left stale constraints after drag-collapse/re-expand, squeezing the
/// content pane. The legacy delegate path sets frames directly: the
/// delegate owns min/max coordinates, collapse rules and resize
/// distribution, so there is nothing left to disagree.
struct TriSplitView<Sidebar: View, Content: View, Inspector: View>: NSViewRepresentable {
    @Binding var sidebarVisible: Bool
    @Binding var inspectorVisible: Bool
    let sidebar: Sidebar
    let content: Content
    let inspector: Inspector

    init(
        sidebarVisible: Binding<Bool>,
        inspectorVisible: Binding<Bool>,
        @ViewBuilder sidebar: () -> Sidebar,
        @ViewBuilder content: () -> Content,
        @ViewBuilder inspector: () -> Inspector
    ) {
        self._sidebarVisible = sidebarVisible
        self._inspectorVisible = inspectorVisible
        self.sidebar = sidebar()
        self.content = content()
        self.inspector = inspector()
    }

    func makeNSView(context: Context) -> NSSplitView {
        let coordinator = context.coordinator

        let splitView = NSSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin

        let sidebarHost = NSHostingView(rootView: sidebar)
        sidebarHost.sizingOptions = []
        let sidebarPane = NSVisualEffectView()
        sidebarPane.material = .sidebar
        sidebarPane.blendingMode = .behindWindow
        sidebarHost.frame = sidebarPane.bounds
        sidebarHost.autoresizingMask = [.width, .height]
        sidebarPane.addSubview(sidebarHost)
        sidebarPane.frame = NSRect(x: 0, y: 0, width: 300, height: 600)

        let contentHost = NSHostingView(rootView: content)
        contentHost.sizingOptions = []
        contentHost.frame = NSRect(x: 0, y: 0, width: 700, height: 600)

        let inspectorHost = NSHostingView(rootView: inspector)
        inspectorHost.sizingOptions = []
        inspectorHost.frame = NSRect(x: 0, y: 0, width: 460, height: 600)
        inspectorHost.isHidden = !inspectorVisible

        sidebarPane.isHidden = !sidebarVisible

        splitView.addSubview(sidebarPane)
        splitView.addSubview(contentHost)
        splitView.addSubview(inspectorHost)

        coordinator.splitView = splitView
        coordinator.sidebarPane = sidebarPane
        coordinator.sidebarHost = sidebarHost
        coordinator.contentHost = contentHost
        coordinator.inspectorHost = inspectorHost
        coordinator.appliedSidebarVisible = sidebarVisible
        coordinator.appliedInspectorVisible = inspectorVisible
        splitView.delegate = coordinator

        return splitView
    }

    func updateNSView(_ splitView: NSSplitView, context: Context) {
        let coordinator = context.coordinator
        coordinator.sidebarHost?.rootView = sidebar
        coordinator.contentHost?.rootView = content
        coordinator.inspectorHost?.rootView = inspector
        coordinator.setSidebarVisible = { sidebarVisible = $0 }
        coordinator.setInspectorVisible = { inspectorVisible = $0 }

        // Bindings are commands: only apply genuine programmatic changes
        // (toolbar/menu). User drags report back through the delegate.
        if coordinator.appliedSidebarVisible != sidebarVisible {
            coordinator.appliedSidebarVisible = sidebarVisible
            coordinator.setPane(coordinator.sidebarPane, visible: sidebarVisible)
        }

        if coordinator.appliedInspectorVisible != inspectorVisible {
            coordinator.appliedInspectorVisible = inspectorVisible
            coordinator.setPane(coordinator.inspectorHost, visible: inspectorVisible)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator: NSObject, NSSplitViewDelegate {
        static var sidebarMin: CGFloat { 220 }
        static var contentMin: CGFloat { 320 }
        static var inspectorMin: CGFloat { 280 }

        weak var splitView: NSSplitView?
        weak var sidebarPane: NSVisualEffectView?
        var sidebarHost: NSHostingView<Sidebar>?
        var contentHost: NSHostingView<Content>?
        var inspectorHost: NSHostingView<Inspector>?

        var appliedSidebarVisible = true
        var appliedInspectorVisible = false
        var setSidebarVisible: ((Bool) -> Void)?
        var setInspectorVisible: ((Bool) -> Void)?

        // Remembered widths for re-expanding after a collapse.
        var savedSidebarWidth: CGFloat = 300
        var savedInspectorWidth: CGFloat = 460

        private func isPaneVisible(_ view: NSView?) -> Bool {
            guard let view, let splitView else { return false }
            return !view.isHidden && !splitView.isSubviewCollapsed(view)
        }

        func setPane(_ view: NSView?, visible: Bool) {
            guard let view, let splitView else { return }

            if visible {
                view.isHidden = false
                // Restore the remembered width whether it was hidden or
                // drag-collapsed to zero.
                if view === sidebarPane {
                    splitView.setPosition(savedSidebarWidth, ofDividerAt: 0)
                } else {
                    let target = max(
                        splitView.bounds.width - savedInspectorWidth - splitView.dividerThickness,
                        Self.sidebarWidthNow(self) + Self.contentMin)
                    splitView.setPosition(target, ofDividerAt: 1)
                }
            } else {
                view.isHidden = true
            }

            splitView.adjustSubviews()
        }

        private static func sidebarWidthNow(_ coordinator: Coordinator) -> CGFloat {
            guard let pane = coordinator.sidebarPane,
                  coordinator.isPaneVisible(pane) else { return 0 }
            return pane.frame.width
        }

        // MARK: - NSSplitViewDelegate

        func splitView(_ splitView: NSSplitView, canCollapseSubview subview: NSView) -> Bool {
            subview === sidebarPane || subview === inspectorHost
        }

        func splitView(
            _ splitView: NSSplitView,
            shouldCollapseSubview subview: NSView,
            forDoubleClickOnDividerAt dividerIndex: Int
        ) -> Bool {
            subview === sidebarPane || subview === inspectorHost
        }

        func splitView(_ splitView: NSSplitView, shouldHideDividerAt dividerIndex: Int) -> Bool {
            // Hide the divider only for programmatically hidden panes;
            // a drag-collapsed pane keeps its divider so it can be
            // dragged back out.
            if dividerIndex == 0 {
                return sidebarPane?.isHidden ?? false
            }
            return inspectorHost?.isHidden ?? false
        }

        func splitView(
            _ splitView: NSSplitView,
            constrainMinCoordinate proposedMinimumPosition: CGFloat,
            ofSubviewAt dividerIndex: Int
        ) -> CGFloat {
            if dividerIndex == 0 {
                return Self.sidebarMin
            }

            let sidebarWidth = isPaneVisible(sidebarPane)
                ? (sidebarPane?.frame.width ?? 0) + splitView.dividerThickness
                : 0
            return sidebarWidth + Self.contentMin
        }

        func splitView(
            _ splitView: NSSplitView,
            constrainMaxCoordinate proposedMaximumPosition: CGFloat,
            ofSubviewAt dividerIndex: Int
        ) -> CGFloat {
            let width = splitView.bounds.width
            if dividerIndex == 0 {
                let inspectorWidth = isPaneVisible(inspectorHost)
                    ? (inspectorHost?.frame.width ?? 0) + splitView.dividerThickness
                    : 0
                return width - inspectorWidth - Self.contentMin - splitView.dividerThickness
            }

            return width - Self.inspectorMin
        }

        /// Content absorbs all resize delta; sidebar and inspector hold
        /// their widths (clamped so content keeps its minimum).
        func splitView(_ splitView: NSSplitView, resizeSubviewsWithOldSize oldSize: NSSize) {
            guard let sidebarPane, let contentHost, let inspectorHost else { return }

            let bounds = splitView.bounds
            let thickness = splitView.dividerThickness

            let sidebarShown = isPaneVisible(sidebarPane)
            let inspectorShown = isPaneVisible(inspectorHost)

            var sidebarWidth = sidebarShown ? sidebarPane.frame.width : 0
            var inspectorWidth = inspectorShown ? inspectorHost.frame.width : 0
            let dividers = (sidebarShown ? thickness : 0) + (inspectorShown ? thickness : 0)

            var contentWidth = bounds.width - sidebarWidth - inspectorWidth - dividers
            if contentWidth < Self.contentMin {
                var deficit = Self.contentMin - contentWidth
                let fromSidebar = min(deficit, max(0, sidebarWidth - Self.sidebarMin))
                sidebarWidth -= fromSidebar
                deficit -= fromSidebar
                let fromInspector = min(deficit, max(0, inspectorWidth - Self.inspectorMin))
                inspectorWidth -= fromInspector
                contentWidth = bounds.width - sidebarWidth - inspectorWidth - dividers
            }

            var x: CGFloat = 0
            sidebarPane.frame = NSRect(x: 0, y: 0, width: sidebarWidth, height: bounds.height)
            if sidebarShown {
                x = sidebarWidth + thickness
            }

            contentHost.frame = NSRect(x: x, y: 0, width: max(contentWidth, 0), height: bounds.height)
            x += max(contentWidth, 0)
            if inspectorShown {
                x += thickness
            }

            inspectorHost.frame = NSRect(x: x, y: 0, width: max(bounds.width - x, 0), height: bounds.height)
        }

        func splitViewDidResizeSubviews(_ notification: Notification) {
            guard let splitView else { return }

            // Remember healthy widths for restore-after-collapse.
            if let pane = sidebarPane, isPaneVisible(pane), pane.frame.width >= Self.sidebarMin {
                savedSidebarWidth = pane.frame.width
            }
            if let host = inspectorHost, isPaneVisible(host), host.frame.width >= Self.inspectorMin {
                savedInspectorWidth = host.frame.width
            }

            // Report drag-collapses/expands back into the bindings.
            let sidebarNow = isPaneVisible(sidebarPane)
            let inspectorNow = isPaneVisible(inspectorHost)
            _ = splitView

            if sidebarNow != appliedSidebarVisible {
                appliedSidebarVisible = sidebarNow
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.setSidebarVisible?(self.appliedSidebarVisible)
                }
            }

            if inspectorNow != appliedInspectorVisible {
                appliedInspectorVisible = inspectorNow
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.setInspectorVisible?(self.appliedInspectorVisible)
                }
            }
        }
    }
}
