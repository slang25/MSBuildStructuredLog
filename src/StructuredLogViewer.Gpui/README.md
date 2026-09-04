# StructuredLogViewer.Gpui (spike)

A cross-platform take on the native macOS viewer, built on
[GPUI.NET](https://github.com/akeit0/gpui-dotnet) — C# bindings over Zed's
GPUI renderer. Same three-pane layout as `StructuredLogViewer.Mac` (search
sidebar, build tree, inspector), same engine underneath, but all C#.

```
C# views (GPUI.NET semantic elements, virtual lists, dock)
    ↕ ViewerSession / TreeModel (this project)
    ↕ BridgeSession + NodeFormatter + SearchExecution (…/StructuredLogViewer.NativeBridge, in-process)
    ↕ StructuredLogger / StructuredLogger.Utils (existing engine, unchanged)
```

The NativeAOT bridge is referenced as an ordinary project, so the exact
formatting the Swift viewer decodes from JSON (row titles, per-kind props,
grouped search results with highlight spans) is reused here with no
serialization at all. `InternalsVisibleTo` on the bridge is the only change
outside this folder, plus the `GPUI.NET` entry in `Directory.Packages.props`.

## Run

```sh
dotnet run --project src/StructuredLogViewer.Gpui -- path/to/build.binlog
dotnet run --project src/StructuredLogViewer.Gpui -- --light   # light theme
```

Or launch with no arguments and drop a `.binlog` on the window / press ⌘O
(macOS uses AppleScript's `choose file`; GPUI.NET has no file dialog yet).

Debug aids, mirroring the Mac app's `-reveal`/`-source` launch arguments:

```sh
... build.binlog --search '$warning'   # run a query once the build opens
... build.binlog --reveal 124          # expand to and select a node id
```

No Rust toolchain needed: the `GPUI.NET.Native.*` packages ship prebuilt
hosts for `osx-arm64`, `osx-x64`, `win-x64` and `linux-x64`. Only macOS has
been exercised here.

## What works

- Load with progress, status bar (result, error/warning counts, duration,
  MSBuild version), welcome/failed states.
- Build tree as a native virtual list: expand/collapse splice the list,
  rows are styled per kind (glyph + tint, project
  TFM badge, `→ Targets` link text, `Import … at (line;col)`, NoImport
  reason chip, durations, dimmed low-relevance rows).
- Search Log pane: debounced cancel-previous search with the viewer's query
  syntax, grouped result tree with highlight spans, click → reveal in tree.
- Details pane: kind, state, timing, source location, per-kind props, full
  text.
- Dock: side regions with native splitters (rendered; drag/collapse not
  yet exercised).
- Arrow-key tree navigation is wired but only lightly exercised (see the
  focus note below).

## Findings on GPUI.NET (0.2.0-preview.1, 2026-09)

- **Model fits well.** "Render() writes a flat arena, Rust retains state" is
  a good match for a million-row tree: only visible rows cross the boundary,
  scroll/measure/selection stay native. Expand/collapse via
  `ListController.Splice` keeps off-screen measurements.
- **Typography gaps in the shipped package.** `FontWeight`, `FontFamily`,
  `FontStyle`, `WhiteSpace`, `TextEllipsis` exist at repo HEAD but not in
  the published 0.2.0-preview.1 — so no bold, no monospace durations, and
  single-line rows use `LineClamp(1)`. Next release should close this.
- **No multi-line text editor in the base package.** The Mac viewer's source
  well (NSTextView + highlighter + Cmd-click semantics) has no equivalent;
  the optional "editor" extension needs a custom Rust host, which defeats the
  "no Rust toolchain" appeal. Details pane shows full text instead.
- **No file dialogs, no clipboard API, no system-appearance signal.** All
  three are easy platform shims, but they're not in the box.
- **Keyboard focus is coarse.** Observer key events reach the view that
  binds them only when no native control consumed them; there's no explicit
  focus model for the tree, so arrow keys work "when nothing else has
  focus".
- **Dock chrome is opinionated** (tab strips, zoom buttons) and not
  restylable to a macOS sidebar look; fine for a spike, not the Mac app's
  feel.
- **Flex defaults bite:** fixed-width spacers shrink under long content
  unless `Shrink(0)` is set — every row part except the primary text needs
  it.
- **Rows have to be Buttons.** `OnClick` only exists on Button/Checkbox/
  Radio, and observer bindings (`OnMouseDown` on a Div) are not allowed
  inside virtual list rows. Button ships card chrome from the native side
  (1px border, 6px radius, element fill), so `RowChrome.Flat` strips it
  back to a transparent hit area and the row paints selection/hover
  itself. Nested buttons are untested (propagation unknown), hence the
  separate chevron and body buttons.
- **Not the full power of GPUI.** It is a semantic subset — Div/Text/
  Button/Badge, Scroll/List/Table/Input/Slider/Dock, overlays, and vector
  `Drawing` paths — driven by ~100 fluent style ops. No custom `Element`
  impls, no paint callbacks, no gpui crate access, no gpui-component
  widgets in the default host. Anything beyond that means a custom Rust
  host via the extension mechanism.
- **Building the native host from source** (to pick up HEAD's typography
  ops) needs a newer Rust than is installed here: Zed's `gpui_util` uses
  `slice::as_array`, stabilised after the local toolchain. `rustup update`
  would likely fix it; not done in this spike.
- **Preview-grade API churn** — the package is 3 days behind HEAD and
  already missing members the README documents. Pin versions.
- Hot reload (`dotnet watch`) is advertised for `Render()` edits; not tried
  here.

## Not done

Source viewer, timeline, project graph, properties+items pane, files/find
in files, favorites, context menus, sort children, copy. All are tractable
with the same building blocks except the source viewer.
