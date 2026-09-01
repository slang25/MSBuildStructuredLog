import Foundation

/// What a Cmd-click on a token should do.
public enum SemanticNavigation: Equatable, Sendable {
    /// Jump straight there — a single unambiguous destination.
    case open(SemanticLocation)

    /// Several candidates; the UI offers a picker.
    case choose([SemanticLocation])

    /// Nothing to navigate to; `reason` is user-facing.
    case none(reason: String)
}

/// The per-file semantic layer: `MSBuildTokenizer`'s lexical spans joined to
/// the import edges the build actually recorded.
///
/// Everything here is local and synchronous, so Cmd-hover never waits on the
/// engine. Only symbol resolution (`$(Prop)`, `@(Item)`, target names) needs a
/// round trip, because those answers are unbounded and evaluation-scoped.
public struct SourceSemanticIndex: Sendable, Equatable {
    /// Navigable spans, sorted by start offset. Import tokens with no
    /// recorded edge are dropped, so an underline always means something.
    public let tokens: [MSBuildToken]

    public let file: SemanticFile

    private let importsByLine: [Int: [SemanticImport]]

    /// Imports the build reported with no line number — the expansion of an
    /// `Sdk="..."` attribute, which has no `<Import>` element to point at.
    private let implicitImports: [SemanticImport]

    public var evaluationId: String? { file.evaluationId }

    public init(text: String, file: SemanticFile) {
        self.file = file

        var byLine: [Int: [SemanticImport]] = [:]
        var implicit: [SemanticImport] = []
        for edge in file.imports ?? [] {
            if edge.line > 0 {
                byLine[edge.line, default: []].append(edge)
            } else {
                implicit.append(edge)
            }
        }
        self.importsByLine = byLine
        self.implicitImports = implicit

        let scanned = MSBuildTokenizer.tokenize(text)
        self.tokens = scanned.filter { token in
            switch token.kind {
            case .importPath:
                return byLine[token.line] != nil
            case .sdkReference:
                return byLine[token.line] != nil || !implicit.isEmpty
            default:
                return true
            }
        }
    }

    /// The token containing `offset`, if any.
    public func token(at offset: Int) -> MSBuildToken? {
        var low = 0
        var high = tokens.count - 1
        while low <= high {
            let mid = (low + high) / 2
            let range = tokens[mid].range
            if offset < range.location {
                high = mid - 1
            } else if offset >= NSMaxRange(range) {
                low = mid + 1
            } else {
                return tokens[mid]
            }
        }
        return nil
    }

    /// The symbol kind to ask the engine about, or nil when the token
    /// resolves locally from the recorded import edges.
    public func symbolKind(for token: MSBuildToken) -> SemanticSymbolKind? {
        switch token.kind {
        case .property: return .property
        case .item: return .item
        case .targetReference, .targetDefinition: return .target
        case .importPath, .sdkReference: return nil
        }
    }

    /// Import destinations, resolved locally from the build's import edges.
    public func importNavigation(for token: MSBuildToken) -> SemanticNavigation {
        var edges = importsByLine[token.line] ?? []
        if edges.isEmpty, token.kind == .sdkReference {
            edges = implicitImports
        }

        let locations = edges.map { edge in
            SemanticLocation(
                path: edge.importedPath,
                line: 1,
                label: (edge.importedPath as NSString).lastPathComponent,
                detail: edge.importedPath,
                available: edge.available)
        }

        let reachable = locations.filter { $0.available != false }
        if reachable.isEmpty {
            return .none(reason: locations.isEmpty
                ? "This import was not recorded in the build."
                : "'\(locations[0].detail ?? "")' is not embedded in this binlog.")
        }

        return reachable.count == 1 ? .open(reachable[0]) : .choose(reachable)
    }

    /// Which half of a resolved symbol a Cmd-click should follow.
    public enum NavigationPreference: Sendable {
        /// Where the symbol is written down. What you want from a reference.
        case definitions

        /// Where it actually ran. What you want from `<Target Name="X">` —
        /// jumping to the definition you are already looking at is useless,
        /// and source→log is the jump no editor can offer.
        case executions
    }

    public static func navigation(
        for symbol: SemanticSymbol,
        preferring preference: NavigationPreference = .definitions
    ) -> SemanticNavigation {
        guard symbol.found else {
            return .none(reason: "No \(symbol.kind.rawValue) '\(symbol.name)' in this evaluation.")
        }

        let definitions = (symbol.definitions ?? []).filter { $0.available != false || $0.nodeId != nil }
        let executions = symbol.executions ?? []

        let ordered = preference == .executions
            ? [executions, definitions]
            : [definitions, executions]

        for candidates in ordered where !candidates.isEmpty {
            return candidates.count == 1 ? .open(candidates[0]) : .choose(candidates)
        }

        if preference == .executions, symbol.executions?.isEmpty != false {
            return .none(reason: "'\(symbol.name)' never ran in this evaluation.")
        }

        return .none(reason: symbol.note
            ?? "'\(symbol.name)' has no recorded definition in this evaluation.")
    }
}
