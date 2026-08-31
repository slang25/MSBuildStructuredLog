import BinlogKit
import SwiftUI

/// The full search-syntax reference (SearchSyntax.md embedded in the
/// bridge), rendered as scrollable text in a popover / help window.
struct SearchSyntaxHelpView: View {
    private static let helpText: String = BinlogSession.searchHelp

    var body: some View {
        ScrollView {
            Text(markdown)
                .font(.system(size: 12))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
        }
        .frame(width: 480, height: 520)
    }

    private var markdown: AttributedString {
        (try? AttributedString(
            markdown: Self.helpText,
            options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(Self.helpText)
    }
}
