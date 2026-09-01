import Foundation

// Wire shapes for libmslog's semantic exports (mslog_semantic_file /
// mslog_semantic_resolve). See the "semantics" section of mslog.h.

public enum SemanticSymbolKind: String, Codable, Sendable {
    case property
    case item
    case target
}

/// One evaluation a source file participated in. MSBuild files are not
/// self-contained — the same Directory.Build.props is imported by many
/// projects and `$(OutputPath)` differs in each — so every semantic answer
/// is scoped to one of these.
public struct SemanticContext: Codable, Sendable, Equatable, Identifiable {
    public var evaluationId: String
    public var projectFile: String?
    public var label: String
    public var isProjectFile: Bool?

    public var id: String { evaluationId }

    public init(evaluationId: String, projectFile: String? = nil, label: String, isProjectFile: Bool? = nil) {
        self.evaluationId = evaluationId
        self.projectFile = projectFile
        self.label = label
        self.isProjectFile = isProjectFile
    }
}

/// An `<Import>` edge recorded by the build, anchored to a 1-based line in
/// the containing file. Implicit SDK imports report line 0 — there is no
/// element to point at — and are matched to `Sdk="..."` attributes instead.
public struct SemanticImport: Codable, Sendable, Equatable {
    public var line: Int
    public var column: Int
    public var importedPath: String
    public var available: Bool?

    public init(line: Int, column: Int = 0, importedPath: String, available: Bool? = nil) {
        self.line = line
        self.column = column
        self.importedPath = importedPath
        self.available = available
    }
}

public struct SemanticTargetDefinition: Codable, Sendable, Equatable {
    public var name: String
    public var line: Int

    public init(name: String, line: Int) {
        self.name = name
        self.line = line
    }
}

public struct SemanticFile: Codable, Sendable, Equatable {
    public var path: String?

    /// The context the imports below were resolved under.
    public var evaluationId: String?

    public var contexts: [SemanticContext]?

    /// Contexts before truncation; larger than `contexts.count` means the
    /// picker is showing a subset.
    public var contextsTotal: Int?

    public var imports: [SemanticImport]?
    public var targets: [SemanticTargetDefinition]?

    public init(
        path: String? = nil,
        evaluationId: String? = nil,
        contexts: [SemanticContext]? = nil,
        contextsTotal: Int? = nil,
        imports: [SemanticImport]? = nil,
        targets: [SemanticTargetDefinition]? = nil
    ) {
        self.path = path
        self.evaluationId = evaluationId
        self.contexts = contexts
        self.contextsTotal = contextsTotal
        self.imports = imports
        self.targets = targets
    }
}

/// A jump destination: a source location, a build-tree node, or both.
public struct SemanticLocation: Codable, Sendable, Equatable, Identifiable {
    public var path: String?
    public var line: Int?
    public var label: String?
    public var detail: String?
    public var nodeId: String?

    /// False when `path` can't be opened (not archived, not on disk).
    public var available: Bool?

    public var id: String {
        "\(path ?? "")|\(line ?? 0)|\(nodeId ?? "")|\(label ?? "")"
    }

    public init(
        path: String? = nil,
        line: Int? = nil,
        label: String? = nil,
        detail: String? = nil,
        nodeId: String? = nil,
        available: Bool? = nil
    ) {
        self.path = path
        self.line = line
        self.label = label
        self.detail = detail
        self.nodeId = nodeId
        self.available = available
    }
}

/// One label/value row of quick info.
public struct SemanticFact: Codable, Sendable, Equatable {
    public var label: String?
    public var value: String?

    public init(label: String? = nil, value: String? = nil) {
        self.label = label
        self.value = value
    }
}

public struct SemanticSymbol: Codable, Sendable, Equatable {
    public var kind: SemanticSymbolKind
    public var name: String

    /// False (not an error) when the evaluation has no such symbol.
    public var found: Bool

    public var value: String?

    /// Caveat worth showing under the value, e.g. that assignment locations
    /// are unavailable because the build ran without property tracking.
    public var note: String?

    public var definitions: [SemanticLocation]?

    /// Where a target actually ran; empty for properties and items.
    public var executions: [SemanticLocation]?

    public var facts: [SemanticFact]?

    public init(
        kind: SemanticSymbolKind,
        name: String,
        found: Bool,
        value: String? = nil,
        note: String? = nil,
        definitions: [SemanticLocation]? = nil,
        executions: [SemanticLocation]? = nil,
        facts: [SemanticFact]? = nil
    ) {
        self.kind = kind
        self.name = name
        self.found = found
        self.value = value
        self.note = note
        self.definitions = definitions
        self.executions = executions
        self.facts = facts
    }
}
