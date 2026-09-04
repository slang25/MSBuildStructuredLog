import Foundation
import Observation

/// One open document-well tab: an embedded source file, a preprocessed
/// project, or generated text (e.g. a copied subtree).
public struct SourceTab: Identifiable, Equatable {
    public enum Kind: Equatable {
        case file
        case preprocessed
        case generated
    }

    public let id: String
    public let kind: Kind
    public let title: String
    public let path: String
    public var content: String

    /// Bumped each navigation so the editor re-scrolls even to the same line.
    public var gotoLine: Int?
    public var gotoToken: Int = 0

    /// The evaluation this tab's semantics are resolved under. One tab per
    /// path; the context is switchable, because the same .props file means
    /// something different in every project that imports it.
    public var evaluationId: String?

    /// Evaluations offered in the context picker (may be a capped subset of
    /// `contextsTotal`).
    public var contexts: [SemanticContext] = []
    public var contextsTotal: Int = 0

    /// Navigable spans joined to the build's import edges; nil until the
    /// engine answers (or when this tab isn't MSBuild XML).
    public var semantics: SourceSemanticIndex?

    /// End-of-element notes the editor draws past the text — currently the
    /// evaluated form of each skipped import's condition. Computed with
    /// `semantics`, so the two never disagree about the content they
    /// describe.
    public var annotations: [SourceAnnotation] = []

    public init(id: String, kind: Kind, title: String, path: String, content: String, gotoLine: Int? = nil) {
        self.id = id
        self.kind = kind
        self.title = title
        self.path = path
        self.content = content
        self.gotoLine = gotoLine
    }

    public var selectedContext: SemanticContext? {
        contexts.first { $0.evaluationId == evaluationId }
    }

    /// Whether this tab's text is worth running the MSBuild tokenizer over.
    /// Preprocessed output is excluded: its imports are inlined, so line
    /// numbers no longer line up with the build's import edges.
    public var isSemanticCandidate: Bool {
        guard kind == .file else { return false }
        let lowered = path.lowercased()
        for suffix in Self.msbuildExtensions where lowered.hasSuffix(suffix) {
            return true
        }
        return content.hasPrefix("<Project")
    }

    private static let msbuildExtensions = [
        ".csproj", ".vbproj", ".fsproj", ".vcxproj", ".esproj", ".shproj",
        ".props", ".targets", ".proj", ".tasks", ".overridetasks", ".pubxml",
    ]
}

/// What the quick-info popover renders for the token under the pointer.
public struct SemanticQuickInfo: Sendable, Equatable {
    public enum Body: Sendable, Equatable {
        case symbol(SemanticSymbol)
        case imports([SemanticLocation])

        /// An import the build evaluated and declined. Carries the reason
        /// and, for a false condition, what it expanded to.
        case skippedImports([SemanticSkippedImport])

        case unavailable(String)
    }

    /// The token as written, e.g. `$(OutputPath)` or `<Target Name="Build">`.
    public var title: String
    public var body: Body

    /// The evaluation the answer is scoped to, for the popover footer.
    public var contextLabel: String?

    public init(title: String, body: Body, contextLabel: String? = nil) {
        self.title = title
        self.body = body
        self.contextLabel = contextLabel
    }
}

/// The document well shown in the trailing inspector: closable tabs over
/// the source editor. Tabs are keyed by path+kind so re-opening the same
/// file re-activates (and re-scrolls) its tab.
@MainActor
@Observable
public final class SourceController {
    public private(set) var tabs: [SourceTab] = []
    public var selectedTabId: String?
    public private(set) var errorMessage: String?

    /// Toggles the inspector open; observed by the UI layer.
    public private(set) var presentationToken = 0

    public weak var engine: (any BinlogEngine)?

    /// Asks the main tree to reveal a node — wired to `BuildSession`.
    public var onReveal: ((String) -> Void)?

    private var semanticTasks: [String: Task<Void, Never>] = [:]

    public init() {}

    public func reset() {
        for task in semanticTasks.values {
            task.cancel()
        }
        semanticTasks = [:]
        tabs = []
        selectedTabId = nil
        errorMessage = nil
    }

    public var selectedTab: SourceTab? {
        guard let selectedTabId else { return nil }
        return tabs.first { $0.id == selectedTabId }
    }

