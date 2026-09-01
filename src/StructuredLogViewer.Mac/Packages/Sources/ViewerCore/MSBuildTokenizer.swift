import Foundation

/// A navigable span of MSBuild source: the lexical half of the semantic
/// layer. Ranges are UTF-16, matching NSTextView.
public struct MSBuildToken: Equatable, Sendable {
    public enum Kind: String, Equatable, Sendable {
        /// `$(Name)` — including the identifier part of `$(Name.Method())`.
        case property

        /// `@(Name)`.
        case item

        /// The `Name` attribute value of a `<Target>` element.
        case targetDefinition

        /// One entry of DependsOnTargets / BeforeTargets / AfterTargets / …
        case targetReference

        /// The `Project` attribute value of an `<Import>` element.
        case importPath

        /// An `Sdk="..."` attribute. MSBuild logs the imports it expands to
        /// with no line number, so these resolve differently from `<Import>`.
        case sdkReference
    }

    public var kind: Kind
    public var name: String
    public var range: NSRange

    /// 1-based line the token starts on; import edges are matched by line.
    public var line: Int

    public init(kind: Kind, name: String, range: NSRange, line: Int) {
        self.kind = kind
        self.name = name
        self.range = range
        self.line = line
    }
}

/// Splits MSBuild XML into navigable tokens with a single linear scan — no
/// XML parse, so it survives malformed and preprocessed content just like
/// `XMLHighlighter`. Purely lexical: it says *what* the cursor is over, and
/// the engine says what that means in a given evaluation.
public enum MSBuildTokenizer {
    /// Attributes whose value is a `;`-separated list of target names.
    private static let targetListAttributes: Set<String> = [
        "dependsontargets",
        "beforetargets",
        "aftertargets",
        "initialtargets",
        "defaulttargets",
        "targets",
    ]

    public static func tokenize(_ text: String) -> [MSBuildToken] {
        let scalars = Array(text.utf16)
        let lineStarts = Self.lineStarts(scalars)
        var scanner = Scanner(scalars: scalars, lineStarts: lineStarts)
        scanner.run()

        // Lookup is a binary search over start offsets, so order matters more
        // than the scan's incidental emission order.
        return scanner.tokens.sorted { $0.range.location < $1.range.location }
    }

    /// Offsets at which each line begins; index + 1 is the 1-based line.
    static func lineStarts(_ scalars: [UInt16]) -> [Int] {
        var starts: [Int] = [0]
        for index in scalars.indices where scalars[index] == UInt16(0x0A) {
            starts.append(index + 1)
        }
        return starts
    }

