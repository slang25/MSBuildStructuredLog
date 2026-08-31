import AppKit
import SwiftUI

/// Three-pane split (sidebar / content / inspector) backed directly by
/// NSSplitViewController instead of NavigationSplitView + .inspector.
///
/// Why: on macOS 26, SwiftUI's split plumbing crashes during divider
/// drags — SplitViewChildController reacts to hosting-view min/max size
/// updates *inside* AppKit's constraint-update pass and synchronously
/// invalidates its platform host there, which throws
/// (NSInternalInconsistency in _postWindowNeedsUpdateConstraints).
/// Deferring our own geometry work and pinning column min/max sizes both
/// failed to starve that path (three identical crash reports), so the
/// split is now plain AppKit: NSHostingControllers with empty
/// sizingOptions never feed SwiftUI sizes into Auto Layout, and divider
/// tracking is NSSplitView's own, which is re-entrancy safe.
struct TriSplitView<Sidebar: View, Content: View, Inspector: View>: NSViewControllerRepresentable {
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

    func makeNSViewController(context: Context) -> NSSplitViewController {
        let controller = NSSplitViewController()
        controller.splitView.autosaveName = "MainWindowSplit"

        let coordinator = context.coordinator

        let sidebarHost = NSHostingController(rootView: sidebar)
        sidebarHost.sizingOptions = []
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarHost)
        sidebarItem.minimumThickness = 240
        sidebarItem.maximumThickness = 500
        sidebarItem.canCollapse = true
        sidebarItem.isCollapsed = !sidebarVisible

        let contentHost = NSHostingController(rootView: content)
        contentHost.sizingOptions = []
        let contentItem = NSSplitViewItem(viewController: contentHost)
        contentItem.minimumThickness = 400

        let inspectorHost = NSHostingController(rootView: inspector)
        inspectorHost.sizingOptions = []
        let inspectorItem = NSSplitViewItem(inspectorWithViewController: inspectorHost)
        inspectorItem.minimumThickness = 300
        inspectorItem.canCollapse = true
        inspectorItem.isCollapsed = !inspectorVisible

        controller.splitViewItems = [sidebarItem, contentItem, inspectorItem]

        coordinator.sidebarHost = sidebarHost
        coordinator.contentHost = contentHost
        coordinator.inspectorHost = inspectorHost
        coordinator.sidebarItem = sidebarItem
        coordinator.inspectorItem = inspectorItem

        // Reflect user-driven collapses (double-click on divider, drag to
        // zero) back into the bindings, off the current update turn.
        coordinator.observations = [
            sidebarItem.observe(\.isCollapsed) { [weak coordinator] item, _ in
                let visible = !item.isCollapsed
                DispatchQueue.main.async { coordinator?.setSidebarVisible?(visible) }
            },
            inspectorItem.observe(\.isCollapsed) { [weak coordinator] item, _ in
                let visible = !item.isCollapsed
                DispatchQueue.main.async { coordinator?.setInspectorVisible?(visible) }
            },
        ]

        return controller
    }

    func updateNSViewController(_ controller: NSSplitViewController, context: Context) {
        let coordinator = context.coordinator
        coordinator.sidebarHost?.rootView = sidebar
        coordinator.contentHost?.rootView = content
        coordinator.inspectorHost?.rootView = inspector
        coordinator.setSidebarVisible = { sidebarVisible = $0 }
        coordinator.setInspectorVisible = { inspectorVisible = $0 }

        if let item = coordinator.sidebarItem, item.isCollapsed == sidebarVisible {
            item.animator().isCollapsed = !sidebarVisible
        }
        if let item = coordinator.inspectorItem, item.isCollapsed == inspectorVisible {
            item.animator().isCollapsed = !inspectorVisible
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        var sidebarHost: NSHostingController<Sidebar>?
        var contentHost: NSHostingController<Content>?
        var inspectorHost: NSHostingController<Inspector>?
        var sidebarItem: NSSplitViewItem?
        var inspectorItem: NSSplitViewItem?
        var setSidebarVisible: ((Bool) -> Void)?
        var setInspectorVisible: ((Bool) -> Void)?
        var observations: [NSKeyValueObservation] = []
    }
}