    /// Opens the source for a node (error → file at line, project → its
    /// project file, import → the imported file...).
    public func openSource(for node: NodeSummary) {
        guard let engine else { return }
        Task { [weak self] in
            do {
                let location = try await engine.source(of: node.id)
                guard let text = location.text else {
                    self?.errorMessage = "'\(location.filePath)' is not embedded in this binlog."
                    return
                }
                self?.open(
                    kind: .file,
                    title: (location.filePath as NSString).lastPathComponent,
                    path: location.filePath,
                    content: text,
                    line: location.line)
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    /// - Parameter preferredEvaluationId: carried across a Cmd-click so that
    ///   following an import keeps the evaluation you were reading in; the
    ///   engine falls back to the file's own default if it doesn't apply.
    public func openFile(path: String, line: Int? = nil, preferredEvaluationId: String? = nil) {
        guard let engine else { return }
        Task { [weak self] in
            do {
                let text = try await engine.readFile(path: path)
                self?.open(
                    kind: .file,
                    title: (path as NSString).lastPathComponent,
                    path: path,
                    content: text,
                    line: line,
                    preferredEvaluationId: preferredEvaluationId)
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    public func openPreprocessed(for node: NodeSummary) {
        guard let engine else { return }
        Task { [weak self] in
            do {
                let text = try await engine.preprocess(node.id)
                let baseTitle = node.props?["projectFile"].map { ($0 as NSString).lastPathComponent }
                    ?? node.name ?? node.title
                self?.open(
                    kind: .preprocessed,
                    title: "\(baseTitle) (preprocessed)",
                    path: "preprocessed:\(node.id)",
                    content: text,
                    line: nil)
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    public func open(
        kind: SourceTab.Kind,
        title: String,
        path: String,
        content: String,
        line: Int?,
        preferredEvaluationId: String? = nil
    ) {
        let id = "\(kind):\(path)"
        var isNew = false
        if let index = tabs.firstIndex(where: { $0.id == id }) {
            tabs[index].content = content
            tabs[index].gotoLine = line
            tabs[index].gotoToken += 1
        } else {
            isNew = true
            tabs.append(SourceTab(id: id, kind: kind, title: title, path: path, content: content, gotoLine: line))
        }
        selectedTabId = id
        presentationToken += 1

        // Re-index only on first open: the same file re-opened at a different
        // line keeps whatever context the user chose.
        if isNew, tabs.last?.isSemanticCandidate == true {
            loadSemantics(tabId: id, evaluationId: preferredEvaluationId)
        }
    }

    public func close(tabId: String) {
        guard let index = tabs.firstIndex(where: { $0.id == tabId }) else { return }
        semanticTasks.removeValue(forKey: tabId)?.cancel()
        tabs.remove(at: index)
        if selectedTabId == tabId {
            selectedTabId = tabs.indices.contains(index) ? tabs[index].id : tabs.last?.id
        }
    }

    /// Asks the UI to reveal the document well without changing which tab
    /// is open — for commands that act on the editor and so need it on
    /// screen first.
    public func present() {
        guard selectedTabId != nil else { return }
        presentationToken += 1
    }

    public func clearError() {
        errorMessage = nil
    }

    // MARK: - semantics

    /// Re-resolves a tab under a different evaluation. Import edges are
    /// evaluation-scoped, so the whole index is rebuilt.
    public func selectContext(tabId: String, evaluationId: String) {
        guard let index = tabs.firstIndex(where: { $0.id == tabId }),
              tabs[index].evaluationId != evaluationId else { return }
        tabs[index].evaluationId = evaluationId
        loadSemantics(tabId: tabId, evaluationId: evaluationId)
    }

    /// Jumps the build tree to the evaluation a tab is scoped to.
    public func revealContext(_ evaluationId: String) {
        onReveal?(evaluationId)
    }

    private func loadSemantics(tabId: String, evaluationId: String?) {
        guard let engine, let tab = tabs.first(where: { $0.id == tabId }) else { return }

        semanticTasks[tabId]?.cancel()
        let path = tab.path
        let content = tab.content

        semanticTasks[tabId] = Task { [weak self] in
            do {
                let file = try await engine.semanticFile(path: path, evaluationId: evaluationId)
                try Task.checkCancellation()

                // Multi-MB build files: tokenize and anchor off the main actor.
                let (index, annotations) = await Task.detached(priority: .userInitiated) {
                    let index = SourceSemanticIndex(text: content, file: file)
                    let annotations = SourceAnnotations.importAnnotations(
                        text: content, skipped: index.skippedImports)
                    return (index, annotations)
                }.value
                try Task.checkCancellation()

                self?.apply(index: index, annotations: annotations, to: tabId)
            } catch is CancellationError {
            } catch EngineError.cancelled {
            } catch {
                // Semantics are an enhancement — a file that can't be indexed
                // still reads fine, so this must never raise an alert.
            }
        }
    }

    private func apply(
        index: SourceSemanticIndex,
        annotations: [SourceAnnotation],
        to tabId: String
    ) {
        guard let position = tabs.firstIndex(where: { $0.id == tabId }) else { return }
        tabs[position].semantics = index
        tabs[position].annotations = annotations
        tabs[position].evaluationId = index.file.evaluationId
        tabs[position].contexts = index.file.contexts ?? []
        tabs[position].contextsTotal = index.file.contextsTotal ?? tabs[position].contexts.count
    }

    // MARK: - navigation and quick info

    /// Resolves the token under the pointer for the quick-info popover.
    public func quickInfo(for token: MSBuildToken, in tabId: String) async -> SemanticQuickInfo? {
        guard let tab = tabs.first(where: { $0.id == tabId }), let index = tab.semantics else { return nil }
        let contextLabel = tab.selectedContext?.label
        let title = Self.title(for: token)

        guard let kind = index.symbolKind(for: token) else {
            switch index.importNavigation(for: token) {
            case .open(let location):
                return SemanticQuickInfo(title: title, body: .imports([location]), contextLabel: contextLabel)
            case .choose(let locations):
                return SemanticQuickInfo(title: title, body: .imports(locations), contextLabel: contextLabel)
            case .none(let reason):
                // A skip the build recorded explains itself far better than
                // the generic "nowhere to go" line.
                let skipped = index.skippedImports(for: token)
                return SemanticQuickInfo(
                    title: title,
                    body: skipped.isEmpty ? .unavailable(reason) : .skippedImports(skipped),
                    contextLabel: contextLabel)
            }
        }

        guard let engine, let evaluationId = tab.evaluationId else {
            return SemanticQuickInfo(
                title: title,
                body: .unavailable("No evaluation context for this file."),
                contextLabel: contextLabel)
        }

        do {
            let symbol = try await engine.semanticResolve(
                evaluationId: evaluationId, kind: kind, name: token.name)
            return SemanticQuickInfo(title: title, body: .symbol(symbol), contextLabel: contextLabel)
        } catch {
            return SemanticQuickInfo(
                title: title,
                body: .unavailable((error as? EngineError)?.message ?? error.localizedDescription),
                contextLabel: contextLabel)
        }
    }

    /// Cmd-click. Single destination jumps; several offer a picker via
    /// `onChoice`; nothing navigable reports why.
    public func navigate(
        token: MSBuildToken,
        in tabId: String,
        onChoice: @escaping ([SemanticLocation]) -> Void
    ) {
        guard let tab = tabs.first(where: { $0.id == tabId }), let index = tab.semantics else { return }

        guard let kind = index.symbolKind(for: token) else {
            handle(index.importNavigation(for: token), from: tab, onChoice: onChoice)
            return
        }

        guard let engine, let evaluationId = tab.evaluationId else { return }

        // Cmd-clicking a target's own <Target Name="..."> should go to where
        // it ran, not to the line already under the pointer.
        let preference: SourceSemanticIndex.NavigationPreference =
            token.kind == .targetDefinition ? .executions : .definitions

        Task { [weak self] in
            do {
                let symbol = try await engine.semanticResolve(
                    evaluationId: evaluationId, kind: kind, name: token.name)
                self?.handle(
                    SourceSemanticIndex.navigation(for: symbol, preferring: preference),
                    from: tab,
                    onChoice: onChoice)
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    private func handle(
        _ navigation: SemanticNavigation,
        from tab: SourceTab,
        onChoice: ([SemanticLocation]) -> Void
    ) {
        switch navigation {
        case .open(let location):
            go(to: location, from: tab)
        case .choose(let locations):
            onChoice(locations)
        case .none(let reason):
            errorMessage = reason
        }
    }

    /// Follows one destination: a source location opens a tab (keeping the
    /// current evaluation context where it applies), a node reveals in the
    /// build tree.
    public func go(to location: SemanticLocation, from tab: SourceTab? = nil) {
        if let path = location.path, location.available != false {
            openFile(
                path: path,
                line: location.line ?? 1,
                preferredEvaluationId: tab?.evaluationId ?? selectedTab?.evaluationId)
            return
        }

        if let nodeId = location.nodeId {
            onReveal?(nodeId)
            return
        }

        errorMessage = location.path.map { "'\($0)' is not embedded in this binlog." }
            ?? "This destination is not available."
    }

    static func title(for token: MSBuildToken) -> String {
        switch token.kind {
        case .property: return "$(\(token.name))"
        case .item: return "@(\(token.name))"
        case .targetDefinition, .targetReference: return "Target \(token.name)"
        case .importPath, .sdkReference: return token.name
        }
    }
}
