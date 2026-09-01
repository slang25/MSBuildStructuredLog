import SwiftUI
import ViewerCore

/// Xcode-style jump bar over the main tree: chevron breadcrumb of the
/// selected node's ancestor chain; clicking an element reveals it.
struct JumpBarView: View {
    let chain: [NodeSummary]
    let onJump: (NodeSummary) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 2) {
                ForEach(Array(chain.enumerated()), id: \.offset) { index, element in
                    if index > 0 {
                        Image(systemName: "chevron.compact.right")
                            .font(.system(size: 9))
                            .foregroundStyle(.tertiary)
                    }

                    Button {
                        onJump(element)
                    } label: {
                        HStack(spacing: 3) {
                            let style = NodeStyling.style(for: element)
                            Image(systemName: style.symbolName)
                                .font(.system(size: 9))
                                .foregroundStyle(Color(nsColor: style.color))
                            Text(shortTitle(element))
                                .font(.caption)
                                .lineLimit(1)
                        }
                    }
                    .buttonStyle(.plain)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
        }
        .frame(height: 22)
        .background(.bar)
    }

    private func shortTitle(_ element: NodeSummary) -> String {
        let title = element.name ?? element.title
        return title.count > 60 ? String(title.prefix(57)) + "…" : title
    }
}
