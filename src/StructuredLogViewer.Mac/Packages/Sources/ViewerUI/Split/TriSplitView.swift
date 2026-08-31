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
/// split is plain AppKit: NSHostingControllers with empty sizingOptions
/// never feed SwiftUI sizes into Auto Layout, and divider tracking is
/// NSSplitView's own.
///
/// Collapse sync rules: the visibility bindings are commands, not
/// mirrors. updateNSViewController applies a collapse/expand only when
/// the *binding value changed* since the last update — never by
/// comparing against live NSSplitViewItem state, which fights the user
/// mid-divider-drag and wrecks the layout. User-driven collapses flow
/// back through KVO on the next run-loop turn.
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

        let coordinator = context.coordinator

        let sidebarHost = NSHostingController(rootView: sidebar)
        sidebarHost.sizingOptions = []
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarHost)
        sidebarItem.minimumThickness = 220
        sidebarItem.canCollapse = true
        sidebarItem.isCollapsed = !sidebarVisible
        // Collapsing a pane resizes its siblings, never the window.
        sidebarItem.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
        sidebarItem.holdingPriority = NSLayoutConstraint.Priority(260)

        let contentHost = NSHostingController(rootView: content)
        contentHost.sizingOptions = []
        let contentItem = NSSplitViewItem(viewController: contentHost)
        contentItem.minimumThickness = 320
        // Lowest holding priority: the content pane absorbs window and
        // divider resizes; sidebar/inspector keep their widths.
        contentItem.holdingPriority = NSLayoutConstraint.Priority(240)

        let inspectorHost = NSHostingController(rootView: inspector)
        inspectorHost.sizingOptions = []
        let inspectorItem = NSSplitViewItem(inspectorWithViewController: inspectorHost)
        inspectorItem.minimumThickness = 280
        inspectorItem.canCollapse = true
        inspectorItem.isCollapsed = !inspectorVisible
        inspectorItem.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
        inspectorItem.holdingPriority = NSLayoutConstraint.Priority(260)

        controller.splitViewItems = [sidebarItem, contentItem, inspectorItem]
        controller.splitView.autosaveName = "MainWindowSplit.v2"

        coordinator.sidebarHost = sidebarHost
        coordinator.contentHost = contentHost
        coordinator.inspectorHost = inspectorHost
        coordinator.sidebarItem = sidebarItem
        coordinator.inspectorItem = inspectorItem
        coordinator.appliedSidebarVisible = sidebarVisible
        coordinator.appliedInspectorVisible = inspectorVisible

        // Reflect user-driven collapses (dragging a divider closed/open)
        // back into the bindings on the next turn. Recording the value in
        // `applied*` first keeps the next update from re-applying it.
        coordinator.observations = [
            sidebarItem.observe(\.isCollapsed) { [weak coordinator] item, _ in
                let visible = !item.isCollapsed
                DispatchQueue.main.async {
                    guard let coordinator, coordinator.appliedSidebarVisible != visible else { return }
                    coordinator.appliedSidebarVisible = visible
                    coordinator.setSidebarVisible?(visible)
                }
            },
            inspectorItem.observe(\.isCollapsed) { [weak coordinator] item, _ in
                let visible = !item.isCollapsed
                DispatchQueue.main.async {
                    guard let coordinator, coordinator.appliedInspectorVisible != visible else { return }
                    coordinator.appliedInspectorVisible = visible
                    coordinator.setInspectorVisible?(visible)
                }
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

        // Apply only genuine programmatic changes (toolbar buttons, menu
        // commands): react to the binding differing from what we last
        // applied, not from live item state.
        if coordinator.appliedSidebarVisible != sidebarVisible {
            coordinator.appliedSidebarVisible = sidebarVisible
            coordinator.sidebarItem?.animator().isCollapsed = !sidebarVisible
        }

        if coordinator.appliedInspectorVisible != inspectorVisible {
            coordinator.appliedInspectorVisible = inspectorVisible
            coordinator.inspectorItem?.animator().isCollapsed = !inspectorVisible
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
        var appliedSidebarVisible = true
        var appliedInspectorVisible = false
        var setSidebarVisible: ((Bool) -> Void)?
        var setInspectorVisible: ((Bool) -> Void)?
        var observations: [NSKeyValueObservation] = []
    }
}
