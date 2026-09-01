import AppKit

/// Hand-rolled MSBuild-XML syntax highlighting producing an
/// NSAttributedString. Single linear scan, no XML parse — robust against
/// malformed/preprocessed content and fast enough for multi-MB files.
///
/// The palette is built around what you actually read in a build file:
/// element and attribute names carry the structure, `$(…)` / `@(…)` / `%(…)`
/// expressions carry the logic, and everything else — angle brackets, quotes,
/// equals signs, comments — is deliberately quiet so it stays out of the way.
public enum XMLHighlighter {
    public struct Palette {
        /// Text between elements.
        public let text: NSColor

        /// `<`, `>`, `/`, `=` and attribute quotes.
        public let punctuation: NSColor

        public let elementName: NSColor
        public let attributeName: NSColor
        public let attributeValue: NSColor
        public let comment: NSColor

        /// `&amp;` and friends.
        public let entity: NSColor

        /// `$(Property)`, `@(Item)`, `%(Metadata)` — including inside strings,
        /// which is where most of an MSBuild file's meaning lives.
        public let expression: NSColor

        public init(
            text: NSColor,
            punctuation: NSColor,
            elementName: NSColor,
            attributeName: NSColor,
            attributeValue: NSColor,
            comment: NSColor,
            entity: NSColor,
            expression: NSColor
        ) {
            self.text = text
            self.punctuation = punctuation
            self.elementName = elementName
            self.attributeName = attributeName
            self.attributeValue = attributeValue
            self.comment = comment
            self.entity = entity
            self.expression = expression
        }

        /// Five clearly separated hues — green elements, purple attribute
        /// names, blue strings, orange expressions, cyan entities — over grey
        /// punctuation and comments. Follows the appearance, so one attributed
        /// string is correct in both light and dark without re-highlighting.
        public static let standard = Palette(
            text: dynamic(light: 0x1F2328, dark: 0xC9D1D9),
            punctuation: dynamic(light: 0x8C959F, dark: 0x6E7681),
            elementName: dynamic(light: 0x116329, dark: 0x7EE787),
            attributeName: dynamic(light: 0x6639BA, dark: 0xD2A8FF),
            attributeValue: dynamic(light: 0x0A63C7, dark: 0x79C0FF),
            comment: dynamic(light: 0x6E7781, dark: 0x8B949E),
            entity: dynamic(light: 0x0F6E6E, dark: 0x56D4DD),
            expression: dynamic(light: 0x953800, dark: 0xFFA657))

        private static func dynamic(light: UInt32, dark: UInt32) -> NSColor {
            NSColor(name: nil) { appearance in
                appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                    ? rgb(dark)
                    : rgb(light)
            }
        }

        private static func rgb(_ hex: UInt32) -> NSColor {
            NSColor(
                srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
                green: CGFloat((hex >> 8) & 0xFF) / 255,
                blue: CGFloat(hex & 0xFF) / 255,
                alpha: 1)
        }
    }

