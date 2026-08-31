import Foundation
import Observation

/// Creates an engine for a binlog path, reporting load progress 0–1.
/// The app injects BinlogKit's opener; tests inject fakes.
public typealias EngineOpener = @Sendable (_ path: String, _ progress: @escaping @Sendable (Double) -> Void) async throws -> BinlogEngine

/// Top-level per-document state: owns the engine, tree store and the
/// sidebar controllers for one open binlog.
@MainActor
@Observable
public final class BuildSession {
    public enum Phase: Equatable {
        case idle
        case loading(Double)
        case loaded
        case failed(String)
    }

    public private(set) var phase: Phase = .idle
    public private(set) var path: String?
    public private(set) var info: BuildInfo?
    public private(set) var store: NodeStore?
    public private(set) var engine: BinlogEngine?

    public let searchLog: SearchController
    public let propertiesAndItems: SearchController
    public let findInFiles: FilesController
    public let sources: SourceController
    public let favorites: FavoritesStore

    /// Project/evaluation node scoping the Properties+Items pane.
    public var propertiesContext: NodeSummary? {
        didSet { propertiesAndItems.reset() }
    }

    /// Set by panes/tree code to ask the main tree to reveal a node.
    public var revealRequest: RevealRequest?

    public struct RevealRequest: Equatable {
        public let nodeId: String
        public let token: UUID

        public init(nodeId: String) {
            self.nodeId = nodeId
            self.token = UUID()
        }
    }

    private let opener: EngineOpener
    private var loadTask: Task<Void, Never>?

    public init(opener: @escaping EngineOpener) {
        self.opener = opener
        self.searchLog = SearchController(recentsKey: "recentSearches")
        self.propertiesAndItems = SearchController(recentsKey: "recentPropertySearches")
        self.findInFiles = FilesController()
        self.sources = SourceController()
        self.favorites = FavoritesStore()
    }

    public func load(path: String) {
        loadTask?.cancel()
        self.path = path
        phase = .loading(0)

        let opener = self.opener
        loadTask = Task { [weak self] in
            do {
                let engine = try await opener(path) { ratio in
                    Task { @MainActor [weak self] in
                        if case .loading = self?.phase {
                            self?.phase = .loading(ratio)
                        }
                    }
                }

                guard let self, !Task.isCancelled else {
                    await engine.close()
                    return
                }

                let details = try await engine.node(engine.info.rootId)
                self.attach(engine: engine, rootSummary: details.node)
            } catch is CancellationError {
                // superseded by a newer load
            } catch let error as EngineError {
                self?.phase = .failed(error.message)
            } catch {
                self?.phase = .failed(error.localizedDescription)
            }
        }
    }

    /// Wires up an already-open engine (used by load and by tests/spikes).
    public func attach(engine: BinlogEngine, rootSummary: NodeSummary) {
        self.engine = engine
        self.info = engine.info
        self.store = NodeStore(engine: engine, rootSummary: rootSummary)

        searchLog.performer = { [weak engine] query, max in
            guard let engine else { throw EngineError.badHandle }
            return try await engine.search(query: query, maxResults: max)
        }

        propertiesAndItems.performer = { [weak engine, weak self] query, max in
            guard let engine else { throw EngineError.badHandle }
            guard let context = await self?.propertiesContext else {
                throw EngineError.failure(
                    code: "NoContext",
                    message: "Select a project or evaluation to scope the search.")
            }
            return try await engine.searchPropertiesAndItems(
                contextId: context.id, query: query, maxResults: max)
        }

        findInFiles.engine = engine
        sources.engine = engine
        phase = .loaded
    }

    public func reload() {
        guard let path else { return }
        let oldEngine = engine
        engine = nil
        store = nil
        searchLog.reset()
        propertiesAndItems.reset()
        findInFiles.reset()
        sources.reset()
        favorites.clear()
        propertiesContext = nil
        Task { await oldEngine?.close() }
        load(path: path)
    }

    public func requestReveal(nodeId: String) {
        revealRequest = RevealRequest(nodeId: nodeId)
    }

    public func closeEngine() {
        loadTask?.cancel()
        let engine = self.engine
        self.engine = nil
        self.store = nil
        phase = .idle
        Task { await engine?.close() }
    }
}