    static func line(at offset: Int, in lineStarts: [Int]) -> Int {
        var low = 0
        var high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= offset { low = mid } else { high = mid - 1 }
        }
        return low + 1
    }

    private struct Scanner {
        let scalars: [UInt16]
        let lineStarts: [Int]
        var tokens: [MSBuildToken] = []

        private let lt = UInt16(UnicodeScalar("<").value)
        private let gt = UInt16(UnicodeScalar(">").value)
        private let slash = UInt16(UnicodeScalar("/").value)
        private let bang = UInt16(UnicodeScalar("!").value)
        private let question = UInt16(UnicodeScalar("?").value)
        private let dash = UInt16(UnicodeScalar("-").value)
        private let equals = UInt16(UnicodeScalar("=").value)
        private let quote = UInt16(UnicodeScalar("\"").value)
        private let apostrophe = UInt16(UnicodeScalar("'").value)
        private let dollar = UInt16(UnicodeScalar("$").value)
        private let at = UInt16(UnicodeScalar("@").value)
        private let openParen = UInt16(UnicodeScalar("(").value)
        private let semicolon = UInt16(UnicodeScalar(";").value)

        init(scalars: [UInt16], lineStarts: [Int]) {
            self.scalars = scalars
            self.lineStarts = lineStarts
        }

        mutating func run() {
            var i = 0
            let n = scalars.count
            while i < n {
                if scalars[i] == lt {
                    if isCommentStart(i) {
                        i = skipComment(from: i)
                        continue
                    }
                    if i + 1 < n, scalars[i + 1] == bang || scalars[i + 1] == question {
                        i = skipTo(gt, from: i) + 1
                        continue
                    }
                    i = scanTag(from: i)
                    continue
                }

                // Text content between elements can still hold expressions.
                if let token = expression(at: i) {
                    tokens.append(token)
                    i = NSMaxRange(token.range)
                    continue
                }

                i += 1
            }
        }

        // MARK: - tags

        /// Scans one element from its `<` and returns the offset just past it.
        private mutating func scanTag(from start: Int) -> Int {
            let n = scalars.count
            var i = start + 1
            if i < n, scalars[i] == slash { i += 1 }

            let nameStart = i
            while i < n, isNameChar(scalars[i]) { i += 1 }
            let element = string(nameStart, i).lowercased()

            while i < n, scalars[i] != gt {
                if isWhitespace(scalars[i]) || scalars[i] == slash {
                    i += 1
                    continue
                }

                let attributeStart = i
                while i < n, isNameChar(scalars[i]) { i += 1 }
                guard i > attributeStart else {
                    // Not a name character and not whitespace — step over it
                    // rather than spin (malformed markup).
                    i += 1
                    continue
                }

                let attribute = string(attributeStart, i).lowercased()

                while i < n, isWhitespace(scalars[i]) { i += 1 }
                guard i < n, scalars[i] == equals else { continue }
                i += 1
                while i < n, isWhitespace(scalars[i]) { i += 1 }
                guard i < n, scalars[i] == quote || scalars[i] == apostrophe else { continue }

                let delimiter = scalars[i]
                i += 1
                let valueStart = i
                while i < n, scalars[i] != delimiter, scalars[i] != lt { i += 1 }
                let valueEnd = i
                if i < n, scalars[i] == delimiter { i += 1 }

                scanAttributeValue(
                    element: element,
                    attribute: attribute,
                    from: valueStart,
                    to: valueEnd)
            }

            return i < n ? i + 1 : n
        }

        private mutating func scanAttributeValue(
            element: String,
            attribute: String,
            from start: Int,
            to end: Int
        ) {
            guard end > start else { return }

            var local: [MSBuildToken] = []

            // Expressions win over everything: a target list entry that is
            // really `$(BuildDependsOn)` is a property reference, not a
            // target named "$(BuildDependsOn)".
            var i = start
            while i < end {
                if let token = expression(at: i, limit: end) {
                    local.append(token)
                    i = NSMaxRange(token.range)
                    continue
                }
                i += 1
            }

            let hasExpression = !local.isEmpty

            if element == "target", attribute == "name", !hasExpression {
                local.append(token(.targetDefinition, from: start, to: end))
            } else if targetListAttributes.contains(attribute) {
                appendTargetReferences(from: start, to: end, into: &local)
            } else if let kind = importKind(element: element, attribute: attribute) {
                // The whole attribute is one link. Any `$(...)` inside it is
                // dropped rather than left overlapping: the recorded edge
                // already names the file the expression expanded to, so the
                // import is the more useful thing to click.
                local.removeAll()
                local.append(token(kind, from: start, to: end))
            }

            local.sort { $0.range.location < $1.range.location }
            tokens.append(contentsOf: local)
        }

        private func importKind(element: String, attribute: String) -> MSBuildToken.Kind? {
            switch (element, attribute) {
            case ("import", "project"): return .importPath
            case ("import", "sdk"), ("project", "sdk"), ("sdk", "name"): return .sdkReference
            default: return nil
            }
        }

        /// Splits a `;`-separated target list, skipping entries that overlap
        /// an expression (already emitted as property/item tokens).
        private mutating func appendTargetReferences(
            from start: Int,
            to end: Int,
            into local: inout [MSBuildToken]
        ) {
            let expressions = local
            var segmentStart = start
            var i = start
            while i <= end {
                if i == end || scalars[i] == semicolon {
                    var from = segmentStart
                    var to = i
                    while from < to, isWhitespace(scalars[from]) { from += 1 }
                    while to > from, isWhitespace(scalars[to - 1]) { to -= 1 }

                    let overlaps = expressions.contains { expression in
                        expression.range.location < to && NSMaxRange(expression.range) > from
                    }

                    if to > from, !overlaps {
                        local.append(token(.targetReference, from: from, to: to))
                    }

                    segmentStart = i + 1
                }
                i += 1
            }
        }

        // MARK: - expressions

        /// `$(Name` / `@(Name` at `offset`, as a token over just the name.
        private func expression(at offset: Int, limit: Int? = nil) -> MSBuildToken? {
            let end = limit ?? scalars.count
            guard offset + 2 < end else { return nil }
            let prefix = scalars[offset]
            guard prefix == dollar || prefix == at else { return nil }
            guard scalars[offset + 1] == openParen else { return nil }

            var i = offset + 2
            while i < end, isIdentifierChar(scalars[i]) { i += 1 }
            guard i > offset + 2 else { return nil }

            return token(prefix == dollar ? .property : .item, from: offset + 2, to: i)
        }

        // MARK: - helpers

        private func token(_ kind: MSBuildToken.Kind, from start: Int, to end: Int) -> MSBuildToken {
            MSBuildToken(
                kind: kind,
                name: string(start, end),
                range: NSRange(location: start, length: end - start),
                line: MSBuildTokenizer.line(at: start, in: lineStarts))
        }

        private func string(_ start: Int, _ end: Int) -> String {
            guard end > start else { return "" }
            return String(decoding: scalars[start..<end], as: UTF16.self)
        }

        private func isCommentStart(_ i: Int) -> Bool {
            i + 3 < scalars.count
                && scalars[i + 1] == bang
                && scalars[i + 2] == dash
                && scalars[i + 3] == dash
        }

        private func skipComment(from start: Int) -> Int {
            var i = start + 4
            let n = scalars.count
            while i + 2 < n {
                if scalars[i] == dash, scalars[i + 1] == dash, scalars[i + 2] == gt {
                    return i + 3
                }
                i += 1
            }
            return n
        }

        private func skipTo(_ character: UInt16, from start: Int) -> Int {
            var i = start
            let n = scalars.count
            while i < n, scalars[i] != character { i += 1 }
            return min(i, n)
        }

        /// Property and item names: `$(Foo.Substring(0,1))` is a call on the
        /// property `Foo`, and `@(Files->'%(Filename)')` is a transform of
        /// `Files` — so `.` and `-` end the identifier.
        private func isIdentifierChar(_ c: UInt16) -> Bool {
            (c >= 0x41 && c <= 0x5A)        // A-Z
                || (c >= 0x61 && c <= 0x7A) // a-z
                || (c >= 0x30 && c <= 0x39) // 0-9
                || c == 0x5F                // _
        }

        /// XML element and attribute names, which do allow `.`, `-` and `:`.
        private func isNameChar(_ c: UInt16) -> Bool {
            (c >= 0x41 && c <= 0x5A)       // A-Z
                || (c >= 0x61 && c <= 0x7A) // a-z
                || (c >= 0x30 && c <= 0x39) // 0-9
                || c == 0x5F                // _
                || c == 0x2D                // -
                || c == 0x2E                // .
                || c == 0x3A                // :
        }

        private func isWhitespace(_ c: UInt16) -> Bool {
            c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D
        }
    }
}
