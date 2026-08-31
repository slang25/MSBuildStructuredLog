import Foundation
import Observation

/// Debounced, cancel-previous search driving the Search Log and
/// Properties+Items panes. The `performer` closure runs the actual query
/// (the engine auto-cancels the previous native search).
@MainActor
@Observable
public final class SearchController {
    public static let defaultMaxResults = 500
    public static let debounceMilliseconds = 300
    public static let minimumQueryLength = 3

    public var query: String = "" {
        didSet {
            if query != oldValue {
                scheduleSearch()
            }
        }
    }

    public private(set) var response: SearchResponse?
    public private(set) var isSearching = false
    public private(set) var errorMessage: String?
    public private(set) var recentSearches: [String] = []
    public var maxResults = SearchController.defaultMaxResults

    /// Set by BuildSession when the engine attaches.
    public var performer: (@Sendable (_ query: String, _ maxResults: Int) async throws -> SearchResponse)?

    /// Injected clock for tests; production uses real sleeping.
    var debounce: @MainActor (_ milliseconds: Int) async throws -> Void = { ms in
        try await Task.sleep(nanoseconds: UInt64(ms) * 1_000_000)
    }

    private var searchTask: Task<Void, Never>?
    private let recentsKey: String
    private let recentsLimit = 20

    public init(recentsKey: String = "recentSearches") {
        self.recentsKey = recentsKey
        self.recentSearches = UserDefaults.standard.stringArray(forKey: recentsKey) ?? []
    }

    /// A query is executable once it's 3+ characters, or shorter when it
    /// clearly targets the DSL (starts with `$`).
    public static func isExecutable(_ query: String) -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return false }
        return trimmed.count >= minimumQueryLength || trimmed.hasPrefix("$")
    }

    public func reset() {
        searchTask?.cancel()
        searchTask = nil
        response = nil
        isSearching = false
        errorMessage = nil
    }

    /// Runs the current query immediately (Enter key / "show more").
    public func searchNow() {
        scheduleSearch(debounced: false)
    }

    public func commitToRecents() {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        recentSearches.removeAll { $0 == trimmed }
        recentSearches.insert(trimmed, at: 0)
        if recentSearches.count > recentsLimit {
            recentSearches.removeLast(recentSearches.count - recentsLimit)
        }
        UserDefaults.standard.set(recentSearches, forKey: recentsKey)
    }

    public func clearRecents() {
        recentSearches = []
        UserDefaults.standard.removeObject(forKey: recentsKey)
    }

    private func scheduleSearch(debounced: Bool = true) {
        searchTask?.cancel()

        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard Self.isExecutable(trimmed) else {
            response = nil
            isSearching = false
            errorMessage = nil
            return
        }

        guard let performer else { return }

        isSearching = true
        errorMessage = nil
        let max = maxResults
        let debounce = self.debounce

        searchTask = Task { [weak self] in
            do {
                if debounced {
                    try await debounce(Self.debounceMilliseconds)
                }
                try Task.checkCancellation()
                let result = try await performer(trimmed, max)
                try Task.checkCancellation()
                guard let self else { return }
                self.response = result
                self.isSearching = false
                self.commitToRecents()
            } catch is CancellationError {
                // superseded
            } catch EngineError.cancelled {
                // superseded natively
            } catch let error as EngineError {
                self?.errorMessage = error.message
                self?.isSearching = false
            } catch {
                self?.errorMessage = error.localizedDescription
                self?.isSearching = false
            }
        }
    }
}
