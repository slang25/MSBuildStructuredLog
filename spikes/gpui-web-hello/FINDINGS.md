# gpui in the browser: feasibility spike

Goal: prove Zed's gpui (the exact revision the native viewer uses,
`zed-industries/zed@f66ed399`) renders in a browser, and see how far a small
viewer-like UI gets. Result: **it works**, on stable Rust, with WebGPU, and the
viewer-shaped UI (3 columns, 10,000-row `uniform_list`, selection, keyboard,
monospace block, state-mutating button) behaves normally.

Everything below marked *verified* was observed in Chrome 152 through the
Chrome DevTools MCP on 2026-09-04 (macOS, Apple Silicon, DPR 2). Anything
marked *assumed* was not run.

## Layout

```
spikes/gpui-web-hello/
  Cargo.toml            gpui + gpui_web (git rev f66ed399), no gpui_platform
  rust-toolchain.toml   1.97.1 (copied from the native app) + wasm32 target
  index.html            trunk entry; canvas CSS; data-wasm-opt="0"
  Trunk.toml            serve on 127.0.0.1:8765 with COOP/COEP headers
  src/main.rs           the app (~300 lines)
  FINDINGS.md           this file
  dist/, dist-opt/, target/         build output (gitignored)
```

## Reproduce

Toolchain pieces installed (user level only):

```sh
rustup +1.97.1 target add wasm32-unknown-unknown   # ~10 s
brew install trunk                                  # trunk 0.21.14 (bottled)
# trunk downloads a matching wasm-bindgen CLI (0.2.127, from Cargo.lock) and
# wasm-opt (binaryen version_123) into ~/.cache/trunk on first use.
```

Build and serve (from the repo root):

```sh
cd spikes/gpui-web-hello
trunk build --release            # -> dist/  (cargo + wasm-bindgen)
python3 -m http.server 8765 --bind 127.0.0.1 --directory dist
# or: trunk serve --release      # same thing plus rebuild-on-change, port 8765
open http://127.0.0.1:8765/
```

Timings on an M-series Mac: cold `cargo build --release --target
wasm32-unknown-unknown` of the whole gpui/gpui_web/wgpu/cosmic-text graph was
**54 s wall (260 s CPU)**; a subsequent `trunk build --release` after editing
only `main.rs` is **~40 s** (LTO + codegen-units=1 dominate; drop those for
iteration and it is much faster). `cargo check` is seconds.

## Sizes (release, opt-level="s", lto=true)

| artifact                          | raw      | gzip -9 |
|-----------------------------------|----------|---------|
| `*_bg.wasm` (no wasm-opt)         | 10.27 MB | 3.50 MB |
| `*_bg.wasm` (wasm-opt -Oz)        |  8.05 MB | 3.26 MB |
| wasm-bindgen JS shim              |  142 KB  |  22 KB  |

That includes ~1.6 MB of embedded fonts (gpui_web bundles IBM Plex Sans x4 and
Lilex x4 via `include_bytes!`), the full wgpu WebGPU + WebGL2 backends, naga
(shader translation), cosmic-text and swash. Nothing has been done to trim it;
a `wasm32` build of the real viewer would be larger still. Brotli over the wire
would land around 2.5-3 MB (assumed, not measured).

## What was verified in the browser

- WebGPU is selected (`Browser graphics initialized: requested=WebGpu,
  selected=BrowserWebGpu`), `dual_source_blending=true`. The WebGL2 fallback
  exists in gpui_wgpu but was not exercised.
- Warm reload: canvas inserted **83 ms** after navigation start, first painted
  frame at **~100 ms** (Chrome had the compiled wasm in its code cache; the
  10 MB fetch from localhost took 27 ms). Cold compile of a 10 MB module in a
  fresh profile will be noticeably slower (assumed: hundreds of ms to ~1 s).
- `uniform_list` with 10,000 rows renders only the visible window; appending
  2,000 more rows via the button is instant.
- Wheel scrolling: 120 synthetic wheel events (deltaY=120) over 2.1 s moved the
  list exactly 654 rows (120*120/22), with `requestAnimationFrame` intervals of
  **p50 8.3 ms, p99 9.4 ms, max 9.4 ms** -- i.e. a solid 120 Hz on a ProMotion
  display, on the main thread, single-threaded build.
