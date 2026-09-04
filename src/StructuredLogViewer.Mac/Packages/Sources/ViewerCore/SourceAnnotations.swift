import Foundation

/// A short note the editor draws after a span of source, in the margin past
/// the end of the line. Not part of the text storage — offsets stay valid
/// for the tokenizer, the find bar and the line-number ruler.
public struct SourceAnnotation: Equatable, Sendable {
    /// UTF-16 offset the note is drawn after, matching NSTextView.
    public var offset: Int

    public var text: String

    /// 1-based line the anchor falls on; only used for diagnostics.
    public var line: Int

    public init(offset: Int, text: String, line: Int) {
        self.offset = offset
        self.text = text
        self.line = line
    }
}

/// Turns the build's skipped-import records into end-of-element annotations.
///
/// MSBuild records the evaluated form of an import's `Condition` — the one
/// condition evaluation a binlog contains. Showing it next to the element it
/// belongs to turns "why is this import not happening?" from a tree hunt into
/// a glance.
public enum SourceAnnotations {
    /// Annotations for `skipped`, anchored just past the `>` that closes each
    /// import element. Records whose position doesn't land inside `text` are
    /// dropped rather than guessed at.
    public static func importAnnotations(
        text: String,
        skipped: [SemanticSkippedImport]
    ) -> [SourceAnnotation] {
        guard !skipped.isEmpty else { return [] }

        let scalars = Array(text.utf16)
        let lineStarts = MSBuildTokenizer.lineStarts(scalars)

        // Several records can share one element — `Project="a;b"` skips each
        // half separately — so collect by anchor and join.
        var textsByAnchor: [Int: [String]] = [:]
        var order: [Int] = []

        for record in skipped where record.line > 0 {
            guard let start = offset(line: record.line, column: record.column, in: lineStarts, scalars: scalars),
                  let anchor = endOfTag(from: start, in: scalars) else {
                continue
            }

            if textsByAnchor[anchor] == nil {
                order.append(anchor)
                textsByAnchor[anchor] = []
            }

            let note = record.annotation
            if !textsByAnchor[anchor]!.contains(note) {
                textsByAnchor[anchor]!.append(note)
            }
        }

        return order.sorted().map { anchor in
            SourceAnnotation(
                offset: anchor,
                text: textsByAnchor[anchor]!.joined(separator: " · "),
                line: MSBuildTokenizer.line(at: anchor, in: lineStarts))
        }
    }

    /// Where to draw a note horizontally, or nil when there is no room for
    /// it. Preference is just after the element; when that would run past
    /// the trailing margin the note slides left to sit against it, because
    /// the editor wraps rather than scrolling horizontally and anything past
    /// the edge is simply invisible. A note that would then collide with the
    /// code is dropped — overlapping text is worse than a missing hint, and
    /// the same fact is still on the element's hover.
    public static func placement(
        elementEnd: CGFloat,
        noteWidth: CGFloat,
        trailingMargin: CGFloat,
        inset: CGFloat,
        minimumGap: CGFloat
    ) -> CGFloat? {
        let x = min(elementEnd + inset, trailingMargin - noteWidth)
        return x >= elementEnd + minimumGap ? x : nil
    }

    /// UTF-16 offset for a 1-based line/column. Falls back to the start of
    /// the line when the column is missing or past its end — MSBuild columns
    /// are reliable, but a preprocessed or re-wrapped file may not agree.
    static func offset(
        line: Int,
        column: Int,
        in lineStarts: [Int],
        scalars: [UInt16]
    ) -> Int? {
        guard line >= 1, line <= lineStarts.count else { return nil }
        let start = lineStarts[line - 1]
        guard column > 1 else { return start }

        let lineEnd = line < lineStarts.count ? lineStarts[line] : scalars.count
        let candidate = start + column - 1
        return candidate < lineEnd ? candidate : start
    }

    /// The offset just past the `>` closing the tag that starts at or after
    /// `start`, ignoring `>` inside attribute values.
    static func endOfTag(from start: Int, in scalars: [UInt16]) -> Int? {
        let lt = UInt16(UnicodeScalar("<").value)
        let gt = UInt16(UnicodeScalar(">").value)
        let quote = UInt16(UnicodeScalar("\"").value)
        let apostrophe = UInt16(UnicodeScalar("'").value)

        // MSBuild points at the `<`; a fallback to the line start may leave
        // us on leading whitespace, so walk on to the tag.
        var i = start
        while i < scalars.count, scalars[i] != lt {
            // Never cross into the next line looking for one.
            if scalars[i] == UInt16(0x0A) { return nil }
            i += 1
        }
        guard i < scalars.count else { return nil }

        var delimiter: UInt16?
        while i < scalars.count {
            let c = scalars[i]
            if let open = delimiter {
                if c == open { delimiter = nil }
            } else if c == quote || c == apostrophe {
                delimiter = c
            } else if c == gt {
                return i + 1
            }
            i += 1
        }

        return nil
    }
}
