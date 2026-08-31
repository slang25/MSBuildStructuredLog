import AppKit

/// Hand-rolled MSBuild-XML syntax highlighting producing an
/// NSAttributedString. Single linear scan, no XML parse — robust against
/// malformed/preprocessed content and fast enough for multi-MB files.
public enum XMLHighlighter {
    public struct Palette {
        public let text: NSColor
        public let tag: NSColor
        public let attributeName: NSColor
        public let attributeValue: NSColor
        public let comment: NSColor
        public let entity: NSColor

        public static var standard: Palette {
            Palette(
                text: .textColor,
                tag: .systemBlue,
                attributeName: .systemTeal,
                attributeValue: .systemRed,
                comment: .systemGreen,
                entity: .systemPurple)
        }
    }

    public static let font = NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular)

    public static func highlight(_ text: String, palette: Palette = .standard) -> NSAttributedString {
        let attributed = NSMutableAttributedString(string: text, attributes: [
            .font: font,
            .foregroundColor: palette.text,
        ])

        let scalars = Array(text.utf16)
        var i = 0
        let n = scalars.count

        func setColor(_ color: NSColor, _ range: NSRange) {
            attributed.addAttribute(.foregroundColor, value: color, range: range)
        }

        let lt = UInt16(UnicodeScalar("<").value)
        let gt = UInt16(UnicodeScalar(">").value)
        let quote = UInt16(UnicodeScalar("\"").value)
        let apostrophe = UInt16(UnicodeScalar("'").value)
        let equals = UInt16(UnicodeScalar("=").value)
        let ampersand = UInt16(UnicodeScalar("&").value)
        let semicolon = UInt16(UnicodeScalar(";").value)
        let bang = UInt16(UnicodeScalar("!").value)
        let dash = UInt16(UnicodeScalar("-").value)

        while i < n {
            let c = scalars[i]
            if c == lt {
                // Comment?
                if i + 3 < n, scalars[i + 1] == bang, scalars[i + 2] == dash, scalars[i + 3] == dash {
                    var end = i + 4
                    while end + 2 < n {
                        if scalars[end] == dash && scalars[end + 1] == dash && scalars[end + 2] == gt {
                            end += 3
                            break
                        }
                        end += 1
                    }
                    if end + 2 >= n { end = n }
                    setColor(palette.comment, NSRange(location: i, length: end - i))
                    i = end
                    continue
                }

                // Tag: color '<', '/', name until whitespace; then attributes.
                var end = i + 1
                var inValue = false
                var valueDelimiter: UInt16 = 0
                var sawName = false
                var nameEnd = i + 1
                while end < n {
                    let ch = scalars[end]
                    if inValue {
                        if ch == valueDelimiter {
                            inValue = false
                            setColor(palette.attributeValue, NSRange(location: end, length: 1))
                        } else {
                            setColor(palette.attributeValue, NSRange(location: end, length: 1))
                        }
                        end += 1
                        continue
                    }
                    if ch == quote || ch == apostrophe {
                        inValue = true
                        valueDelimiter = ch
                        setColor(palette.attributeValue, NSRange(location: end, length: 1))
                        end += 1
                        continue
                    }
                    if ch == gt {
                        end += 1
                        break
                    }
                    if !sawName, ch == UInt16(UnicodeScalar(" ").value) || ch == UInt16(UnicodeScalar("\t").value) || ch == UInt16(UnicodeScalar("\n").value) || ch == UInt16(UnicodeScalar("\r").value) {
                        sawName = true
                        nameEnd = end
                    }
                    end += 1
                }

                if !sawName { nameEnd = end }
                setColor(palette.tag, NSRange(location: i, length: nameEnd - i))
                if sawName {
                    colorAttributes(
                        attributed, scalars, from: nameEnd, to: end,
                        palette: palette, equals: equals, quote: quote, apostrophe: apostrophe, gt: gt)
                }
                if end - 1 >= i, end <= n, scalars[end - 1] == gt {
                    setColor(palette.tag, NSRange(location: end - 1, length: 1))
                }
                i = end
                continue
            }

            if c == ampersand {
                var end = i + 1
                while end < n, end - i < 10, scalars[end] != semicolon { end += 1 }
                if end < n, scalars[end] == semicolon {
                    setColor(palette.entity, NSRange(location: i, length: end - i + 1))
                    i = end + 1
                    continue
                }
            }

            i += 1
        }

        return attributed
    }

    private static func colorAttributes(
        _ attributed: NSMutableAttributedString,
        _ scalars: [UInt16],
        from start: Int,
        to end: Int,
        palette: Palette,
        equals: UInt16,
        quote: UInt16,
        apostrophe: UInt16,
        gt: UInt16
    ) {
        // Between tag name and '>': runs before '=' are attribute names;
        // quoted runs were already colored as values in the main loop.
        var i = start
        var runStart = -1
        var inValue = false
        var delimiter: UInt16 = 0
        while i < end {
            let ch = scalars[i]
            if inValue {
                if ch == delimiter { inValue = false }
                i += 1
                continue
            }
            if ch == quote || ch == apostrophe {
                inValue = true
                delimiter = ch
                runStart = -1
                i += 1
                continue
            }
            if ch == equals {
                if runStart >= 0 {
                    attributed.addAttribute(
                        .foregroundColor, value: palette.attributeName,
                        range: NSRange(location: runStart, length: i - runStart))
                    runStart = -1
                }
                i += 1
                continue
            }
            let isSpace = ch == UInt16(UnicodeScalar(" ").value) || ch == UInt16(UnicodeScalar("\t").value)
                || ch == UInt16(UnicodeScalar("\n").value) || ch == UInt16(UnicodeScalar("\r").value)
            if isSpace || ch == gt || ch == UInt16(UnicodeScalar("/").value) {
                runStart = -1
            } else if runStart < 0 {
                runStart = i
            }
            i += 1
        }
    }
}
