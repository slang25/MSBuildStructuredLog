import SwiftUI
import ViewerCore

/// The trailing inspector's document well: closable tabs of open source
/// files / preprocessed XML over the editor, plus the evaluation-context
/// picker that scopes the semantic layer.
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
                    contextBar(for: tab)
                    SourceEditorView(tab: tab, sources: sources)
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

    /// A build file means something different in every project that imports
    /// it, so the semantic layer is scoped to one evaluation — and that
    /// choice belongs on screen, not buried in a preference.
    @ViewBuilder
    private func contextBar(for tab: SourceTab) -> some View {
        if !tab.contexts.isEmpty {
            HStack(spacing: 6) {
                Menu {
                    ForEach(tab.contexts) { context in
                        Button {
                            sources.selectContext(tabId: tab.id, evaluationId: context.evaluationId)
                        } label: {
                            if context.evaluationId == tab.evaluationId {
                                Label(context.label, systemImage: "checkmark")
                            } else {
                                Text(context.label)
                            }
                        }
                    }

                    if tab.contextsTotal > tab.contexts.count {
                        Divider()
                        Text("Showing \(tab.contexts.count) of \(tab.contextsTotal)")
                    }

                    if let evaluationId = tab.evaluationId {
                        Divider()
                        Button("Reveal Evaluation in Tree") {
                            sources.revealContext(evaluationId)
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "scope")
                            .font(.system(size: 9))
                        Text(tab.selectedContext?.label ?? "Choose an evaluation")
                            .font(.caption)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                .menuStyle(.borderlessButton)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(contextHelp(for: tab))

                if tab.contextsTotal > 1 {
                    Text("\(tab.contextsTotal)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .help("\(tab.contextsTotal) evaluations included this file")
                        .layoutPriority(1)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(.bar)

            Divider()
        }
    }

    private func contextHelp(for tab: SourceTab) -> String {
        var lines = ["Evaluated as part of this project — $(properties), targets and imports resolve in its context."]
        if let context = tab.selectedContext {
            lines.append(context.label)
            if let projectFile = context.projectFile {
                lines.append(projectFile)
            }
        }
        return lines.joined(separator: "\n")
    }
}