- Click selects a row, ArrowUp/ArrowDown move the selection (gpui actions +
  key bindings + focus all work), the detail pane re-renders, hover styles
  work, the Lilex monospace `font_family` renders, the uptime counter ticks.
- Console is clean apart from a favicon 404 and a Chrome "form field should
  have an id" issue for gpui_web's hidden IME `<textarea>`.

## Problems hit and how they were solved

1. **hello_web wants nightly.** The upstream example ships
   `rust-toolchain.toml` = nightly + `rust-src`, and `.cargo/config.toml` with
   `-C target-feature=+atomics,+bulk-memory,+mutable-globals`,
   `--shared-memory`, and `[unstable] build-std`. That is only because
   `gpui_web`'s default `multithreaded` feature spawns real web workers
   (`wasm_thread`) and needs a rebuilt std with atomics.
   Solution: do not use `gpui_platform` (its `gpui_web` dependency has default
   features on, and cargo feature unification would drag `multithreaded` back
   in). Depend on `gpui_web` directly with `default-features = false` and
   construct the platform by hand -- it is the same four lines as
   `gpui_platform::single_threaded_web()`. Everything then compiles on stable
   1.97.1 with the prebuilt wasm32 std. `parking_lot`'s `nightly` feature is
   harmless on stable: it only gates `stdarch_wasm_atomic_wait` behind
   `target_feature = "atomics"`.
   Cost: `cx.background_spawn` / `background_executor()` run on the main
   thread. Anything CPU-heavy (parsing a binlog) would block rendering; the
   multithreaded build (nightly + COOP/COEP headers) or a hand-rolled worker
   would be needed for that.

2. **"app was released" right after launch, no canvas.** Fully reproducible
   and explained by the source: `Application::run` keeps the `App` `Rc` alive
   only inside the launch closure, and `WebPlatform::run` returns immediately
   after calling it (the browser owns the run loop), so the `App` was dropped
   the moment `on_finish_launching` returned. gpui documents exactly this case
   and offers `Application::run_embedded`, which returns an
   `ApplicationHandle` (with `update()` and `to_async()`) that owns the app.
   Solution: `let handle = app.run_embedded(...); std::mem::forget(handle);`.
   Note that upstream's own examples still call `.run()`; they may be relying
   on the multithreaded path keeping things alive, or they may have the same
   bug -- not investigated. `ApplicationHandle` is also precisely the hook an
   external host (e.g. a .NET wasm module) would use to re-enter gpui.

3. **Borrow error in `open_window`** (`window.focus(&view.read(cx).focus, cx)`)
   -- ordinary two-phase borrow issue, clone the `FocusHandle` first.

4. **Synthetic pointer events land at half the coordinates.** `gpui_web`
   reads `offsetX/offsetY`; Chrome derives those for *untrusted* events as
   `clientX / devicePixelRatio`. Real mouse input is unaffected -- this only
   matters for automated testing, where you double the client coordinates.

5. **`--headless` Chrome not needed.** Chrome DevTools MCP drove the user's
   regular Chrome, so WebGPU availability in headless mode was not tested.

## gpui APIs that are stubbed on web at this revision

From `crates/gpui_web/src/platform.rs` / `window.rs` (all verified by reading
the code, not by calling them):

| API | Web behaviour | Browser alternative |
|-----|---------------|---------------------|
| `prompt_for_paths`, `prompt_for_new_path` | returns `Err("not supported on the web")` | `<input type=file>` / File System Access API (`showOpenFilePicker`) via `web-sys` + a `FetchHttpClient`-style bridge; or drag-and-drop onto the canvas (gpui_web *does* wire `DragEvent`/`DataTransfer`) |
| `set_menus`, `set_dock_menu` | no-op | render an in-canvas menu bar with gpui elements |
| `read_from_clipboard` (sync) | always `None` | use `read_from_clipboard_async` (implemented with `navigator.clipboard.read()`, secure context + user activation required) |
| `write_to_clipboard` | text only, via `navigator.clipboard.writeText` | fine for text; images/other types not supported |
| `open_window` | one top-level `Normal` window; popup/floating/dialog kinds error; reopening after close errors | keep everything in one window; overlays via `deferred`/anchored elements instead of `AnchoredPopup` windows |
| `quit`, `restart`, `hide*`, `activate` | warn / no-op | n/a |
| `app_path`, `path_for_auxiliary_executable` | `Err` | n/a |
| `read/write/delete_credentials` | `Ok(None)` / `Err` | `localStorage`/IndexedDB via web-sys |
| `reveal_path`, `open_with_system`, `register_url_scheme` | no-op / `Ok(())` | n/a |
| `WebWindow::minimize`, `zoom` | warn | n/a |
| `background_spawn` (single-threaded build) | runs on the main thread | multithreaded feature (nightly) or a dedicated web worker |
| `on_system_wake` | no-op | n/a |

