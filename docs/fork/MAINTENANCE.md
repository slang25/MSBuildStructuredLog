# Maintaining this fork

This is a fork of [KirillOsenkov/MSBuildStructuredLog](https://github.com/KirillOsenkov/MSBuildStructuredLog).
Upstream is active and we want its changes. Upstream's maintainer has seen the
viewer work here and does not want to maintain it, so it lives on this side of
the line permanently.

That makes the whole job one thing: **keep the patch against upstream small
enough to hold in your head, and keep everything else in directories upstream
will never touch.**

Right now the fork adds ~24,000 lines and modifies ~240 across 16 upstream
files. Only that second number costs anything to maintain.

## The two rules

**1. Our code lives in our directories.** These are fork-owned; upstream has no
files in them, so nothing we put here can ever conflict:

```
src/StructuredLogViewer.Gpui/          the viewer (Rust, gpui)
src/StructuredLogViewer.NativeBridge/  libmslog.dylib, the NativeAOT bridge
src/StructuredLogViewer.Semantics/     the MSBuild semantic model
src/StructuredLogViewer.Web/           the browser-wasm engine and shell
docs/fork/                             this directory
scripts/
.github/
```

A file of ours inside an upstream project directory is worse than it looks: it
is glob-compiled into upstream's assembly, so it silently changes the public API
of the `MSBuild.StructuredLogger` package, and it sits in a directory upstream
may restructure. `src/StructuredLogger/Semantics/` was exactly this until it
became `src/StructuredLogViewer.Semantics/`.

**2. Every upstream file we modify is listed in
[`upstream-delta.allow`](upstream-delta.allow), with a reason.** Adding a line
there is a deliberate act, not a side effect. Before adding one, check whether
the change can live in a fork-owned directory instead.

`scripts/check-upstream-delta.sh` enforces both, and runs in CI. It also reports
allowlist entries we no longer need — which is how you notice a patch has landed
upstream and the entry can go.

```
./scripts/check-upstream-delta.sh          # against upstream/main or origin/main
./scripts/check-upstream-delta.sh some/ref
```

## Syncing with upstream

**Merge, never rebase.** Rebasing our ~34 commits onto a new upstream tip
re-resolves the same conflicts every single time, because the commits are
rewritten and `rerere` has nothing to match. Merging resolves each conflict
once, and the resolution stays in history.

```
git fetch upstream main
git checkout gpui-viewer
git merge upstream/main
```

`.github/workflows/upstream-sync.yml` does this daily: it fast-forwards `master`
to upstream `main`, then opens a PR from `master` into `gpui-viewer`, saying in
the body whether it merges cleanly and which paths conflict if not. The point is
to hit conflicts one upstream commit at a time while the change is still fresh,
rather than once a year in a heap.

Turn on `rerere` locally so a resolution you work out once is replayed next
time:

```
git config rerere.enabled true
git config rerere.autoupdate true
```

## Remotes and branches

- `gpui-viewer` — the default branch, and the long-lived one. Everything lands
  here.
- `master` — a clean mirror of upstream `main`. No fork commit ever lands on it;
  the sync workflow only fast-forwards it, and fails loudly if it can't. It
  exists so the daily sync PR's diff is upstream's actual commits.
- Upstream is `https://github.com/KirillOsenkov/MSBuildStructuredLog.git`. In
  a local clone it may be called `origin` or `upstream`; the delta script takes
  either, and CI adds it explicitly as `upstream`.

## The endgame: no fork at all

Nothing in the viewer needs a source fork *except* the 240 lines in
[`upstream-delta.allow`](upstream-delta.allow), and every one of those groups
is defensible on its own merits, without mentioning this viewer:

| group | the case for it upstream |
|---|---|
| trivial modernization | `Enum.GetValues<T>()`, `Marshal.SizeOf<Guid>()`, ignore `.DS_Store` |
| `BinlogCache` index collision | a real bug in upstream's MCP server: nodes added after `BuildAnalyzer` ran keep `Index 0` and collide with the `Build` root |
| Native AOT / trimming | upstream already ships `PublishNativeAOT.sh` for the Avalonia viewer; the core library currently has reflection holes that defeat it |
| `NodeId` decoupling | `Resolve` takes the index map instead of the MCP server's `LoadedBinlog` — strictly fewer dependencies |
| import conditions | `NoImport` keeps the `Condition` it was skipped for, separately from the prose `Reason`; additive public API, useful to upstream's own viewer |

If those land, the fork dissolves: the viewer becomes its own repository
consuming `MSBuild.StructuredLogger` from NuGet, and syncing is a version bump.
The AOT group is the one that genuinely cannot be fixed from outside the
package, so it is the one that decides whether this is possible.

Send them as separate small PRs — upstream's maintainer closes big ones and
takes small focused ones. Lead the AOT PR with
`EveryConcreteObjectModelTypeCanBeCreated`: the test is what makes the
hand-written `Serialization.CreateNode` switch maintainable for someone who
didn't write it.

Until then, keep each group as its own commit so it can be `git format-patch`'d
onto a clean branch off upstream `main` without untangling anything.
