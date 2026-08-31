import Foundation

// These types are both ViewerCore's domain model and the exact JSON wire
// shape produced by libmslog (camelCase, null-omitting) — BinlogKit
// decodes bridge payloads straight into them.

public enum NodeState: String, Codable, Sendable {
    case succeeded, failed, skipped, none
}

public enum ChildSortMode: Int, Codable, Sendable {
    case natural = 0
    case byName = 1
    case byDuration = 2
}

public struct NodeSummary: Codable, Sendable, Equatable {
    public var id: String
    public var kind: String
    public var title: String
    public var name: String?
    public var value: String?
    public var hasChildren: Bool
    public var childCount: Int
    public var isLowRelevance: Bool
    public var state: NodeState
    public var durationMs: Double?
    public var hasSource: Bool
    public var canPreprocess: Bool
    public var childIndex: Int?
    public var props: [String: String]?

    public init(
        id: String,
        kind: String,
        title: String,
        name: String? = nil,
        value: String? = nil,
        hasChildren: Bool = false,
        childCount: Int = 0,
        isLowRelevance: Bool = false,
        state: NodeState = .none,
        durationMs: Double? = nil,
        hasSource: Bool = false,
        canPreprocess: Bool = false,
        childIndex: Int? = nil,
        props: [String: String]? = nil
    ) {
        self.id = id
        self.kind = kind
        self.title = title
        self.name = name
        self.value = value
        self.hasChildren = hasChildren
        self.childCount = childCount
        self.isLowRelevance = isLowRelevance
        self.state = state
        self.durationMs = durationMs
        self.hasSource = hasSource
        self.canPreprocess = canPreprocess
        self.childIndex = childIndex
        self.props = props
    }
}

public struct NodeDetails: Codable, Sendable {
    public var node: NodeSummary
    public var parentId: String?
    public var startTime: String?
    public var endTime: String?
    public var fullText: String?
    public var sourceFile: String?
    public var sourceLine: Int?
}

public struct ChildrenPage: Codable, Sendable {
    public var parentId: String
    public var total: Int
    public var offset: Int
    public var count: Int
    public var sortMode: Int
    public var children: [NodeSummary]

    public init(parentId: String, total: Int, offset: Int, count: Int, sortMode: Int, children: [NodeSummary]) {
        self.parentId = parentId
        self.total = total
        self.offset = offset
        self.count = count
        self.sortMode = sortMode
        self.children = children
    }
}

public struct Ancestors: Codable, Sendable {
    /// Root first, the requested node itself last.
    public var chain: [NodeSummary]

    public init(chain: [NodeSummary]) {
        self.chain = chain
    }
}

public struct Highlight: Codable, Sendable, Equatable {
    public var text: String
    public var isHighlight: Bool?
    public var style: String?

    public init(text: String, isHighlight: Bool? = nil, style: String? = nil) {
        self.text = text
        self.isHighlight = isHighlight
        self.style = style
    }
}

public struct SearchTreeNode: Codable, Sendable {
    public var node: NodeSummary?
    public var text: String?
    public var highlights: [Highlight]?
    public var children: [SearchTreeNode]?

    public init(node: NodeSummary? = nil, text: String? = nil, highlights: [Highlight]? = nil, children: [SearchTreeNode]? = nil) {
        self.node = node
        self.text = text
        self.highlights = highlights
        self.children = children
    }
}

public struct SearchResponse: Codable, Sendable {
    public var query: String
    public var resultCount: Int
    public var overflow: Bool
    public var elapsedMs: Double
    public var roots: [SearchTreeNode]

    public init(query: String, resultCount: Int, overflow: Bool, elapsedMs: Double, roots: [SearchTreeNode]) {
        self.query = query
        self.resultCount = resultCount
        self.overflow = overflow
        self.elapsedMs = elapsedMs
        self.roots = roots
    }
}

public struct SourceLocation: Codable, Sendable {
    public var filePath: String
    public var line: Int?
    public var text: String?

    public init(filePath: String, line: Int? = nil, text: String? = nil) {
        self.filePath = filePath
        self.line = line
        self.text = text
    }
}

public struct FileEntry: Codable, Sendable, Equatable {
    public var path: String
    public var lines: Int
    public var length: Int

    public init(path: String, lines: Int, length: Int) {
        self.path = path
        self.lines = lines
        self.length = length
    }
}

public struct FileList: Codable, Sendable {
    public var total: Int
    public var files: [FileEntry]

    public init(total: Int, files: [FileEntry]) {
        self.total = total
        self.files = files
    }
}

public struct FileMatchSpan: Codable, Sendable, Equatable {
    public var start: Int
    public var length: Int

    public init(start: Int, length: Int) {
        self.start = start
        self.length = length
    }
}

public struct FileMatch: Codable, Sendable {
    public var line: Int
    public var text: String
    public var spans: [FileMatchSpan]?

    public init(line: Int, text: String, spans: [FileMatchSpan]? = nil) {
        self.line = line
        self.text = text
        self.spans = spans
    }
}

public struct FileMatches: Codable, Sendable {
    public var path: String
    public var matches: [FileMatch]

    public init(path: String, matches: [FileMatch]) {
        self.path = path
        self.matches = matches
    }
}

public struct FileSearchResponse: Codable, Sendable {
    public var query: String
    public var totalMatches: Int
    public var overflow: Bool?
    public var files: [FileMatches]

    public init(query: String, totalMatches: Int, overflow: Bool? = nil, files: [FileMatches]) {
        self.query = query
        self.totalMatches = totalMatches
        self.overflow = overflow
        self.files = files
    }
}

public struct BuildInfo: Codable, Sendable {
    public var rootId: String
    public var succeeded: Bool
    public var errorCount: Int
    public var warningCount: Int
    public var nodeCount: Int
    public var hasSourceArchive: Bool
    public var msBuildVersion: String?
    public var filePath: String
    public var fileSize: Int64
    public var durationMs: Double
    public var startTime: String?
    public var endTime: String?

    public init(
        rootId: String,
        succeeded: Bool,
        errorCount: Int,
        warningCount: Int,
        nodeCount: Int,
        hasSourceArchive: Bool,
        msBuildVersion: String? = nil,
        filePath: String,
        fileSize: Int64,
        durationMs: Double,
        startTime: String? = nil,
        endTime: String? = nil
    ) {
        self.rootId = rootId
        self.succeeded = succeeded
        self.errorCount = errorCount
        self.warningCount = warningCount
        self.nodeCount = nodeCount
        self.hasSourceArchive = hasSourceArchive
        self.msBuildVersion = msBuildVersion
        self.filePath = filePath
        self.fileSize = fileSize
        self.durationMs = durationMs
        self.startTime = startTime
        self.endTime = endTime
    }
}

public struct StatsRecord: Codable, Sendable {
    public var type: String
    public var totalLength: Int64
    public var count: Int
    public var largest: Int
    public var children: [StatsRecord]?
}

public struct BuildStats: Codable, Sendable {
    public var fileSize: Int64
    public var uncompressedStreamSize: Int64
    public var recordCount: Int64
    public var fileFormatVersion: Int
    public var stringCount: Int
    public var stringTotalSize: Int64
    public var stringLargest: Int
    public var nameValueListCount: Int
    public var nameValueListTotalSize: Int64
    public var nameValueListLargest: Int
    public var blobCount: Int
    public var blobTotalSize: Int64
    public var blobLargest: Int
    public var records: StatsRecord?
}

public struct BridgeErrorPayload: Codable, Sendable {
    public var code: String
    public var message: String
}
