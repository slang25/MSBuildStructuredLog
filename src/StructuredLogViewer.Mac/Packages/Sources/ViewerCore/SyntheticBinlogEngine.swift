import Foundation

/// In-memory fake engine generating a huge deterministic tree — used by
/// the NSOutlineView perf spike and by unit tests. Node ids follow the
/// bridge's path scheme rooted at "0" ("0", "0/3", "0/3/7", ...), each
/// depth-limited node fanning out `fanout` children.
open class SyntheticBinlogEngine: BinlogEngine, @unchecked Sendable {
    public let fanout: Int
    public let depth: Int
    public let latencyNanoseconds: UInt64
    public let info: BuildInfo

    public init(fanout: Int = 50, depth: Int = 4, latencyNanoseconds: UInt64 = 0) {
        self.fanout = fanout
        self.depth = depth
        self.latencyNanoseconds = latencyNanoseconds

        var count = 1
        var levelSize = 1
        for _ in 0..<depth {
            levelSize *= fanout
            count += levelSize
        }

        self.info = BuildInfo(
            rootId: "0",
            succeeded: true,
            errorCount: 0,
            warningCount: 0,
            nodeCount: count,
            hasSourceArchive: false,
            filePath: "synthetic://\(fanout)^\(depth)",
            fileSize: 0,
            durationMs: 1234)
    }

    private func level(of id: String) -> Int {
        id == "0" ? 0 : id.split(separator: "/").count - 1
    }

    open func summary(for id: String) -> NodeSummary {
        let nodeLevel = level(of: id)
        let hasChildren = nodeLevel < depth
        return NodeSummary(
            id: id,
            kind: nodeLevel == 0 ? "Build" : (nodeLevel == 1 ? "Project" : (nodeLevel == 2 ? "Target" : "Message")),
            title: "Node \(id)",
            hasChildren: hasChildren,
            childCount: hasChildren ? fanout : 0,
            state: .none)
    }

    private func pause() async throws {
        if latencyNanoseconds > 0 {
            try await Task.sleep(nanoseconds: latencyNanoseconds)
        }
    }

    open func node(_ id: String) async throws -> NodeDetails {
        try await pause()
        var details = NodeDetails(node: summary(for: id))
        details.fullText = "Node \(id)"
        return details
    }

    open func children(of id: String, offset: Int, count: Int, sortMode: ChildSortMode) async throws -> ChildrenPage {
        try await pause()
        let parent = summary(for: id)
        guard parent.hasChildren else {
            return ChildrenPage(parentId: id, total: 0, offset: 0, count: 0, sortMode: sortMode.rawValue, children: [])
        }

        let end = min(offset + count, fanout)
        var children: [NodeSummary] = []
        for i in offset..<max(offset, end) {
            var child = summary(for: "\(id)/\(i)")
            child.childIndex = i
            children.append(child)
        }

        return ChildrenPage(
            parentId: id, total: fanout, offset: offset,
            count: children.count, sortMode: sortMode.rawValue, children: children)
    }

    open func ancestors(of id: String) async throws -> Ancestors {
        try await pause()
        var chain: [NodeSummary] = []
        let parts = id.split(separator: "/").map(String.init)
        guard parts.first == "0" else { throw EngineError.badNodeId("Unknown node \(id)") }

        var current = "0"
        chain.append(summary(for: current))
        for part in parts.dropFirst() {
            current += "/\(part)"
            var element = summary(for: current)
            element.childIndex = Int(part)
            chain.append(element)
        }

        return Ancestors(chain: chain)
    }

    open func subtreeText(of id: String) async throws -> String { "Node \(id)" }

    open func source(of id: String) async throws -> SourceLocation {
        throw EngineError.failure(code: "NoSource", message: "Synthetic nodes have no source.")
    }

    open func preprocess(_ id: String) async throws -> String {
        throw EngineError.failure(code: "NoPreprocess", message: "Synthetic nodes cannot be preprocessed.")
    }

    open func search(query: String, maxResults: Int) async throws -> SearchResponse {
        try await pause()
        return SearchResponse(query: query, resultCount: 0, overflow: false, elapsedMs: 0, roots: [])
    }

    open func searchPropertiesAndItems(contextId: String, query: String, maxResults: Int) async throws -> SearchResponse {
        try await search(query: query, maxResults: maxResults)
    }

    open func listFiles() async throws -> FileList { FileList(total: 0, files: []) }

    open func readFile(path: String) async throws -> String {
        throw EngineError.failure(code: "NoFile", message: "No embedded files.")
    }

    open func searchFiles(term: String, maxResults: Int) async throws -> FileSearchResponse {
        FileSearchResponse(query: term, totalMatches: 0, files: [])
    }

    open func stats() async throws -> BuildStats {
        throw EngineError.failure(code: "NoStats", message: "No stats for synthetic builds.")
    }

    open func timeline() async throws -> BuildTimeline {
        // A small deterministic two-lane timeline for previews/tests.
        var lanes: [TimelineLane] = []
        for laneId in 0..<2 {
            var blocks: [TimelineBlock] = []
            for i in 0..<8 {
                let start = Double(i) * 120 + Double(laneId) * 40
                blocks.append(TimelineBlock(
                    id: "0/\(laneId)/\(i)",
                    kind: i % 3 == 0 ? "Project" : (i % 3 == 1 ? "Target" : "Task"),
                    text: "Block \(laneId).\(i)",
                    start: start,
                    end: start + 100,
                    indent: i % 3))
            }
            lanes.append(TimelineLane(nodeId: laneId, maxIndent: 2, blocks: blocks))
        }
        return BuildTimeline(durationMs: 1000, lanes: lanes)
    }

    open func close() async {}
}
