# StructuredLogViewer.Gpui.Rust (spike)

The viewer front end written directly on Zed's [`gpui`](https://gpui.rs)
crate, with the .NET engine behind `libmslog.dylib` — the same NativeAOT
bridge the Swift macOS app talks to. Compared with the GPUI.NET spike in
`../StructuredLogViewer.Gpui`, this has the whole framework available:
system font and weights, `uniform_list`, custom `Element`s, native menus,
file dialogs, clipboard, drag-and-drop, window appearance, key contexts.

```
Rust views (gpui: uniform_list tree, custom text input, dock-less split)
    ↕ engine.rs — libloading over include/mslog.h, serde over the same JSON DTOs BinlogKit decodes
    ↕ libmslog.dylib (…/StructuredLogViewer.NativeBridge, NativeAOT)
    ↕ StructuredLogger / StructuredLogger.Utils (unchanged)
```

## Build and run

```sh
# once: the bridge dylib (dotnet publish, NativeAOT)
../StructuredLogViewer.NativeBridge/build-dylib.sh

cargo run -- path/to/build.binlog
cargo run -- path/to/build.binlog --search '$warning' --reveal 124   # debug aids
cargo run -- path/to/build.binlog --source /path/in/binlog.props --line 163
cargo run -- path/to/build.binlog --timeline
cargo run -- path/to/build.binlog --pane files                       # sidebar panes
cargo run -- path/to/build.binlog --reveal 104 --props TargetFramework
cargo run -- path/to/build.binlog --find TargetFramework
cargo test                                                           # lexer / files-tree tests
```

`rust-toolchain.toml` pins the Rust version Zed's gpui needs (1.97.1);
rustup installs it for this directory only. The first build compiles gpui
(≈2 min on an M-series Mac); later builds are seconds.

Two cargo features matter on macOS, both set in `Cargo.toml`:

- `gpui_platform/runtime_shaders` — compiles Metal shaders at launch, so
  the Metal Toolchain component is not needed.
- `gpui_platform/font-kit` — the macOS text backend. Without it every
  layout is right and no glyph is drawn (that cost half an hour).

The dylib is found via `$MSLOG_DYLIB`, next to the executable, or in the
bridge's `out/` folder in the source tree.

## What's here

- `engine.rs` — `Engine` (dylib + fn pointers) and `Session` (one open
  build); blocking calls run on gpui's background executor.
- `tree.rs` / `tree_view.rs` — flattened tree over `uniform_list`; children
  fetched on expand; arrows walk/fold, Enter toggles, Space or double-click
  opens the node's source (or toggles when it has none), ⌘C copies,
  reveal expands ancestors from `mslog_node_ancestors`. Right-click gives the
  WPF viewer's context menu, built per node so an entry only appears when it
  applies: favourite, view source, preprocess, the copies (title, name,
  value, file path, subtree via `mslog_node_subtree_text`, children), the
  search clauses it composes into the Search Log box (`under($42)`,
  `notunder($42)`, `project(Name)`), show in timeline, and re-sorting
  children by name or duration (a fold plus a re-fetch at the bridge's
  `sortMode`).
- `search_view.rs` — the two query panes, Search Log and Properties and
  items: debounced cancel-previous search (`mslog_cancel`), grouped results
  with `StyledText` highlight runs, click → reveal. The empty state is the
  WPF viewer's watermark — the whole query language, every fragment
  clickable, checked against `mslog_search_help` so nothing is advertised
  that the engine will not accept. Properties and items
  goes through `mslog_search_properties_and_items` and carries the
  "Project: …" context bar; the tree tells it which Project or
  ProjectEvaluation the selection sits under (`TreeEvent::ProjectContext`,
  the WPF `UpdateProjectContext` rule) and the query re-runs when that moves.
- `files_view.rs` — the two source-archive panes. **Files** is
  `mslog_files_list` as a folder tree with single-child chains folded the way
  WPF's `CompressTree` does, folders before files, a line count per file and
  a filter box that opens every folder above a match. **Find in Files** is a
  debounced `mslog_files_search`, grouped by file, each hit with its line
  number and highlighted spans. Both open into the document well. Unit-tested.
