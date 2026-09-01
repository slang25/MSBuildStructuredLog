import SwiftUI
import ViewerCore

public enum SidebarPane: String, CaseIterable, Identifiable {
    case searchLog
    case propertiesAndItems
    case files
    case findInFiles
    case favorites

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .searchLog: return "Search Log"
        case .propertiesAndItems: return "Properties + Items"
        case .files: return "Files"
        case .findInFiles: return "Find in Files"
        case .favorites: return "Favorites"
        }
    }

    public var symbolName: String {
        switch self {
        case .searchLog: return "magnifyingglass"
        case .propertiesAndItems: return "list.bullet.rectangle"
        case .files: return "doc.text"
        case .findInFiles: return "doc.text.magnifyingglass"
        case .favorites: return "star"
        }
    }
}

/// Search Log pane: query field + syntax help + recents + result tree.
struct SearchLogPane: View {
    @Bindable var controller: SearchController
    let onSelect: (NodeSummary) -> Void
    @State private var showingHelp = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                SearchQueryField(
                    text: $controller.query,
                    prompt: "Search ($error, $task csc, …)",
                    onSubmit: { controller.searchNow() })

                Button {
                    showingHelp.toggle()
                } label: {
                    Image(systemName: "questionmark.circle")
                }
                .buttonStyle(.borderless)
                .help("Search syntax reference")
                .popover(isPresented: $showingHelp, arrowEdge: .bottom) {
                    SearchSyntaxHelpView()
                }
            }
            .padding(6)

            Divider()

            if controller.isSearching {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = controller.errorMessage {
                ContentUnavailableView(error, systemImage: "exclamationmark.triangle")
            } else if let response = controller.response {
                VStack(spacing: 0) {
                    SearchStatusBar(response: response)
                    Divider()
                    ResultTreeView(roots: response.roots, onSelect: onSelect)
                }
            } else {
                SearchWatermarkView(
                    recents: controller.recentSearches,
                    maxResults: controller.maxResults,
                    onSearch: { query in
                        controller.query = query
                        controller.searchNow()
                    },
                    onClearRecents: { controller.clearRecents() })
            }
        }
    }
}

struct SearchStatusBar: View {
    let response: SearchResponse

    var body: some View {
        HStack {
            Text(statusText)
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
    }

    private var statusText: String {
        var text = "\(response.resultCount) result\(response.resultCount == 1 ? "" : "s")"
        if response.overflow {
            text += " (capped)"
        }
        text += String(format: " · %.0f ms", response.elapsedMs)
        return text
    }
}

/// Properties + Items pane: shows the project context bar and a scoped
/// search over properties/items/assignments.
struct PropertiesItemsPane: View {
    @Bindable var session: BuildSession
    @Bindable var controller: SearchController
    let onSelect: (NodeSummary) -> Void

