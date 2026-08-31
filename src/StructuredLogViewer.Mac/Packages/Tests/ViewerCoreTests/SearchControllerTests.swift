import XCTest
@testable import ViewerCore

@MainActor
final class SearchControllerTests: XCTestCase {
    private func makeController() -> SearchController {
        let controller = SearchController(recentsKey: "test.recents.\(UUID().uuidString)")
        controller.debounce = { _ in } // no real sleeping in tests
        return controller
    }

    nonisolated private static func response(_ query: String, count: Int = 1) -> SearchResponse {
        SearchResponse(query: query, resultCount: count, overflow: false, elapsedMs: 1, roots: [])
    }

    func testExecutabilityRules() {
        XCTAssertFalse(SearchController.isExecutable(""))
        XCTAssertFalse(SearchController.isExecutable("ab"))
        XCTAssertTrue(SearchController.isExecutable("abc"))
        XCTAssertTrue(SearchController.isExecutable("$e"))
        XCTAssertFalse(SearchController.isExecutable("   "))
    }

    func testShortQueryClearsResults() async throws {
        let controller = makeController()
        controller.performer = { query, _ in Self.response(query) }

        controller.query = "$error"
        try await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(controller.response?.query, "$error")

        controller.query = "ab"
        XCTAssertNil(controller.response)
        XCTAssertFalse(controller.isSearching)
    }

    func testTypingCancelsPreviousSearch() async throws {
        let controller = makeController()
        let started = SendableBox<[String]>([])
        controller.performer = { query, _ in
            started.withLock { $0.append(query) }
            if query == "slow query" {
                try await Task.sleep(nanoseconds: 5_000_000_000)
            }
            return Self.response(query)
        }

        controller.query = "slow query"
        try await Task.sleep(nanoseconds: 50_000_000)
        controller.query = "fast query"
        try await Task.sleep(nanoseconds: 200_000_000)

        XCTAssertEqual(controller.response?.query, "fast query")
        XCTAssertFalse(controller.isSearching)
    }

    func testRecentsAreRecordedAndDeduplicated() async throws {
        let controller = makeController()
        controller.performer = { query, _ in Self.response(query) }

        controller.query = "$error"
        try await Task.sleep(nanoseconds: 100_000_000)
        controller.query = "$warning"
        try await Task.sleep(nanoseconds: 100_000_000)
        controller.query = "$error"
        try await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(controller.recentSearches.first, "$error")
        XCTAssertEqual(controller.recentSearches.filter { $0 == "$error" }.count, 1)
    }

    func testErrorSurfaced() async throws {
        let controller = makeController()
        controller.performer = { _, _ in
            throw EngineError.failure(code: "Boom", message: "query exploded")
        }

        controller.query = "$error"
        try await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(controller.errorMessage, "query exploded")
        XCTAssertFalse(controller.isSearching)
    }
}

final class SendableBox<Value>: @unchecked Sendable {
    private var value: Value
    private let lock = NSLock()

    init(_ value: Value) {
        self.value = value
    }

    func withLock<R>(_ body: (inout Value) -> R) -> R {
        lock.lock()
        defer { lock.unlock() }
        return body(&value)
    }
}
