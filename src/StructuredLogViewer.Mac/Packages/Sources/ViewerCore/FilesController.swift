import Foundation
import Observation

/// Drives the Files pane (embedded archive listing + filter) and the
/// Find in Files pane (content search).
@MainActor
@Observable
public final class FilesController {
    public private(set) var allFiles: [FileEntry] = []
    public private(set) var isLoadingList = false

    public var pathFilter: String = ""

    public var filteredFiles: [FileEntry] {
        let trimmed = pathFilter.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return allFiles }
        return allFiles.filter { $0.path.localizedCaseInsensitiveContains(trimmed) }
    }

    public var findTerm: String = "" {
        didSet {
            if findTerm != oldValue {
                scheduleFind()
            }
        }
    }

    public private(set) var findResults: FileSearchResponse?
    public private(set) var isSearching = false
    public private(set) var errorMessage: String?

    public weak var engine: (any BinlogEngine)? {
        didSet {
            if engine != nil {
                loadListIfNeeded()
            }
        }
    }

    var debounce: @MainActor (_ milliseconds: Int) async throws -> Void = { ms in
        try await Task.sleep(nanoseconds: UInt64(ms) * 1_000_000)
    }

    private var listTask: Task<Void, Never>?
    private var findTask: Task<Void, Never>?

    public init() {}

    public func reset() {
        listTask?.cancel()
        findTask?.cancel()
        allFiles = []
        findResults = nil
        isSearching = false
        isLoadingList = false
        errorMessage = nil
    }

    public func loadListIfNeeded() {
        guard allFiles.isEmpty, !isLoadingList, let engine else { return }
        isLoadingList = true
        listTask = Task { [weak self] in
            do {
                let list = try await engine.listFiles()
                self?.allFiles = list.files
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
            }
            self?.isLoadingList = false
        }
    }

    private func scheduleFind() {
        findTask?.cancel()
        let trimmed = findTerm.trimmingCharacters(in: .whitespaces)
        guard trimmed.count >= SearchController.minimumQueryLength, let engine else {
            findResults = nil
            isSearching = false
            return
        }

        isSearching = true
        errorMessage = nil
        let debounce = self.debounce

        findTask = Task { [weak self] in
            do {
                try await debounce(SearchController.debounceMilliseconds)
                try Task.checkCancellation()
                let results = try await engine.searchFiles(term: trimmed, maxResults: 1000)
                try Task.checkCancellation()
                self?.findResults = results
                self?.isSearching = false
            } catch is CancellationError {
            } catch EngineError.cancelled {
            } catch {
                self?.errorMessage = (error as? EngineError)?.message ?? error.localizedDescription
                self?.isSearching = false
            }
        }
    }
}
