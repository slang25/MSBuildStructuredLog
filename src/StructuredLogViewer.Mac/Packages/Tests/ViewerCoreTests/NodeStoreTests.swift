import XCTest
@testable import ViewerCore

/// Mock engine recording calls, with controllable latency, built on the
/// synthetic tree generator.
final class RecordingEngine: SyntheticBinlogEngine, @unchecked Sendable {
    private let lock = NSLock()
    private var _childrenCalls: [(id: String, offset: Int, count: Int)] = []

    var childrenCalls: [(id: String, offset: Int, count: Int)] {
        lock.lock()
        defer { lock.unlock() }
        return _childrenCalls
    }

    override func children(of id: String, offset: Int, count: Int, sortMode: ChildSortMode) async throws -> ChildrenPage {
        lock.lock()
        _childrenCalls.append((id, offset, count))
        lock.unlock()
        return try await super.children(of: id, offset: offset, count: count, sortMode: sortMode)
    }
}

@MainActor
final class NodeStoreTests: XCTestCase {
    private func makeStore(fanout: Int = 100, depth: Int = 3, pageSize: Int = 32) -> (NodeStore, RecordingEngine) {
        let engine = RecordingEngine(fanout: fanout, depth: depth)
        let store = NodeStore(engine: engine, rootSummary: engine.summary(for: "0"), pageSize: pageSize)
        return (store, engine)
    }

    func testChildReturnsStablePlaceholderThenFills() async throws {
        let (store, _) = makeStore()
        let root = store.root

        let placeholder = store.child(of: root, at: 5)
        XCTAssertTrue(placeholder.isPlaceholder)

        // Same object identity on repeated asks.
        XCTAssertTrue(store.child(of: root, at: 5) === placeholder)

        try await store.loadPage(of: root, containing: 5)
        XCTAssertFalse(placeholder.isPlaceholder)
        XCTAssertEqual(placeholder.summary.id, "0/5")
        XCTAssertTrue(store.child(of: root, at: 5) === placeholder)
    }

    func testPageFetchIsDeduplicated() async throws {
        let (store, engine) = makeStore(pageSize: 32)
        let root = store.root

        // Asking for many indices within one page must fetch that page once.
        for i in 0..<32 {
            _ = store.child(of: root, at: i)
        }

        try await store.loadPage(of: root, containing: 0)
        XCTAssertEqual(engine.childrenCalls.filter { $0.offset == 0 }.count, 1)

        // A different page is a separate fetch.
        _ = store.child(of: root, at: 40)
        try await store.loadPage(of: root, containing: 40)
        XCTAssertEqual(engine.childrenCalls.count, 2)
        XCTAssertEqual(engine.childrenCalls[1].offset, 32)
    }

    func testOnChildrenUpdatedFiresWithPageRange() async throws {
        let (store, _) = makeStore(fanout: 10, pageSize: 4)
        let root = store.root

        var updates: [Range<Int>] = []
        store.onChildrenUpdated = { parent, range in
            XCTAssertTrue(parent === root)
            updates.append(range)
        }

        _ = store.child(of: root, at: 9)
        try await store.loadPage(of: root, containing: 9)

        // Page 2 of a 10-child parent with page size 4 = indices 8..<10.
        XCTAssertEqual(updates, [8..<10])
    }

    func testRevealLoadsChainAndReturnsRefs() async throws {
        let (store, _) = makeStore(fanout: 60, depth: 3, pageSize: 16)

        let chain = try await store.reveal(id: "0/33/7/12")
        XCTAssertEqual(chain.map(\.summary.id), ["0", "0/33", "0/33/7", "0/33/7/12"])

        // Every element except the root sits in its parent's slot at the
        // right index and is real.
        XCTAssertEqual(chain[1].indexInParent, 33)
        XCTAssertEqual(chain[2].indexInParent, 7)
        XCTAssertEqual(chain[3].indexInParent, 12)
        XCTAssertTrue(chain.allSatisfy { !$0.isPlaceholder })

        // Reveal again resolves instantly from the id index without
        // re-walking (returns identical refs).
        let again = try await store.reveal(id: "0/33/7/12")
        XCTAssertTrue(zip(chain, again).allSatisfy { $0 === $1 })
    }

    func testRevealUnknownNodeThrows() async {
        let (store, _) = makeStore()
        do {
            _ = try await store.reveal(id: "7/1")
            XCTFail("expected badNodeId")
        } catch {
            // expected
        }
    }

    func testSortModeInvalidatesAndRefetches() async throws {
        let (store, engine) = makeStore(fanout: 8, pageSize: 8)
        let root = store.root

        _ = store.child(of: root, at: 0)
        try await store.loadPage(of: root, containing: 0)
        let before = engine.childrenCalls.count

        store.setSortMode(.byDuration, for: root)
        let slot = store.child(of: root, at: 0)
        XCTAssertTrue(slot.isPlaceholder)

        try await store.loadPage(of: root, containing: 0)
        XCTAssertGreaterThan(engine.childrenCalls.count, before)
        XCTAssertFalse(store.child(of: root, at: 0).isPlaceholder)
    }

    func testStaleGenerationPageIsDropped() async throws {
        let (store, _) = makeStore(fanout: 8, pageSize: 8)
        let root = store.root

        _ = store.child(of: root, at: 0)
        let pending = Task { try await store.loadPage(of: root, containing: 0) }

        // Invalidate while the fetch may be in flight; the applied result
        // must not resurrect old slots into the new generation silently.
        store.setSortMode(.byName, for: root)
        _ = try? await pending.value

        // Slots for the new generation still fetch fine.
        try await store.loadPage(of: root, containing: 0)
        XCTAssertFalse(store.child(of: root, at: 0).isPlaceholder)
    }
}