- `favorites.rs` — the shared `Favorites` entity and its pane. ⌘D on the
  build tree, or the star that appears on the selected row, toggles
  membership; the pane lists them with a remove button and reveals on click,
  and the tab strip shows the count.
- `text_input.rs` — gpui's `input` example as a reusable single-line field
  (IME, selection, clipboard) emitting change/submit events.
- `inspector.rs` — details from `mslog_node_get`, with View Source /
  Preprocess buttons.
- `msbuild.rs` — the Mac viewer's `XMLHighlighter` + `MSBuildTokenizer`
  ported: one linear byte scan for colours, one for navigable tokens
  (`$(prop)`, `@(item)`, target names, Import/Sdk paths), the
  `SemanticIndex` that joins tokens to the build's recorded import edges,
  and the end-of-element annotations for skipped imports. Unit-tested.
- `source_view.rs` — the document well: Details tab + closable source /
  preprocessed tabs (middle-click closes one); a `uniform_list` editor (fixed
  18px rows, horizontal scroll) with `InteractiveText` lines; a viewport-pinned `canvas` overlay
  that paints the line gutter and the skipped-import pills at the trailing
  edge (`'.csproj' == '.vbproj' → false`); the evaluation-context picker;
  drag to select and ⌘C to copy (⌘A takes the file) — the gutter and the
  inlays are painted, never part of the text, so neither can end up on the
  clipboard; hover → quick info popover (property value, facts, definitions,
  executions, imports, or why an import was skipped), plain click pins it,
  ⌘-hover underlines navigable tokens, ⌘-click follows (single destination
  jumps, several offer a chooser, a target definition goes to where it
  ran), and "reveal evaluation in tree".
- `timeline_view.rs` — the Tracing view painted with `canvas`: lanes per
  worker node, flame-chart nesting recomputed over the *visible* blocks so
  the kind filters re-compact, 1/2/5×10ⁿ ruler, labels inside wide blocks,
  scroll to pan, ⌥-scroll to zoom about the pointer, −/Fit/+ buttons,
  hover tooltip, click → reveal in the tree.
- `workspace.rs` — transparent title bar with status pills and a Log /
  Timeline switch (⌘1/⌘2), sidebar | centre | well with draggable
  dividers, load/progress/failed states, ⌘O via the native open panel,
  drop a `.binlog` on the window, and a double-click on the title bar doing
  whatever `AppleActionOnDoubleClick` says (`window.titlebar_double_click`).
  The error and warning pills run `$error` / `$warning` when there is
  anything to see. The sidebar carries the WPF viewer's five
  panes on a tab strip along its bottom edge (⌘F, ⌘⇧P, ⌘⇧E, ⌘⇧F, ⌘⇧D, and
  the View menu): Files and Find in Files appear only when the log embedded
  a source archive, and every pane but Search Log is built on first use.
- `styling.rs` / `icons.rs` — the row vocabulary. Shapes are the WPF
  viewer's (`themes/Generic.xaml`): a small rounded square for most kinds
  (the colour *is* the type), a plus and a minus for AddItem/RemoveItem, the
  NuGet mark for a package, a box-and-wire for a file copy, and a
  folded-corner page for a project, tinted by extension the way Visual Studio
  tints them — dashed when it is an evaluation rather than a build. Squares
  are `div`s; the shapes are `canvas` paths.

  The colours are not WPF's, though. A kind resolves to one `Tone`, and the
  icon's hairline and the wash behind it are both derived from that single
  accent composed over the row background — so there is no second palette to
  keep in sync and nothing is a pastel that only reads on white. The accents
  are the GitHub Primer ramps, which hold contrast at both ends. WPF's own
  gradients were tried first and read as toys; three treatments (a wash under
  a hairline, a flat swatch, a hairline alone) were compared side by side in
  both themes before this one.
