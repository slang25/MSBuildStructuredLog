import BinlogKit
import SwiftUI
import ViewerCore

/// The app-facing per-document root: drives load state and hosts the main
/// window once the build is open. Views never read file contents — they
/// hand the path to BuildSession, which streams it through libmslog.
public struct BinlogDocumentView: View {
    private let url: URL
    @State private var session: BuildSession

    public init(url: URL) {
        self.url = url
        _session = State(initialValue: BuildSession { path, progress in
            try await BinlogSession.open(path: path, progress: progress)
        })
    }

    public var body: some View {
        Group {
            switch session.phase {
            case .idle:
                Color.clear
            case .loading(let ratio):
                VStack(spacing: 12) {
                    ProgressView(value: max(0, min(1, ratio))) {
                        Text("Loading \(url.lastPathComponent)…")
                    }
                    .frame(width: 320)
                    Text("Reading, analyzing and indexing the build log")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Could Not Open Build Log", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { session.load(path: url.path) }
                }
            case .loaded:
                MainWindowView(session: session)
            }
        }
        .task(id: url) {
            if case .idle = session.phase {
                session.load(path: url.path)
            }
        }
        .onDisappear {
            session.closeEngine()
        }
    }
}

struct MainWindowView: View {
    @Bindable var session: BuildSession

    enum DetailMode: String, CaseIterable {
        case tree
        case tracing
        case projectGraph
    }

    @State private var pane: SidebarPane = .searchLog
    // -tracing / -graph launch arguments pick the initial mode (debug aid).
    @State private var detailMode: DetailMode = {
        let arguments = ProcessInfo.processInfo.arguments
        if arguments.contains("-tracing") || arguments.contains("-timeline") { return .tracing }
        if arguments.contains("-graph") { return .projectGraph }
        return .tree
    }()
    @State private var sidebarPresented = true
    @State private var inspectorPresented = false
    @State private var showingStats = false
    @State private var selectedNode: NodeSummary?
    @State private var jumpChain: [NodeSummary] = []

