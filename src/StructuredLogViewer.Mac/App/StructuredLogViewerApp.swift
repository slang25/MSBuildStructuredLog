import AppKit
import SwiftUI
import ViewerUI

/// Native macOS viewer for MSBuild binary logs. Windows are keyed by
/// binlog URL; file contents are never read by the UI layer — the URL is
/// handed to the NativeAOT engine (libmslog) which streams and indexes it.
///
/// Deliberate deviation from a DocumentGroup: SwiftUI documents read
/// through FileWrapper, which risks eagerly loading multi-GB binlogs.
/// A URL-keyed WindowGroup plus NSDocumentController for Open Recent
/// gives the same UX guarantees deterministically.
@main
struct StructuredLogViewerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup(for: URL.self) { $url in
            Group {
                if let url {
                    BinlogDocumentView(url: url)
                        .navigationTitle(url.lastPathComponent)
                        .navigationDocument(url)
                        .frame(minWidth: 900, minHeight: 560)
                        .onAppear {
                            NSDocumentController.shared.noteNewRecentDocumentURL(url)
                        }
                } else {
                    WelcomeView()
                        .frame(minWidth: 520, minHeight: 380)
                        .navigationTitle("Welcome")
                }
            }
            .background(RouterBootstrap())
        }
        .defaultSize(width: 1280, height: 800)
        .commands {
            OpenCommands()
            ViewerCommands()
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            WindowRouter.open(url: url)
        }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        // The URL-less window is the welcome screen.
        true
    }
}

/// Opens binlog windows from AppKit contexts (Finder, Open Recent,
/// NSOpenPanel) where SwiftUI's openWindow environment isn't reachable.
/// URLs arriving before any scene registered (cold launch from Finder)
/// are queued and flushed on registration.
@MainActor
enum WindowRouter {
    static func open(url: URL) {
        NSDocumentController.shared.noteNewRecentDocumentURL(url)

        // Route through the scene system so WindowGroup(for: URL.self)
        // creates (or focuses) the window for this URL.
        if let openWindowAction {
            openWindowAction(url)
            closeWelcomeWindows()
        } else {
            pending.append(url)
        }
    }

    /// A document window superseding the welcome screen closes it.
    static func closeWelcomeWindows() {
        DispatchQueue.main.async {
            for window in NSApp.windows where window.title == "Welcome" && window.isVisible {
                window.close()
            }
        }
    }

    static var openWindowAction: ((URL) -> Void)? {
        didSet {
            guard let openWindowAction else { return }
            let queued = pending
            pending = []
            for url in queued {
                openWindowAction(url)
            }
            if !queued.isEmpty {
                closeWelcomeWindows()
            }
        }
    }

    private static var pending: [URL] = []

    static func presentOpenPanel() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.init(filenameExtension: "binlog") ?? .data]
        panel.allowsMultipleSelection = true
        panel.message = "Choose an MSBuild binary log"
        if panel.runModal() == .OK {
            for url in panel.urls {
                open(url: url)
            }
        }
    }
}

struct OpenCommands: Commands {
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Open…") {
                WindowRouter.presentOpenPanel()
            }
            .keyboardShortcut("o")
        }
    }
}

/// Welcome window: open button, recent binlogs, drop target.
struct WelcomeView: View {
    @Environment(\.openWindow) private var openWindow
    @State private var isDropTargeted = false

    private var recents: [URL] {
        NSDocumentController.shared.recentDocumentURLs.filter { $0.pathExtension == "binlog" }
    }

    var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 14) {
                Image(systemName: "hammer.circle.fill")
                    .font(.system(size: 56))
                    .foregroundStyle(.green)
                Text("MSBuild Structured Log Viewer")
                    .font(.title2.weight(.semibold))
                Text("Open a .binlog to explore the build tree,\nsearch the log, and inspect sources.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)

                Button {
                    WindowRouter.presentOpenPanel()
                } label: {
                    Label("Open Binary Log…", systemImage: "folder")
                }
                .keyboardShortcut("o")
                .controlSize(.large)

                Text("or drop a .binlog file here")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(isDropTargeted ? Color.accentColor.opacity(0.08) : Color.clear)

            if !recents.isEmpty {
                Divider()
                List {
                    Section("Recent") {
                        ForEach(recents, id: \.absoluteString) { url in
                            Button {
                                openWindow(value: url)
                            } label: {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(url.lastPathComponent)
                                        .font(.callout)
                                    Text(url.deletingLastPathComponent().path)
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                        .truncationMode(.head)
                                }
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                .listStyle(.sidebar)
                .frame(width: 240)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $isDropTargeted) { providers in
            for provider in providers {
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    if let url, url.pathExtension == "binlog" {
                        Task { @MainActor in
                            openWindow(value: url)
                            NSDocumentController.shared.noteNewRecentDocumentURL(url)
                        }
                    }
                }
            }
            return true
        }
    }
}

/// Registers the scene-side window opener with the router; attached to
/// every window so at least one live scene can service AppKit opens.
struct RouterBootstrap: View {
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Color.clear
            .onAppear {
                WindowRouter.openWindowAction = { url in
                    openWindow(value: url)
                }
            }
    }
}
