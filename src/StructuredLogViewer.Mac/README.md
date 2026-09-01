# StructuredLogViewer.Mac

A fully native macOS (15+) viewer for MSBuild `.binlog` files: SwiftUI +
AppKit frontend over the existing StructuredLogger engine, compiled to a
NativeAOT dylib.

```
Swift app (SwiftUI + NSOutlineView)
    ↕ BinlogKit (Swift async wrapper, Codable DTOs)
    ↕ libmslog.dylib — C ABI (…/StructuredLogViewer.NativeBridge, NativeAOT)
    ↕ StructuredLogger / StructuredLogger.Utils (existing engine, unchanged)
```

## Layout

- `project.yml` — XcodeGen spec for the thin app target (the generated
  `.xcodeproj` is gitignored; run `xcodegen generate`).
- `App/` — `@main`, window scenes, welcome screen, Info.plist document
  types (`.binlog`).
- `Packages/` — SwiftPM package `StructuredLogViewerKit` with ~all the code:
  - `CMSLog` — Clang module over the bridge's `mslog.h`.
  - `ViewerCore` — models, `BinlogEngine` protocol, view models
    (NodeStore paging, search debounce, source tabs). No AppKit; fully
    testable against mock engines.
  - `BinlogKit` — `BinlogSession`: async Swift wrapper over the C ABI.
  - `ViewerUI` — SwiftUI views + the NSOutlineView build tree.
- `scripts/build-app.sh` — xcodegen + xcodebuild + optional
  codesign/notarytool (same env vars as `build-macos.cake`).

## Building

```sh
brew install xcodegen              # once
cd src/StructuredLogViewer.Mac
xcodegen generate
xcodebuild -project StructuredLogViewer.xcodeproj -scheme StructuredLogViewer build
# or: ./scripts/build-app.sh
```

The app build runs `../StructuredLogViewer.NativeBridge/build-dylib.sh`
(`dotnet publish` NativeAOT) as a pre-build phase and embeds
`libmslog.dylib` into `Contents/Frameworks/`. Set `SKIP_DOTNET=1` to
reuse the previous dylib during pure-Swift iteration.

Headless (no Xcode project needed):

```sh
cd src/StructuredLogViewer.Mac/Packages
swift build          # compiles everything
swift test           # ViewerCore unit tests (paging, debounce, reveal)
```

## Design notes / deviations from the original plan

- **AppKit split view.** NavigationSplitView + `.inspector` crash on
  macOS 26 when dividers drag (SwiftUI invalidates its split platform
  host inside AppKit's constraint pass); `TriSplitView` wraps a plain
  NSSplitViewController with `sizingOptions = []` hosting controllers.
- **Windows, not DocumentGroup.** SwiftUI documents read files through
  `FileWrapper`, which risks eagerly loading multi-GB binlogs. The app
  uses `WindowGroup(for: URL.self)` plus `NSDocumentController` for
  Open Recent — file bytes only ever flow through the native engine.
- **Tree**: `NSOutlineView` behind `NSViewRepresentable`. The pull-based
  data source maps directly onto the bridge's paged `children` API; only
  visible rows are ever fetched (pages of 512, dim placeholder rows until
  a page lands). SwiftUI's `List(children:)` cannot do this at
  million-node scale.
- **Source editor**: NSTextView + custom line-number ruler and a
  hand-rolled MSBuild-XML highlighter (background-highlighted above
  200 KB). STTextView can be swapped in later if TextKit 2 features are
  wanted; avoiding the external dependency kept the build hermetic.
- Favorites are session-only (parity with the Avalonia viewer), behind a
  small store so persistence can be added without touching callers.