    var body: some View {
        // AppKit-backed split (see TriSplitView): NavigationSplitView +
        // .inspector crash on macOS 26 during divider drags.
        TriSplitView(
            sidebarVisible: $sidebarPresented,
            inspectorVisible: $inspectorPresented
        ) {
            sidebar
        } content: {
            detail
        } inspector: {
            DocumentWellView(sources: session.sources)
        }
        .toolbar { toolbarContent }
        .sheet(isPresented: $showingStats) {
            if let engine = session.engine {
                StatisticsView(engine: engine)
            }
        }
        .onChange(of: session.sources.presentationToken) {
            if !session.sources.tabs.isEmpty {
                inspectorPresented = true
            }
        }
        .focusedSceneValue(\.buildSession, session)
        .focusedSceneValue(\.paneSelection, $pane)
        .task {
            // -reveal <nodeId> launch argument (debug aid): jump straight
            // to a node once the tree is up.
            let arguments = ProcessInfo.processInfo.arguments
            if let flagIndex = arguments.firstIndex(of: "-reveal"), arguments.indices.contains(flagIndex + 1) {
                try? await Task.sleep(nanoseconds: 500_000_000)
                session.requestReveal(nodeId: arguments[flagIndex + 1])
            }

            // -source <path> (debug aid): open an embedded build file in the
            // document well, so the semantic layer can be exercised directly.
            if let flagIndex = arguments.firstIndex(of: "-source"), arguments.indices.contains(flagIndex + 1) {
                try? await Task.sleep(nanoseconds: 500_000_000)
                session.sources.openFile(path: arguments[flagIndex + 1])
            }
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            Picker("Pane", selection: $pane) {
                ForEach(SidebarPane.allCases) { pane in
                    Image(systemName: pane.symbolName)
                        .help(pane.title)
                        .tag(pane)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 8)
            .padding(.vertical, 6)

            Divider()

            switch pane {
            case .searchLog:
                SearchLogPane(controller: session.searchLog, onSelect: revealFromPane)
            case .propertiesAndItems:
                PropertiesItemsPane(session: session, controller: session.propertiesAndItems, onSelect: revealFromPane)
            case .files:
                FilesPane(controller: session.findInFiles) { file in
                    session.sources.openFile(path: file.path)
                }
            case .findInFiles:
                FindInFilesPane(controller: session.findInFiles) { path, line in
                    session.sources.openFile(path: path, line: line)
                }
            case .favorites:
                FavoritesPane(favorites: session.favorites, onSelect: revealFromPane)
            }
        }
    }

    private var detail: some View {
        VStack(spacing: 0) {
            if detailMode == .tree {
                JumpBarView(chain: jumpChain) { element in
                    session.requestReveal(nodeId: element.id)
                }

                Divider()
            }

            if let store = session.store {
                // The tree stays alive (hidden) while other views show so
                // expansion, selection and pending reveals are preserved.
                ZStack {
                    BuildTreeView(
                        store: store,
                        revealRequest: session.revealRequest,
                        menuActions: menuActions,
                        onSelect: handleSelect,
                        onDoubleClick: handleDoubleClick)
                        .opacity(detailMode == .tree ? 1 : 0)

                    if detailMode == .tracing {
                        TimelineHostView(session: session) { nodeId in
                            detailMode = .tree
                            session.requestReveal(nodeId: nodeId)
                        }
                        .background(Color(nsColor: .textBackgroundColor))
                    } else if detailMode == .projectGraph {
                        ProjectGraphHostView(session: session) { query in
                            pane = .searchLog
                            session.searchLog.query = query
                            session.searchLog.searchNow()
                        }
                        .background(Color(nsColor: .textBackgroundColor))
                    }
                }
            }

            Divider()
            statusBar
        }
    }

    private var statusBar: some View {
        HStack(spacing: 12) {
            if let info = session.info {
                Text(info.succeeded ? "Build succeeded" : "Build failed")
                    .foregroundStyle(info.succeeded ? Color.green : Color.red)
                Text("\(info.nodeCount.formatted()) nodes")
                    .foregroundStyle(.secondary)
                if info.durationMs > 0 {
                    Text(NodeStyling.formatDuration(milliseconds: info.durationMs))
                        .foregroundStyle(.secondary)
                }
                if let version = info.msBuildVersion {
                    Text("MSBuild \(version)")
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Text(ByteCountFormatter.string(fromByteCount: info.fileSize, countStyle: .file))
                    .foregroundStyle(.secondary)
            }
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            Button {
                sidebarPresented.toggle()
            } label: {
                Label("Sidebar", systemImage: "sidebar.leading")
            }
            .help("Toggle the sidebar")
        }

        ToolbarItemGroup {
            Picker("View", selection: $detailMode) {
                Image(systemName: "list.bullet.indent")
                    .help("Log tree")
                    .tag(DetailMode.tree)
                Image(systemName: "chart.bar.xaxis")
                    .help("Tracing timeline")
                    .tag(DetailMode.tracing)
                Image(systemName: "point.3.connected.trianglepath.dotted")
                    .help("Project references")
                    .tag(DetailMode.projectGraph)
            }
            .pickerStyle(.segmented)

            Button {
                session.reload()
            } label: {
                Label("Reload", systemImage: "arrow.clockwise")
            }
            .help("Reload the binlog from disk (⌘R)")

            if let info = session.info {
                if info.errorCount > 0 {
                    CountPill(count: info.errorCount, symbol: "xmark.circle.fill", color: .red) {
                        pane = .searchLog
                        session.searchLog.query = "$error"
                        session.searchLog.searchNow()
                    }
                }
                if info.warningCount > 0 {
                    CountPill(count: info.warningCount, symbol: "exclamationmark.triangle.fill", color: .yellow) {
                        pane = .searchLog
                        session.searchLog.query = "$warning"
                        session.searchLog.searchNow()
                    }
                }
            }

            Button {
                showingStats = true
            } label: {
                Label("Statistics", systemImage: "chart.bar")
            }
            .help("Binlog record statistics")

            Button {
                inspectorPresented.toggle()
            } label: {
                Label("Sources", systemImage: "sidebar.trailing")
            }
            .help("Toggle the source viewer")
        }
    }

    private var menuActions: NodeMenuActions {
        var actions = NodeMenuActions()
        actions.isFavorite = { session.favorites.isFavorite($0.id) }
        actions.toggleFavorite = { session.favorites.toggle($0) }
        actions.viewSource = { session.sources.openSource(for: $0) }
        actions.preprocess = { session.sources.openPreprocessed(for: $0) }
        actions.copySubtree = { summary in
            guard let engine = session.engine else { return }
            Task {
                if let text = try? await engine.subtreeText(of: summary.id) {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                }
            }
        }
        actions.searchInSubtree = { summary in
            pane = .searchLog
            session.searchLog.query = "under($\(summary.id)) "
        }
        actions.sortChildren = { node, mode in
            session.store?.setSortMode(mode, for: node)
        }
        return actions
    }

    private func revealFromPane(_ summary: NodeSummary) {
        session.requestReveal(nodeId: summary.id)
    }

    private func handleSelect(_ summary: NodeSummary?) {
        selectedNode = summary
        updateJumpBar(for: summary)

        if let summary, summary.nodeKind == .project || summary.nodeKind == .projectEvaluation {
            session.propertiesContext = summary
        }
    }

    private func handleDoubleClick(_ summary: NodeSummary) {
        if summary.canPreprocess && !summary.hasSource {
            session.sources.openPreprocessed(for: summary)
        } else if summary.hasSource {
            session.sources.openSource(for: summary)
        }
    }

    private func updateJumpBar(for summary: NodeSummary?) {
        guard let summary, let engine = session.engine else {
            jumpChain = []
            return
        }

        Task {
            if let ancestors = try? await engine.ancestors(of: summary.id) {
                if selectedNode?.id == summary.id {
                    jumpChain = ancestors.chain
                }
            }
        }
    }
}

struct CountPill: View {
    let count: Int
    let symbol: String
    let color: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 3) {
                Image(systemName: symbol)
                    .foregroundStyle(color)
                Text("\(count)")
                    .font(.caption.monospacedDigit())
            }
        }
        .help(symbol.contains("xmark") ? "Show all errors" : "Show all warnings")
    }
}

