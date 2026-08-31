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

    public init(id: String, kind: Kind, title: String, path: String, content: String, gotoLine: Int? = nil) {
        self.id = id
        self.kind = kind
        self.title = title
        self.path = path
        self.content = content
        self.gotoLine = gotoLine
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

    public init() {}

    public func reset() {
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

    public func openFile(path: String, line: Int? = nil) {
        guard let engine else { return }
        Task { [weak self] in
            do {
                let text = try await engine.readFile(path: path)
                self?.open(
                    kind: .file,
                    title: (path as NSString).lastPathComponent,
                    path: path,
                    content: text,
                    line: line)
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

    public func open(kind: SourceTab.Kind, title: String, path: String, content: String, line: Int?) {
        let id = "\(kind):\(path)"
        if let index = tabs.firstIndex(where: { $0.id == id }) {
            tabs[index].content = content
            tabs[index].gotoLine = line
            tabs[index].gotoToken += 1
        } else {
            tabs.append(SourceTab(id: id, kind: kind, title: title, path: path, content: content, gotoLine: line))
        }
        selectedTabId = id
        presentationToken += 1
    }

    public func close(tabId: String) {
        guard let index = tabs.firstIndex(where: { $0.id == tabId }) else { return }
        tabs.remove(at: index)
        if selectedTabId == tabId {
            selectedTabId = tabs.indices.contains(index) ? tabs[index].id : tabs.last?.id
        }
    }

    public func clearError() {
        errorMessage = nil
    }
}
