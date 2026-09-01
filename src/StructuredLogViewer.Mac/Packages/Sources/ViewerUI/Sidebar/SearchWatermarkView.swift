import SwiftUI
import ViewerCore

/// Default content for the Search Log pane: recent searches, a syntax
/// primer, and clickable example queries. Every link is a `mslog-search:`
/// URL that we intercept and push back into the search box.
struct SearchWatermarkView: View {
    let recents: [String]
    let maxResults: Int
    let onSearch: (String) -> Void
    let onClearRecents: () -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                paragraph(String(format: SearchWatermark.intro, maxResults))

                if !filteredRecents.isEmpty {
                    recentsSection
                }

                paragraph(SearchWatermark.matching)
                paragraph(nodeKindLinks)
                paragraph(SearchWatermark.clauses)
                paragraph(SearchWatermark.modifiers)

                Text("Examples:")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                paragraph(bulletedLinks(SearchWatermark.examples))
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .searchLinkHandler(onSearch)
    }

    /// Recents that aren't already offered as examples or node kinds, the
    /// same filter the WPF/Avalonia watermarks apply.
    private var filteredRecents: [String] {
        recents.filter { !SearchWatermark.examples.contains($0) && !SearchWatermark.nodeKinds.contains($0) }
    }

    private var recentsSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 4) {
                Text("Recent")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Button("clear", action: onClearRecents)
                    .buttonStyle(.link)
                    .font(.caption)
            }
            paragraph(bulletedLinks(filteredRecents))
        }
    }

    private var nodeKindLinks: String {
        SearchWatermark.nodeKinds.map { SearchWatermark.link("\($0) ") }.joined(separator: ", ")
    }

    private func bulletedLinks(_ queries: [String]) -> String {
        queries.map { " • \(SearchWatermark.link($0))" }.joined(separator: "\n")
    }

    private func paragraph(_ markdown: String) -> some View {
        Text(AttributedString.watermarkMarkdown(markdown))
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Default content for the Properties + Items pane.
struct PropertiesItemsWatermarkView: View {
    let onSearch: (String) -> Void

    var body: some View {
        ScrollView {
            Text(AttributedString.watermarkMarkdown(SearchWatermark.propertiesAndItems))
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
        }
        .searchLinkHandler(onSearch)
    }
}

extension AttributedString {
    /// Parses watermark markdown, keeping the authored line breaks.
    static func watermarkMarkdown(_ text: String) -> AttributedString {
        let rendered = SearchWatermark.render(text)
        return (try? AttributedString(
            markdown: rendered,
            options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}

extension View {
    /// Routes `mslog-search:` links in this subtree to `onSearch` instead
    /// of handing them to the system.
    func searchLinkHandler(_ onSearch: @escaping (String) -> Void) -> some View {
        environment(\.openURL, OpenURLAction { url in
            guard let query = SearchLink.query(from: url) else { return .systemAction }
            onSearch(query)
            return .handled
        })
    }
}
