import SwiftUI
import ViewerCore

/// The trailing inspector's document well: closable tabs of open source
/// files / preprocessed XML over the editor.
struct DocumentWellView: View {
    @Bindable var sources: SourceController

    var body: some View {
        VStack(spacing: 0) {
            if sources.tabs.isEmpty {
                ContentUnavailableView(
                    "No Source Open",
                    systemImage: "doc.text",
                    description: Text("Click an error or a node with source, or open a file from the Files pane."))
            } else {
                tabBar
                Divider()
                if let tab = sources.selectedTab {
                    SourceEditorView(tab: tab)
                        .id(tab.id)
                }
            }
        }
        .alert(
            "Source Unavailable",
            isPresented: Binding(
                get: { sources.errorMessage != nil },
                set: { if !$0 { sources.clearError() } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(sources.errorMessage ?? "")
        }
    }

    private var tabBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 1) {
                ForEach(sources.tabs) { tab in
                    let isSelected = tab.id == sources.selectedTabId
                    HStack(spacing: 4) {
                        Image(systemName: tab.kind == .preprocessed ? "wand.and.stars" : "doc.text")
                            .font(.system(size: 9))
                            .foregroundStyle(.secondary)
                        Text(tab.title)
                            .font(.caption)
                            .lineLimit(1)
                        Button {
                            sources.close(tabId: tab.id)
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 7, weight: .bold))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.borderless)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(isSelected ? AnyShapeStyle(.selection.opacity(0.4)) : AnyShapeStyle(.clear))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .contentShape(Rectangle())
                    .onTapGesture {
                        sources.selectedTabId = tab.id
                    }
                    .help(tab.path)
                }
            }
            .padding(.horizontal, 4)
            .padding(.vertical, 3)
        }
        .background(.bar)
    }
}
