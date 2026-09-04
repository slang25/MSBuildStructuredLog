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

/// An `<Import>` element the build evaluated and declined to follow.
/// Anchored to the same 1-based coordinates as `SemanticImport`, so the two
/// join against the tokenizer's spans the same way.
public struct SemanticSkippedImport: Codable, Sendable, Equatable {
    public var line: Int
    public var column: Int

    /// The `Project` attribute as logged — usually still unexpanded, e.g.
    /// `$(CustomBeforeDirectoryBuildProps)`.
    public var fileSpec: String?

    /// Prose reason, e.g. "Not imported due to no matching files".
    public var reason: String?

    /// The `Condition` attribute, when a false condition is why it was
    /// skipped. Nil for a missing file, an empty expression or an
    /// unresolved SDK — `reason` covers those.
    public var condition: String?

    /// What `condition` expanded to at the moment MSBuild evaluated it,
    /// e.g. `'' != ''`. Set together with `condition`.
    public var evaluatedCondition: String?

    public init(
        line: Int,
        column: Int = 0,
        fileSpec: String? = nil,
        reason: String? = nil,
        condition: String? = nil,
        evaluatedCondition: String? = nil
    ) {
        self.line = line
        self.column = column
        self.fileSpec = fileSpec
        self.reason = reason
        self.condition = condition
        self.evaluatedCondition = evaluatedCondition
    }

    /// True when MSBuild told us the condition and what it expanded to.
    public var hasCondition: Bool { condition != nil && evaluatedCondition != nil }

    /// The full explanation, for quick info and error text.
    public var explanation: String {
        guard let condition, let evaluatedCondition else {
            return reason ?? "Not imported."
        }
        return "Condition \(condition) evaluated as \(evaluatedCondition) → false"
    }

    /// The terse form for an end-of-element annotation in the editor. The
    /// condition itself is already on screen — what's missing is what it
    /// expanded to, so only that and the verdict are worth the pixels.
    public var annotation: String {
        guard let evaluatedCondition else {
            return reason ?? "Not imported."
        }
        return "\(evaluatedCondition) → false"
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

    /// Imports in this file the build evaluated and declined.
    public var skippedImports: [SemanticSkippedImport]?

    public var targets: [SemanticTargetDefinition]?

    public init(
        path: String? = nil,
        evaluationId: String? = nil,
        contexts: [SemanticContext]? = nil,
        contextsTotal: Int? = nil,
        imports: [SemanticImport]? = nil,
        skippedImports: [SemanticSkippedImport]? = nil,
        targets: [SemanticTargetDefinition]? = nil
    ) {
        self.path = path
        self.evaluationId = evaluationId
        self.contexts = contexts
        self.contextsTotal = contextsTotal
        self.imports = imports
        self.skippedImports = skippedImports
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