Things that *are* implemented: pointer/wheel/keyboard/IME (hidden textarea
mirror), drag-and-drop, resize observer, DPR changes, cursor styles, light/dark
appearance, `open_url`, `FetchHttpClient` for `cx.http_client()`, `Storage`.

## Can this be built for `wasm32-unknown-emscripten`?

**No, not at this revision** (conclusion from reading the dependency graph;
see the `cargo check` note at the end of this section for what was actually
run):

- `gpui_web` is `#![cfg(target_family = "wasm")]` and unconditionally depends
  on `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `web-sys` (~60
  features), `console_error_panic_hook` and `raw-window-handle`'s web handle.
  The entire DOM/event/canvas surface is `wasm-bindgen` imports. Its output
  must be post-processed by `wasm-bindgen-cli`, which produces the JS glue
  that the `web` bindgen target expects to be the module's sole host.
- `wgpu` 29 only compiles its `webgpu`/`webgl` (web-sys based) backends for
  `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))`. On
  emscripten it switches to a GLES-via-EGL path (`khronos-egl` + `libloading`),
  which needs Emscripten's GL emulation and is a different, less-tested
  backend; `gpui_wgpu` enables the `webgl` feature unconditionally for
  `target_family = "wasm"`.
- `wasm-bindgen` 0.2.127 does contain an
  `#[cfg(target_os = "emscripten")]` marker section (so the CLI can *detect*
  emscripten-produced modules), but nothing in gpui_web/gpui_wgpu is written
  against Emscripten's own JS runtime, and the multithreaded feature's
  `wasm_thread` is `wasm32-unknown-unknown`-only.

Consequence for joining gpui with a .NET wasm module: .NET's browser-wasm
runtime is an Emscripten module with its own JS runtime, memory and
`Module` object, while a gpui build is a wasm-bindgen module with its own
memory. They cannot be linked into one `.wasm`. The realistic shapes are:

1. **Two modules, one page, JS as the seam.** gpui (wasm-bindgen) owns the
   canvas; .NET (`dotnet.js`) runs alongside; they talk through
   `[JSExport]`/`[JSImport]` on the .NET side and `wasm_bindgen` extern
   functions on the Rust side, passing JSON/strings or copying bytes between
   the two linear memories. Latency is a JS call plus a copy; fine for
   tree-node batches, not for per-frame data.
2. **.NET in a Web Worker**, same seam but off the render thread -- attractive
   because the single-threaded gpui build has no background executor.
3. Compiling gpui as a *component* the .NET runtime hosts (or vice versa) is
   not possible with today's toolchains.

**Verified:** `rustup +1.97.1 target add wasm32-unknown-emscripten` followed by
`cargo check --target wasm32-unknown-emscripten` (no emcc needed for a check)
fails inside `gpui_wgpu` before reaching gpui_web at all:

```
error[E0599]: no variant ... `Canvas` found for enum `SurfaceTarget<'window>`
   --> crates/gpui_wgpu/src/wgpu_context.rs:176
   --> crates/gpui_wgpu/src/wgpu_renderer.rs:325
```

`wgpu::SurfaceTarget::Canvas` exists only on the web-sys backend, which wgpu
compiles for `wasm32` *except* emscripten. So the gpui web stack at this
revision does not even type-check for emscripten, independent of the
wasm-bindgen question.

## Verdict

- Rendering, layout, text, lists, input and state updates all work in the
  browser at the exact gpui revision the native viewer uses, on **stable Rust**
  with a single-threaded build. WebGPU is used when available.
- The two real costs are the 8-10 MB wasm (3.3-3.5 MB gzipped) and the loss of
  a real background executor unless you accept nightly + build-std + COOP/COEP.
- A gpui UI and a .NET wasm module can share a page but not a module; the
  integration seam is JS, and `ApplicationHandle` (`run_embedded`) is the
  gpui-side entry point for a host-driven run loop.
