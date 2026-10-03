# Testing and driving the viewer

Two layers, both in-process. Neither needs the OS event queue, screen
coordinates, or the app to be frontmost.

## Headless UI tests (`cargo test`)

`src/ui_tests.rs` builds the real views in a gpui test window (fake
platform, deterministic executor) and drives them with simulated input:

```rust
cx.simulate_keystrokes("cmd-f");
cx.simulate_input("import");
cx.simulate_click(position, Modifiers::default());
let bounds = cx.debug_bounds("pill-50");        // needs .debug_selector on the element
let state = well.read_with(cx, |w, cx| w.describe(cx));
```

Assertions go against `describe()`, the same JSON the automation channel
dumps. Tests take a process-wide lock (`serial()`): two sessions opening
on the bridge from two threads has crashed it. `MSLOG_TEST_BINLOG=/path`
points them at another log to reproduce something seen on a real build. The engine is the real `libmslog.dylib` on the fixture in
`testdata/msbuild.binlog` (a copy of upstream's
`src/StructuredLogger.Tests/msbuild.binlog`, which the repo root's `*.binlog`
rule keeps untracked); without the dylib each test
prints a note and passes vacuously. Text is not shaped in the fake
platform (glyph advance is zero), so tests can assert on behaviour and
element bounds but not on pixel geometry that depends on text width.

## Automation channel (`--automation`)

```
scripts/drive.py /tmp/big.binlog --source /path/Sdk.props --line 50 -- \
    'sleep 4000' 'click line-47' 'keys cmd-f' 'type import' 'keys enter' \
    dump 'keys escape' 'scroll editor -300 0' 'click pill-50' dump \
    'screenshot /tmp/find.png'
```

The app reads one JSON command per line from stdin and answers one JSON
line each (`{"ok":true,"result":…}` or `{"ok":false,"error":…}`).
Commands: `keys`, `type`, `action` (e.g. `source_editor::FindNext`),
`perf` (renders per view since the last `perf`, see below),
`click`/`move`/`scroll` by element id or window x/y (a scroll takes an
optional trackpad phase — `'scroll editor 0 120 started'` — where the
default is a wheel-like `moved`), `bounds`, `probes`,
`dump`, `screenshot`, `sleep`, `quit`. See `src/automation.rs` for the
exact shapes.

Element ids come from `automation::probe(id)` children on interactive
elements: `line-N`, `pill-N`, `source-tab-N`, `context-picker`,
`context-N`, `find-prev`/`find-next`/`find-close`, `editor`, `tree-row-N`,
`file-row-N`, `tab-search`/`tab-properties`/`tab-files`/…,
`toggle-sidebar`/`toggle-inspector` and their
`divider-sidebar`/`divider-inspector`. Bounds are
window-relative, refreshed on a fresh frame before every bounds-based
command, and targets outside the viewport are refused.

`dump` returns the workspace state: phase, keyboard focus (`editor`,
`find-input`, `tree`, `search-input`, `files-filter`, `workspace`, …),
sidebar tab, whether either side panel is showing
(`sidebarVisible`/`inspectorVisible`), the source well
(tabs, evaluation context, inlay and toggled-pill lines, find state,
selection, popovers), the build tree's visible rows, both search panes,
and the Files pane.

Screenshots capture the window's screen region; the window must not be
covered. Give the log a few seconds to load (`sleep`) before the first
command that needs it.

## Lessons from the first exploratory pass

- `click` yields a frame between mouse-down and mouse-up, like real input;
  text elements track a click across that frame and drop it otherwise.
- Probe bounds are recorded even for elements clipped by an ancestor's
  overflow. A click at those bounds lands on whatever is visible there;
  check `dump` afterwards rather than trusting the reply.
- Actions are named `viewer::…` for the workspace, `source_editor::…` for
  the well, `tree_view`/`text_input` for theirs.
- Focus is the thing to watch: a click on a non-focusable element hands
  focus to the nearest `track_focus` ancestor, which used to be the
  workspace root, where no pane's shortcuts apply.

## Render cost (`perf`, `scripts/perf.py`)

gpui rebuilds every view in the window whenever any one of them notifies,
unless the view is embedded *cached*. The workspace embeds the tree, the
timeline, the source well and every sidebar pane with
`workspace::cached`, and the well embeds the inspector the same way, so a
hover in the tree re-renders the tree (and its ancestors: the workspace
root) and nothing else. Two rules keep that true:

- A cached view only re-renders when *it* is notified (or resized, or the
  window refreshes for a theme or focus change). If its `render` reads an
  entity that is not one of its child views, it must `cx.observe` it —
  the tree observes `Favorites` for its stars.
- Nothing should call `window.refresh()` for something one view owns; that
  re-renders every cached view. `cx.notify(view)` (with
  `window.current_view()` captured during paint, as the scrollbars do)
  repaints just the owner.

Every `render` and list processor holds a `perf::scope`, which counts
calls (and rows built) while `--automation` is on. `{"cmd":"perf"}`
returns those counts and resets them; the
`the_build_tree_does_not_re_render_its_neighbours` UI test asserts on them.

`scripts/perf.py <file.binlog> [--source <path> --line N]` runs a release
build through hover, arrow, scroll, search and timeline scenarios and
prints each one's renders per view and the process CPU it took. Use it
before and after a change that touches rendering: a view showing up in a
scenario it has no part in is the bug to look for. Element lookups force a
full-window refresh, so the script resolves targets up front and then
drives by coordinates, one input per frame.