// MARK: - focused values for menu commands

public struct BuildSessionFocusedKey: FocusedValueKey {
    public typealias Value = BuildSession
}

public struct PaneSelectionFocusedKey: FocusedValueKey {
    public typealias Value = Binding<SidebarPane>
}

extension FocusedValues {
    public var buildSession: BuildSession? {
        get { self[BuildSessionFocusedKey.self] }
        set { self[BuildSessionFocusedKey.self] = newValue }
    }

    public var paneSelection: Binding<SidebarPane>? {
        get { self[PaneSelectionFocusedKey.self] }
        set { self[PaneSelectionFocusedKey.self] = newValue }
    }
}

/// App menu commands (File Reload, View panes, Help syntax).
public struct ViewerCommands: Commands {
    @FocusedValue(\.buildSession) private var session
    @FocusedValue(\.paneSelection) private var pane

    public init() {}

    public var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("Reload") {
                session?.reload()
            }
            .keyboardShortcut("r")
            .disabled(session == nil)
        }

        CommandMenu("Go") {
            ForEach(Array(SidebarPane.allCases.enumerated()), id: \.element.id) { index, target in
                Button(target.title) {
                    pane?.wrappedValue = target
                }
                .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: .command)
                .disabled(pane == nil)
            }
        }

        CommandGroup(replacing: .help) {
            Button("Search Syntax Reference") {
                SyntaxHelpWindow.show()
            }
        }
    }
}

/// Tiny AppKit-hosted window for the syntax reference from the Help menu.
@MainActor
enum SyntaxHelpWindow {
    private static var window: NSWindow?

    static func show() {
        if let window {
            window.makeKeyAndOrderFront(nil)
            return
        }

        let hosting = NSHostingController(rootView: SearchSyntaxHelpView())
        let newWindow = NSWindow(contentViewController: hosting)
        newWindow.title = "Search Syntax"
        newWindow.setContentSize(NSSize(width: 480, height: 560))
        newWindow.isReleasedWhenClosed = false
        newWindow.makeKeyAndOrderFront(nil)
        window = newWindow
    }
}