- `scrollbar.rs` — overlay scrollbars. gpui scrolls happily from a wheel or
  a trackpad but draws nothing while doing it, which leaves a plain mouse
  with no way to scroll and nobody with a sense of position. This is one
  `canvas` over the content: it reads the geometry off the `ScrollHandle`
  each frame, paints a thumb per overflowing axis, and registers
  *window-level* mouse handlers so a drag survives the pointer leaving the
  thumb. Used by the tree, both search panes, the files panes, the inspector,
  favorites and the editor.
- `theme.rs` — light/dark palettes following the window appearance.

Verified on the checked-in `msbuild.binlog` by window capture: load, tree
expand/collapse, search with highlights, reveal, inspector, the timeline,
opening a csproj and an SDK .props with highlighting, gutter, context bar
(15 evaluations), the three skipped-import inlays, and the hover quick-info
popover for an import. Icons: the chips, the AddItem plus, the NuGet mark, the file-copy wire and
the project pages, in both themes. Also by hand: the editor's scrollbars
(both axes, including dragging the vertical thumb through a 7000-line file),
middle-click closing a tab, the tree's context menu composing `under($259)`
into the Search Log box, dragging a selection across lines with ⌘C landing
exactly the selected span on the clipboard, and the warning pill running
`$warning`. The macOS bundle icon was verified
through `NSWorkspace.icon(forFile:)`. Sidebar: switching panes from the strip, Properties
and items scoped to a revealed project (259 hits for `TargetFramework`) and
its empty state, the Files tree with its filter and click-to-open, Find in
Files with highlighted spans and click-to-line, and favouriting two nodes
with ⌘D. Not exercised by hand: ⌘-click navigation, the destination chooser,
pinned quick info, the context picker, keyboard navigation, the open panel.
Those paths are direct ports and the lexer half is unit-tested.

Capturing the window needs the app frontmost — `screencapture -l` on an
occluded gpui window returns a stale frame, which looks exactly like a hang.

## Icons

`assets/icon.svg` is the master: the same brand motif as the SwiftUI app
(`../StructuredLogViewer.Mac/scripts/generate-appicon.swift`), authored
full-bleed. There is no cross-platform icon *format* — every platform wants
its own container — so the portable part is that one file plus a generator:

```sh
cargo run --example generate-icons     # rewrites everything below
scripts/bundle-mac.sh                  # target/StructuredLogViewer.app
```

| Output | Consumed by |
|---|---|
| `assets/AppIcon.icns` | the macOS bundle, via `CFBundleIconFile` |
| `assets/icon.ico` | a Windows executable resource (`build.rs` + the `winresource` crate) |
| `assets/icons/icon-<N>.png` | Linux `~/.local/share/icons/hicolor/<N>x<N>/apps/` beside a `.desktop` file |
| `web/favicon.png`, `web/apple-touch-icon.png` | the browser head, copied by Trunk |

The generator rasterises with resvg — which gpui already depends on for its
`svg()` element — and writes the `.icns` and `.ico` containers itself, so it
needs no `iconutil`, ImageMagick or `png2ico` and runs on any platform. The
only macOS-specific step is padding: Apple's grid puts the tile on 824 of a
1024pt canvas, and only the `.icns` gets that inset.

A bare Mach-O binary has no icon and no bundle identity, so `cargo run` still
shows the generic executable in the Dock; `scripts/bundle-mac.sh` is what
makes the icon appear, and it also declares `.binlog` so "Open With" lists
the app. Windows and Linux packaging are not wired up — the bridge is a
`.dylib` today — but the containers they need are generated and checked in.

## Not done

Project graph, target/property/NuGet graphs, run/debug a task, hide a node,
find-in-file inside the editor, line wrapping (the Mac editor wraps; this one
scrolls horizontally, which is why the inlays pin to the viewport).
Favorites live for the session only — the WPF viewer persists them. None of
the sidebar result lists take the keyboard yet; the build tree does.
