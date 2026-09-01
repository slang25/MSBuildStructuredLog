import Foundation

/// Errors surfaced from the native bridge (or a mock standing in for it).
public enum EngineError: Error, Sendable, Equatable {
    case cancelled
    case badHandle
    case badNodeId(String)
    case failure(code: String, message: String)

    public var message: String {
        switch self {
        case .cancelled: return "Operation cancelled"
        case .badHandle: return "The build is no longer open"
        case .badNodeId(let detail): return detail
        case .failure(_, let message): return message
        }
    }
}

/// The seam between the view models and libmslog. `BinlogSession` (in
/// BinlogKit) is the real implementation; tests and perf spikes use an
/// in-memory fake. All calls are async: the real engine hops to a
/// background thread for every C call.
public protocol BinlogEngine: AnyObject, Sendable {
    var info: BuildInfo { get }

    func node(_ id: String) async throws -> NodeDetails
    func children(of id: String, offset: Int, count: Int, sortMode: ChildSortMode) async throws -> ChildrenPage
    func ancestors(of id: String) async throws -> Ancestors
    func subtreeText(of id: String) async throws -> String
    func source(of id: String) async throws -> SourceLocation
    func preprocess(_ id: String) async throws -> String

    /// Cancels any previous still-running search started via this method.
    func search(query: String, maxResults: Int) async throws -> SearchResponse
    func searchPropertiesAndItems(contextId: String, query: String, maxResults: Int) async throws -> SearchResponse

    /// File-scoped semantics for an open source file: the evaluations it
    /// participated in plus its import edges and target definitions. Pass a
    /// nil evaluation to take the default context.
    func semanticFile(path: String, evaluationId: String?) async throws -> SemanticFile

    /// Resolves one `$(property)`, `@(item)` or target name within an
    /// evaluation. Unknown names come back with `found == false`, not an error.
    func semanticResolve(evaluationId: String, kind: SemanticSymbolKind, name: String) async throws -> SemanticSymbol

    func listFiles() async throws -> FileList
    func readFile(path: String) async throws -> String
    func searchFiles(term: String, maxResults: Int) async throws -> FileSearchResponse

    func stats() async throws -> BuildStats
    func timeline() async throws -> BuildTimeline
    func projectGraph() async throws -> ProjectGraph

    /// Resolves a target row's parent-target navigation link.
    func parentTarget(of id: String) async throws -> NodeSummary

    /// Releases the underlying build. The engine is unusable afterwards.
    func close() async
}