    public static let font = NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular)

    public static func highlight(_ text: String, palette: Palette = .standard) -> NSAttributedString {
        let attributed = NSMutableAttributedString(string: text, attributes: [
            .font: font,
            .foregroundColor: palette.text,
        ])

        let scanner = Scanner(scalars: Array(text.utf16), palette: palette, attributed: attributed)
        scanner.run()
        return attributed
    }

    private struct Scanner {
        let scalars: [UInt16]
        let palette: Palette
        let attributed: NSMutableAttributedString

        /// Bounds the search for an expression's closing paren, so a stray
        /// `$(` can't repaint the rest of the document.
        private static let maxExpressionLength = 500

        /// Bounds an entity, so a bare `&` doesn't either.
        private static let maxEntityLength = 12

        private let lt = char("<")
        private let gt = char(">")
        private let slash = char("/")
        private let bang = char("!")
        private let question = char("?")
        private let dash = char("-")
        private let equals = char("=")
        private let quote = char("\"")
        private let apostrophe = char("'")
        private let ampersand = char("&")
        private let semicolon = char(";")
        private let dollar = char("$")
        private let at = char("@")
        private let percent = char("%")
        private let openParen = char("(")
        private let closeParen = char(")")
        private let newline = char("\n")

        func run() {
            var i = 0
            let n = scalars.count
            while i < n {
                let c = scalars[i]

                if c == lt {
                    if isCommentStart(i) {
                        let end = endOfComment(from: i)
                        paint(palette.comment, i, end)
                        i = end
                        continue
                    }

                    // <?xml …?>, <!DOCTYPE …>, <![CDATA[ … — structure, not content.
                    if i + 1 < n, scalars[i + 1] == bang || scalars[i + 1] == question {
                        let end = min(indexOf(gt, from: i) + 1, n)
                        paint(palette.punctuation, i, end)
                        i = end
                        continue
                    }

                    i = scanTag(from: i)
                    continue
                }

                if c == ampersand, let end = endOfEntity(from: i) {
                    paint(palette.entity, i, end)
                    i = end
                    continue
                }

                if let end = endOfExpression(from: i, limit: n) {
                    paint(palette.expression, i, end)
                    i = end
                    continue
                }

                i += 1
            }
        }

        // MARK: - elements

        /// Paints one element from its `<` and returns the offset just past it.
        private func scanTag(from start: Int) -> Int {
            let n = scalars.count
            var i = start + 1
            if i < n, scalars[i] == slash { i += 1 }
            paint(palette.punctuation, start, i)

            let nameStart = i
            while i < n, isNameChar(scalars[i]) { i += 1 }
            paint(palette.elementName, nameStart, i)

            while i < n, scalars[i] != gt {
                if isWhitespace(scalars[i]) {
                    i += 1
                    continue
                }

                if scalars[i] == slash {
                    paint(palette.punctuation, i, i + 1)
                    i += 1
                    continue
                }

                let attributeStart = i
                while i < n, isNameChar(scalars[i]) { i += 1 }
                guard i > attributeStart else {
                    // Neither a name character nor whitespace — step over it
                    // rather than spin (malformed markup).
                    i += 1
                    continue
                }
                paint(palette.attributeName, attributeStart, i)

                while i < n, isWhitespace(scalars[i]) { i += 1 }
                guard i < n, scalars[i] == equals else { continue }
                paint(palette.punctuation, i, i + 1)
                i += 1

                while i < n, isWhitespace(scalars[i]) { i += 1 }
                guard i < n, scalars[i] == quote || scalars[i] == apostrophe else { continue }

                let delimiter = scalars[i]
                paint(palette.punctuation, i, i + 1)
                i += 1

                let valueStart = i
                while i < n, scalars[i] != delimiter, scalars[i] != lt { i += 1 }
                paintAttributeValue(from: valueStart, to: i)

                if i < n, scalars[i] == delimiter {
                    paint(palette.punctuation, i, i + 1)
                    i += 1
                }
            }

            if i < n {
                paint(palette.punctuation, i, i + 1)
                return i + 1
            }

            return n
        }

        /// Values are strings, except for the expressions inside them — which
        /// is the part worth seeing in `Condition="'$(X)' != ''"`.
        private func paintAttributeValue(from start: Int, to end: Int) {
            guard end > start else { return }
            paint(palette.attributeValue, start, end)

            var i = start
            while i < end {
                if let expressionEnd = endOfExpression(from: i, limit: end) {
                    paint(palette.expression, i, expressionEnd)
                    i = expressionEnd
                    continue
                }
                i += 1
            }
        }

        // MARK: - runs

        private func isCommentStart(_ i: Int) -> Bool {
            i + 3 < scalars.count
                && scalars[i + 1] == bang
                && scalars[i + 2] == dash
                && scalars[i + 3] == dash
        }

        private func endOfComment(from start: Int) -> Int {
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

        private func endOfEntity(from start: Int) -> Int? {
            var i = start + 1
            let n = scalars.count
            while i < n, i - start < Self.maxEntityLength {
                if scalars[i] == semicolon { return i + 1 }
                if !isNameChar(scalars[i]), scalars[i] != char("#") { return nil }
                i += 1
            }
            return nil
        }

        /// `$(…)`, `@(…)` or `%(…)` starting at `start`, paren-balanced so
        /// nested property functions stay in one piece. Bails at a line break
        /// or a `<`, which an unterminated expression would otherwise swallow.
        private func endOfExpression(from start: Int, limit: Int) -> Int? {
            guard start + 2 < limit else { return nil }
            let prefix = scalars[start]
            guard prefix == dollar || prefix == at || prefix == percent else { return nil }
            guard scalars[start + 1] == openParen else { return nil }

            var depth = 1
            var i = start + 2
            while i < limit, i - start < Self.maxExpressionLength {
                let c = scalars[i]
                if c == newline || c == lt { return nil }
                if c == openParen {
                    depth += 1
                } else if c == closeParen {
                    depth -= 1
                    if depth == 0 { return i + 1 }
                }
                i += 1
            }

            return nil
        }

        // MARK: - helpers

        private func paint(_ color: NSColor, _ start: Int, _ end: Int) {
            guard end > start else { return }
            attributed.addAttribute(
                .foregroundColor, value: color, range: NSRange(location: start, length: end - start))
        }

        private func indexOf(_ character: UInt16, from start: Int) -> Int {
            var i = start
            let n = scalars.count
            while i < n, scalars[i] != character { i += 1 }
            return min(i, n)
        }

        /// XML element and attribute names.
        private func isNameChar(_ c: UInt16) -> Bool {
            (c >= 0x41 && c <= 0x5A)        // A-Z
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

private func char(_ scalar: UnicodeScalar) -> UInt16 {
    UInt16(scalar.value)
}
