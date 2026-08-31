import XCTest
@testable import ViewerCore

final class TimelineNestingTests: XCTestCase {
    private func block(_ id: String, _ kind: String, _ start: Double, _ end: Double) -> TimelineBlock {
        TimelineBlock(id: id, kind: kind, start: start, end: end)
    }

    private func indents(_ rows: [TimelineNesting.Placed]) -> [String: Int] {
        Dictionary(uniqueKeysWithValues: rows.map { ($0.block.id, $0.indent) })
    }

    func testContainmentNests() {
        let (rows, maxIndent) = TimelineNesting.place([
            block("p", "Project", 0, 100),
            block("t", "Target", 10, 90),
            block("k", "Task", 20, 80),
        ])

        let byId = indents(rows)
        XCTAssertEqual(byId["p"], 0)
        XCTAssertEqual(byId["t"], 1)
        XCTAssertEqual(byId["k"], 2)
        XCTAssertEqual(maxIndent, 2)
    }

    func testSiblingsShareLevel() {
        let (rows, maxIndent) = TimelineNesting.place([
            block("p", "Project", 0, 100),
            block("t1", "Target", 0, 50),
            block("t2", "Target", 50, 100), // starts exactly where t1 ends
        ])

        let byId = indents(rows)
        XCTAssertEqual(byId["p"], 0)
        XCTAssertEqual(byId["t1"], 1)
        XCTAssertEqual(byId["t2"], 1)
        XCTAssertEqual(maxIndent, 1)
    }

    func testFilteringCompactsLevels() {
        // With the target hidden, the task should sit directly under the
        // project instead of leaving a gap at its old level.
        let all = [
            block("p", "Project", 0, 100),
            block("k", "Task", 20, 80),
        ]

        let (rows, maxIndent) = TimelineNesting.place(all)
        XCTAssertEqual(indents(rows)["k"], 1)
        XCTAssertEqual(maxIndent, 1)
    }

    func testSameStartOrdersContainersFirst() {
        // Task, target and project all start at 0; the project must claim
        // level 0, target level 1, task level 2 regardless of input order.
        let (rows, _) = TimelineNesting.place([
            block("k", "Task", 0, 30),
            block("p", "Project", 0, 100),
            block("t", "Target", 0, 50),
        ])

        let byId = indents(rows)
        XCTAssertEqual(byId["p"], 0)
        XCTAssertEqual(byId["t"], 1)
        XCTAssertEqual(byId["k"], 2)
    }

    func testSameStartSameKindLongerFirst() {
        let (rows, _) = TimelineNesting.place([
            block("short", "Target", 0, 20),
            block("long", "Target", 0, 60),
        ])

        let byId = indents(rows)
        XCTAssertEqual(byId["long"], 0)
        XCTAssertEqual(byId["short"], 1)
    }

    func testSequentialBlocksStayFlat() {
        let (rows, maxIndent) = TimelineNesting.place((0..<10).map {
            block("b\($0)", "Task", Double($0) * 10, Double($0) * 10 + 9)
        })

        XCTAssertEqual(maxIndent, 0)
        XCTAssertTrue(rows.allSatisfy { $0.indent == 0 })
    }
}
