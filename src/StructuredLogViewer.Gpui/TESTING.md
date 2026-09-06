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
points them at another log to reproduce something seen on a real build. The engine is the real `libmslog.dylib` on the checked-in
`src/StructuredLogger.Tests/msbuild.binlog`; without the dylib each test
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
`click`/`move`/`scroll` by element id or window x/y (a scroll takes an
optional trackpad phase — `'scroll editor 0 120 started'` — where the
default is a wheel-like `moved`), `bounds`, `probes`,
`dump`, `screenshot`, `sleep`, `quit`. See `src/automation.rs` for the
exact shapes.

Element ids come from `automation::probe(id)` children on interactive
elements: `line-N`, `pill-N`, `source-tab-N`, `context-picker`,
`context-N`, `find-prev`/`find-next`/`find-close`, `editor`, `tree-row-N`,
`file-row-N`, `tab-search`/`tab-properties`/`tab-files`/…. Bounds are
window-relative, refreshed on a fresh frame before every bounds-based
command, and targets outside the viewport are refused.

`dump` returns the workspace state: phase, keyboard focus (`editor`,
`find-input`, `tree`, `search-input`, `files-filter`, `workspace`, …),
sidebar tab, the source well
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
