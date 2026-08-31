import Foundation

/// Computes flame-chart nesting for one timeline lane from block time
/// spans alone. Filtering blocks out (e.g. hiding targets) therefore
/// re-compacts the chart instead of leaving empty indent levels.
public enum TimelineNesting {
    public struct Placed: Equatable {
        public let block: TimelineBlock
        public let indent: Int

        public init(block: TimelineBlock, indent: Int) {
            self.block = block
            self.indent = indent
        }
    }

    /// Containment rank for same-start ordering: containers first.
    public static func rank(ofKind kind: String) -> Int {
        switch kind {
        case "ProjectEvaluation", "Project": return 0
        case "Target": return 1
        case "Task": return 2
        default: return 3
        }
    }

    /// Sorts blocks (start ascending; ties: containers first, longer
    /// first) and assigns each an indent = number of still-open blocks.
    public static func place(_ blocks: [TimelineBlock]) -> (rows: [Placed], maxIndent: Int) {
        let epsilon = 0.0005

        let sorted = blocks.sorted { a, b in
            if a.start != b.start { return a.start < b.start }
            let ra = rank(ofKind: a.kind)
            let rb = rank(ofKind: b.kind)
            if ra != rb { return ra < rb }
            return a.end > b.end
        }

        var rows: [Placed] = []
        rows.reserveCapacity(sorted.count)
        var openEnds: [Double] = []
        var maxIndent = 0

        for block in sorted {
            while let top = openEnds.last, top <= block.start + epsilon {
                openEnds.removeLast()
            }

            let indent = openEnds.count
            maxIndent = max(maxIndent, indent)
            rows.append(Placed(block: block, indent: indent))
            openEnds.append(block.end)
        }

        return (rows, maxIndent)
    }
}
