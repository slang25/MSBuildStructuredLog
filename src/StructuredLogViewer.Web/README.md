# StructuredLogViewer.Web (spike)

The gpui viewer in the browser, with the .NET engine running in a Web
Worker. Same Rust UI as `../StructuredLogViewer.Gpui.Rust` (it is the same
crate, compiled for `wasm32-unknown-unknown` with `gpui_web`), same engine
as the Swift app and the native gpui app (`StructuredLogger` behind the
bridge's `BridgeSession`/`NodeFormatter`/`SearchExecution`, compiled to
browser-wasm).

```
browser tab
 ├─ main thread: gpui UI (Rust → wasm32, WebGPU canvas)      dist/structured-log-viewer-gpui_bg.wasm
 │     src/engine.rs `web` backend: {id, method, args} ⇄ {id, ok, result}
 └─ Web Worker: dotnet.js + engine (browser-wasm)             dist/_framework/, dist/engine-worker.js
       engine/Engine.cs  [JSExport] Call(method, argsJson) → mslog.h-shaped JSON
```

The only JavaScript is `engine/wwwroot/engine-worker.js` (65 lines): it
boots the runtime, writes the chosen binlog into the in-memory filesystem,
and forwards messages to the one exported C# method. Inside each module the
calls are direct; the seam is the worker's message port, which is also what
keeps a multi-second load or search from freezing the page.

## Build and run

```sh
cd src/StructuredLogViewer.Web
./build.sh                # engine publish (~30 s) + trunk build (~1 min) → dist/, then serves :8780
./build.sh --ui-only      # reuse the last engine publish
# open http://127.0.0.1:8780/?binlog=sample.binlog
```

Prerequisites (all user-level, already on this machine): Rust 1.97.1 with the
`wasm32-unknown-unknown` target, `trunk` (Homebrew), the .NET 10.0.203 SDK
with the wasm-tools workload (engine/build.sh pins the workload manifest so
it resolves without sudo — see engine/README.md).

URL parameters stand in for the command line: `binlog=<url>`, `search=`,
`reveal=<nodeId>`, `source=<path>&line=N`, `timeline`. The "Open…" button
uses a hidden `<input type=file>`; the file is handed to the worker and never
touches the main thread's memory.

## Verified in Chrome (DevTools MCP, 2026-09-04)

Load of the sample
(357,856 nodes) through the worker with progress, the build tree with
reveal, details, search (`$warning`: 6 results in 32 ms round trip), the
source tab with highlighting, gutter, the 15-evaluation context bar,
skipped-import inlays and the hover quick-info popover, and the timeline.
No console errors.

Driven with synthetic events from DevTools: ⌘-click on an import opened `Microsoft.NET.SupportedPlatforms.props`
in a new tab with the evaluation context carried over; a plain click pinned
quick info; clicking `$(MSBuildEnableWorkloadResolver)` resolved it through
the worker (`true`, with the property-tracking note); typing `$task Csc` with
real key events ran the debounced search (1 result, 43 ms); setting the hidden
file input's `files` from a fetched blob loaded `picked-from-disk.binlog`.

A second input, `avalonia-sln.binlog` (a non-incremental build of the
Avalonia solution: 1.2 MB, 826,487 nodes, 11 worker nodes), loaded through
the AOT engine and rendered the multi-node timeline.
Nothing larger was to hand; the memory ceiling is still untested.

Automation gotchas worth knowing: gpui_web reads `offsetX/Y`, which Chrome
derives as `clientX / devicePixelRatio` for untrusted pointer events, so
synthetic coordinates must be multiplied by the DPR; and gpui's
`InteractiveText` arms its mouse-up handler on the frame *after* mouse-down,
so a synthetic click needs a frame between down and up (macOS System Events
clicks don't leave one either, which is why the native click tests earlier
in this spike never fired).

Sizes: UI wasm 11 MB uncompressed (no wasm-opt; ~3.5 MB gzip). Engine
`_framework`: 31 MB with Mono AOT (the default), 9.7 MB interpreter
(`AOT=0 engine/build.sh`).

Engine timings on the sample binlog, measured back to back on the engine's
own `test.html` in the same Chrome session, warm cache (page-side round trip
through the worker):

| call | interpreter | AOT |
| --- | --- | --- |
| worker ready | 161 ms | 298 ms |
| open (357k nodes) | 5,205 ms | 1,079 ms |
| node_children root | 130 ms | 59 ms |
| search `$warning` | 121 ms | 63 ms |
| search `Csc` | 370 ms | 74 ms |
| timeline | 300 ms | 92 ms |
| preprocess (1.5 M chars) | 423 ms | 311 ms |

AOT publishes in about 65 s. The two publish modes do not overwrite each
other cleanly (a mixed `_framework` fails at runtime boot with a Mono
assertion), so `engine/build.sh` now deletes the publish folder first.

## What it took

- `engine.rs` became an async seam with two backends; every view awaits
  `session.x()` instead of spawning a blocking call. On native the backend
  still hops to the background executor; on web it posts to the worker.
- gpui_web with `default-features = false` (single-threaded, stable Rust).
  `Application::run_embedded` + a leaked handle, not `run`.
- The embedded web fonts have no symbol glyphs, so `web/fonts/DejaVuSans.ttf`
  is registered with `text_system().add_fonts` on web only.
- Fonts: `theme::MONO` is Lilex on web, Menlo on macOS.
- No native menus, file dialog or drag-drop on web; the title strip loses
  its traffic-light gap; the welcome copy changes.

## Not done

Large-binlog memory
(2 GB wasm cap plus MEMFS copy of the file); cancellation of a superseded
search inside the worker (it runs one call at a time and the UI discards
stale results); COOP/COEP + multithreaded gpui_web (needs nightly).