    var body: some View {
        VStack(spacing: 0) {
            if let context = session.propertiesContext {
                HStack(spacing: 4) {
                    let style = NodeStyling.style(for: context)
                    Image(systemName: style.symbolName)
                        .font(.system(size: 10))
                        .foregroundStyle(Color(nsColor: style.color))
                    Text(context.title)
                        .font(.caption)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer()
                    Button {
                        session.propertiesContext = nil
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.borderless)
                    .help("Clear project context")
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(.quaternary.opacity(0.5))

                SearchQueryField(
                    text: $controller.query,
                    prompt: "OutputPath, name=TargetFramework, …",
                    onSubmit: { controller.searchNow() })
                    .padding(6)

                Divider()

                if controller.isSearching {
                    ProgressView()
                        .controlSize(.small)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let error = controller.errorMessage {
                    ContentUnavailableView(error, systemImage: "exclamationmark.triangle")
                } else if let response = controller.response {
                    VStack(spacing: 0) {
                        SearchStatusBar(response: response)
                        Divider()
                        ResultTreeView(roots: response.roots, onSelect: onSelect)
                    }
                } else {
                    PropertiesItemsWatermarkView { query in
                        controller.query = query
                        controller.searchNow()
                    }
                }
            } else {
                ContentUnavailableView(
                    "No Project Selected",
                    systemImage: "shippingbox",
                    description: Text("Select a project or evaluation in the build tree to search its properties and items."))
            }
        }
    }
}

/// Files pane: filterable list of the embedded source archive.
struct FilesPane: View {
    @Bindable var controller: FilesController
    let onOpen: (FileEntry) -> Void

    var body: some View {
        VStack(spacing: 0) {
            SearchQueryField(text: $controller.pathFilter, prompt: "Filter files", onSubmit: {})
                .padding(6)

            Divider()

            if controller.isLoadingList {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if controller.allFiles.isEmpty {
                ContentUnavailableView(
                    "No Embedded Files",
                    systemImage: "doc",
                    description: Text("This binlog has no source archive."))
            } else {
                List(controller.filteredFiles, id: \.path) { file in
                    Button {
                        onOpen(file)
                    } label: {
                        HStack {
                            Image(systemName: "doc.text")
                                .font(.system(size: 10))
                                .foregroundStyle(.secondary)
                            VStack(alignment: .leading, spacing: 0) {
                                Text((file.path as NSString).lastPathComponent)
                                    .font(.caption)
                                Text(file.path)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.head)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
                .listStyle(.sidebar)
            }
        }
        .onAppear { controller.loadListIfNeeded() }
    }
}

/// Find in Files pane: content search over the embedded archive.
struct FindInFilesPane: View {
    @Bindable var controller: FilesController
    let onOpen: (_ path: String, _ line: Int) -> Void

    var body: some View {
        VStack(spacing: 0) {
            SearchQueryField(text: $controller.findTerm, prompt: "Find in files", onSubmit: {})
                .padding(6)

            Divider()

            if controller.isSearching {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let results = controller.findResults {
                if results.files.isEmpty {
                    ContentUnavailableView.search(text: results.query)
                } else {
                    List {
                        ForEach(results.files, id: \.path) { file in
                            Section {
                                ForEach(file.matches, id: \.line) { match in
                                    Button {
                                        onOpen(file.path, match.line)
                                    } label: {
                                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                                            Text("\(match.line)")
                                                .font(.caption2.monospacedDigit())
                                                .foregroundStyle(.secondary)
                                                .frame(minWidth: 30, alignment: .trailing)
                                            matchText(match)
                                                .lineLimit(1)
                                        }
                                    }
                                    .buttonStyle(.plain)
                                }
                            } header: {
                                Text((file.path as NSString).lastPathComponent)
                                    .help(file.path)
                            }
                        }
                    }
                    .listStyle(.sidebar)
                }
            } else {
                ContentUnavailableView(
                    "Find in Files",
                    systemImage: "doc.text.magnifyingglass",
                    description: Text("Searches every file embedded in the binlog."))
            }
        }
    }

    private func matchText(_ match: FileMatch) -> Text {
        let line = match.text
        guard let spans = match.spans, !spans.isEmpty else {
            return Text(line.trimmingCharacters(in: .whitespaces)).font(.caption)
        }

        var result = Text("")
        var cursor = line.startIndex
        for span in spans {
            guard let start = line.index(line.startIndex, offsetBy: span.start, limitedBy: line.endIndex),
                  let end = line.index(start, offsetBy: span.length, limitedBy: line.endIndex),
                  start >= cursor else { continue }
            result = result + Text(line[cursor..<start]).font(.caption)
            result = result + Text(line[start..<end]).font(.caption.bold()).foregroundColor(.accentColor)
            cursor = end
        }
        result = result + Text(line[cursor...]).font(.caption)
        return result
    }
}

/// Favorites pane: session-only pinned nodes.
struct FavoritesPane: View {
    @Bindable var favorites: FavoritesStore
    let onSelect: (NodeSummary) -> Void

    var body: some View {
        if favorites.favorites.isEmpty {
            ContentUnavailableView(
                "No Favorites",
                systemImage: "star",
                description: Text("Right-click a node in the build tree to add it to favorites."))
        } else {
            List(favorites.favorites, id: \.id) { node in
                HStack {
                    let style = NodeStyling.style(for: node)
                    Image(systemName: style.symbolName)
                        .font(.system(size: 10))
                        .foregroundStyle(Color(nsColor: style.color))
                    Text(node.title)
                        .font(.caption)
                        .lineLimit(1)
                    Spacer()
                    Button {
                        favorites.remove(id: node.id)
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.tertiary)
                    }
                    .buttonStyle(.borderless)
                }
                .contentShape(Rectangle())
                .onTapGesture { onSelect(node) }
            }
            .listStyle(.sidebar)
        }
    }
}

/// Shared search field with immediate-binding text.
struct SearchQueryField: View {
    @Binding var text: String
    let prompt: String
    let onSubmit: () -> Void

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            TextField(prompt, text: $text)
                .textFieldStyle(.plain)
                .font(.caption)
                .onSubmit(onSubmit)
            if !text.isEmpty {
                Button {
                    text = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary.opacity(0.5)))
    }
}
