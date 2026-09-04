# StructuredLogViewer.WebEngine (browser-wasm engine)

The engine half of the web viewer: the MSBuild Structured Log engine (`StructuredLogger` +
`StructuredLogger.Utils`) and the `StructuredLogViewer.NativeBridge` classes compiled to
browser-wasm (Mono interpreter, .NET 10) and driven from a Web Worker through a single
JSON-in / JSON-out dispatcher. The Rust/gpui UI half talks to the worker with `postMessage`
and parses exactly the JSON the NativeAOT dylib returns (`include/mslog.h`).

```
src/StructuredLogViewer.Web/engine/
  StructuredLogViewer.WebEngine.csproj   Microsoft.NET.Sdk.WebAssembly, net10.0, refs the NativeBridge project
  Engine.cs                              [JSExport] Engine.Call(method, argsJson) dispatcher + host hooks
  Properties/AssemblyInfo.cs             [SupportedOSPlatform("browser")]
  wwwroot/engine-worker.js               the only JavaScript: module Worker that boots dotnet.js and forwards messages
  wwwroot/test.html                      protocol smoke test page (opens sample.binlog and calls every method)
  wwwroot/sample.binlog                  copy of src/StructuredLogViewer.NativeBridge/msbuild.binlog
  build.sh                               pinned-toolchain publish; prints the publish wwwroot path (--serve to serve it)
  serve.py                               static server with application/wasm MIME (used by build.sh --serve)
  global.json / Directory.Build.props / Directory.Build.targets / Directory.Packages.props
                                         isolate this project from the repo-root SDK roll-forward, props and CPM
  .gitignore                             bin/ obj/ .sdk-manifests-pin/
```

One edit outside this directory: `src/StructuredLogViewer.NativeBridge/StructuredLogViewer.NativeBridge.csproj`
gained `<InternalsVisibleTo Include="StructuredLogViewer.WebEngine" />` (the bridge's formatter/search classes
are `internal`).

## Build and serve

```sh
cd src/StructuredLogViewer.Web/engine
./build.sh            # publish (Release); prints the publish wwwroot path
./build.sh --serve    # publish, then serve on http://127.0.0.1:8940/test.html  (PORT=... to change)
```

Publish output (servable as-is by any static server that sends `application/wasm`):

```
src/StructuredLogViewer.Web/engine/bin/Release/net10.0/publish/wwwroot/
  _framework/            dotnet.js, dotnet.runtime.js, dotnet.native.js, dotnet.native.wasm, *.wasm assemblies
  engine-worker.js
  test.html
  sample.binlog
```

To serve without build.sh: `python3 serve.py bin/Release/net10.0/publish/wwwroot 8940`
(or `python3 -m http.server` from that directory; it also sends `application/wasm`).

What `build.sh` pins (copied from `spikes/dotnet-wasm-direct-interop/run.sh`; see its FINDINGS.md):

* `DOTNET_ROOT=/usr/local/share/dotnet` (the `dotnet` on PATH is a dotnetup install with no workloads).
* `global.json` here pins SDK `10.0.203` (`rollForward: latestPatch`, no prerelease) because the repo-root
  `global.json` allows prerelease and would roll to an 11.0 preview that has no `wasm-tools`.
* `DOTNETSDK_WORKLOAD_MANIFEST_ROOTS` points at `.sdk-manifests-pin/`, a copy of the SDK's loose workload
  manifests with `10.0.109`..`10.0.111` deleted so `wasm-tools` resolves to `10.0.108` -> runtime/emscripten
  packs `10.0.8`, which are the newest ones installed (newer manifests are advertised without their packs;
  fixing that properly needs `sudo dotnet workload update`).
* `Directory.Build.props`/`.targets` here are empty so the repo-root ones are not imported for this project;
  `Directory.Packages.props` turns CPM off for this project (it has no PackageReferences). The referenced
  projects under `src/` still find the repo-root props/CPM from their own directories, so they build the
  same way they do for every other head (their output lands in the repo-root `bin/<Project>/Release/net10.0/`).

Toolchain that was used: SDK 10.0.203, `Microsoft.NET.Runtime.WebAssembly.Sdk` 10.0.8,
`Microsoft.NETCore.App.Runtime.Mono.browser-wasm` 10.0.8, emscripten 3.1.56 (the SDK relinks
`dotnet.native.wasm` in Release; no native code of ours is involved). Full publish: ~30 s.

## Protocol

Create the worker with `new Worker('<wwwroot>/engine-worker.js', { type: 'module' })`. The worker imports
`./_framework/dotnet.js` relative to itself, so `engine-worker.js` must be served next to `_framework/`.

Messages from the worker (page `onmessage`):

| message | when |
|---|---|
| `{event:"ready"}` | runtime booted, exports resolved; requests sent earlier are queued and run now |
| `{event:"progress", ratio}` | during `open`, `ratio` 0..1, every 0.5% and at 1.0 |
| `{event:"error", error:{code:"BootFailed", message}}` | the runtime failed to start |
| `{id, ok:true, result}` | reply; `result` is the parsed JSON below |
| `{id, ok:false, error:{code, message}}` | reply; either the engine returned `{"error":{...}}` or the worker threw |

Requests to the worker: `{id, method, args}`. `id` is any caller-chosen value echoed back; `args` is an object
(omitted = `{}`). Calls are synchronous inside the worker and answered in order.

Error codes: `BadNodeId` (unresolvable node id), `NoSession` (no binlog open), `BootFailed`, otherwise the
.NET exception type name (`InvalidOperationException`, `ArgumentException`, `FileNotFoundException`, ...).
`message` is the exception message.

| method | args | result |
|---|---|---|
| `open` | `{url}` or `{file}` (a `File`, structured-cloned) or `{path}` (already in the runtime FS) | BuildInfo: `{rootId, succeeded, errorCount, warningCount, nodeCount, hasSourceArchive, msBuildVersion, filePath, fileSize, durationMs, startTime, endTime}`. Closes any previously open binlog. `filePath` is the path inside the in-memory FS (`/binlogs/<name>`). |
| `close` | `{}` | `{}` |
| `build_info` | `{}` | BuildInfo (as above) |
| `node_get` | `{id}` | NodeDetails: `{node: NodeSummary, parentId, startTime, endTime, fullText, sourceFile, sourceLine}` |
| `node_children` | `{id, offset=0, count=512, sortMode=0}` | ChildrenPage: `{parentId, total, offset, count, sortMode, children: [NodeSummary]}`. `count <= 0` -> 512, capped at 5000. sortMode 0 natural, 1 by name, 2 by duration. Natural order entries carry `childIndex`. |
| `node_ancestors` | `{id}` | Ancestors: `{chain: [NodeSummary]}`, root first, the node itself last, `childIndex` on every non-root entry |
| `node_source` | `{id}` | SourceLocation: `{filePath, line, text}`; `text` absent when the file is not in the archive; error `InvalidOperationException` when the node has no source location |
| `node_preprocess` | `{id}` | `{"text": "<xml>"}` (all imports inlined); error when the node is not a Project / ProjectEvaluation / Import |
| `file_read` | `{path}` | `{"text": "<content>"}` |
| `search` | `{query, maxResults=500}` | SearchResponse: `{query, resultCount, overflow, elapsedMs, roots: [{node?: NodeSummary, text, highlights: [{text, isHighlight, style?}], children?: [...]}]}`; cap 5000 |
| `semantic_file` | `{path, evaluationId?}` | SemanticFile: `{path, evaluationId, contextsTotal, contexts: [{evaluationId, projectFile, label, isProjectFile}], imports: [{line, column, importedPath, available}], skippedImports: [{line, column, fileSpec, reason, condition?, evaluatedCondition?}], targets: [{name, line}]}` |
| `semantic_resolve` | `{evaluationId, kind, name}` (`kind` = `property` / `item` / `target`) | SemanticSymbol: `{kind, name, found, value?, note?, definitions: [Location], executions: [Location], facts: [{label, value}]}`, Location = `{path, line, label, detail, nodeId?, available}` |
| `timeline` | `{}` | Timeline: `{startTime, durationMs, lanes: [{nodeId, maxIndent, blocks: [{id, kind, text, start, end, indent, hasError}]}]}` |

NodeSummary: `{id, kind, title, name?, value?, hasChildren, childCount, isLowRelevance, state, durationMs?,
hasSource, canPreprocess, childIndex?, props?: {string: string}}` (see `Dto.cs` for the per-kind `props`).

Everything is serialized by the bridge's own source-generated `BridgeJsonContext` (camelCase, nulls omitted), so
the shapes are byte-for-byte what the dylib emits: the same `NodeFormatter`, `SearchExecution`,
`SemanticFormatter`, `TimelineFormatter` and `FileSearch` code runs, and `Engine.cs` mirrors the glue in
`Exports.cs` (build info, children paging, ancestors, source, preprocess) line for line.

Extras, same payloads as the corresponding `mslog_*` exports (not required by the UI spec, cheap to expose):
`node_subtree_text {id}` -> `{text}`, `target_parent {id}` -> NodeSummary, `search_properties_and_items
{contextNodeId, query, maxResults}` -> SearchResponse, `files_list {}` -> FileList, `files_search {term,
maxResults}` -> FileSearchResponse, `project_graph {}` -> ProjectGraph, `build_stats {}` -> Stats.

Deviations from the dylib worth knowing:

* The dylib returns errors as a status code plus `{code, message}`; here they are wrapped as
  `{"error": {code, message}}` on the wire and surfaced by the worker as `{ok:false, error}`. Bad handle
  (status 3) has no equivalent: there is one implicit session, so a call before `open` is `NoSession`.
* No cancellation (`opId`) and no threading: the worker runs one call at a time; a long `open` or `search`
  blocks only the worker, never the page.
* `open` accepts `{url}` / `{file}` and stages the bytes into the emscripten MEMFS itself; the C# side only
  ever sees `{path}`.

## Verified in Chrome (2026-09-04, Apple Silicon, Chrome via the DevTools MCP)

`test.html` against the published output, `sample.binlog` (740,843 bytes, 357,856 nodes, 13.2 s build with an
embedded source archive). Every call succeeded. Timings are
page-side round trips (postMessage in, JSON parse out), interpreter only (no AOT):

| step | time |
|---|---|
| worker ready (runtime boot, warm cache) | 95-123 ms |
| `open` sample.binlog by URL (fetch + MEMFS write + read + analyze + search index) | 1,246-1,270 ms |
| `build_info` | 2 ms |
| `node_children` root, 0..50 (40 children) | 44 ms |
| `node_get` root | 2 ms |
| `node_ancestors` project | 2 ms |
| `node_preprocess` project (1.54 M chars) | 127 ms |
| `search "$warning"` (6 results) | 30 ms (engine 20.7 ms) |
| `search "Csc"` (20 results, overflow) | 69 ms |
| `timeline` (3 lanes, 1,110 blocks) | 73 ms |
| `semantic_file` csproj (3 contexts, 2 imports) | 16 ms |
| `file_read` csproj | 0.5 ms |
| `semantic_resolve` property MSBuildProjectName | 6 ms |
| `node_source` project | 1 ms |
| bad node id -> `BadNodeId`; call after `close` -> `NoSession` | ok |
| whole script | 1.72 s |

Size of the published `_framework/`: 9.66 MB uncompressed, 55 files (`dotnet.native.wasm` 1,558,773 bytes;
`System.Private.CoreLib.wasm` 2,069,269; `dotnet.runtime.js` 198 KB; `dotnet.native.js` 217 KB; `dotnet.js`
50 KB; the rest are trimmed assemblies, largest `System.Private.Xml` 1.2 MB, `StructuredLogger` 437 KB,
`System.Data.Common` 475 KB, `System.Linq.Expressions` 428 KB). Compression was turned off in the csproj
(`CompressionEnabled=false`); with a server that gzips/brotlis on the fly expect roughly a third of that on
the wire. Only console error during the run: the `favicon.ico` 404.

Not exercised: `open {file}` (needs a real file picker; the code path is the same as `{url}` after
`arrayBuffer()`), multi-GB binlogs (the runtime is capped at `--max-memory=2147483648` by the SDK link flags,
and MEMFS holds the whole file in memory in addition to the object graph), and the extra methods.

## Problems hit and how they were solved

1. **MD5 is not implemented on browser-wasm.** `node_preprocess` failed with
   `CryptographicException: Cryptography_UnknownHashAlgorithm, MD5`: `PreprocessedFileManager`'s default host
   hooks name the preprocessed temp file by an MD5 of its content and write it under `Path.GetTempPath()`.
   Those hooks are public static delegates meant for hosts to replace, so `Engine.ConfigureHost()` installs
   FNV-1a based ones that write into the MEMFS `/tmp/MSBuildStructuredLog`. No engine code changed.
2. **Trimming vs. StructuredLogger's Reflector.** The wasm publish trims (`PublishTrimmed`), and the engine
   reads private fields of `Microsoft.Build.Framework` event-args types through Linq Expressions; the NativeAOT
   dylib hit the same thing (empty builds). The csproj points `TrimmerRootDescriptor` at the bridge's
   `ILLink.Descriptors.xml` (a `TrimmerRootDescriptor` item only applies to the project being published, so
   the bridge's own item does nothing here). Result: 357,856 nodes read correctly.
3. **JSON.** Reflection-based `JsonSerializer` is disabled under trimming; every payload goes through the
   bridge's source-generated `BridgeJsonContext`. The two engine-only shapes (`{"text"}` and the
   `{"error":{...}}` envelope) have their own tiny `EngineJsonContext` in `Engine.cs`. Args are parsed with
   `JsonDocument` (no reflection).
4. **Exception messages were resource keys** (`Arg_ParamName_Name, id`): the trimmed default sets
   `UseSystemResourceKeys=true`. Set to `false` in the csproj (+0.8 MB in CoreLib) so the UI gets real text.
5. **Fingerprinted file names.** The static-web-assets pipeline renames content and framework files
   (`dotnet.abc123.js`) and expects an import map in the HTML to resolve them; import maps do not apply inside a
   Worker. `StaticWebAssetsFingerprintContent=false`, `WasmFingerprintAssets=false`, `WasmFingerprintDotnetJs=false`
   keep every name stable so `import './_framework/dotnet.js'` works from the worker.
6. **Reaching the emscripten FS from the worker.** `dotnet.create()` returns a `RuntimeAPI` whose `Module` is
   the emscripten module; `dotnet.native.js` does `Module["FS"] = FS`, so `runtime.Module.FS.writeFile(path,
   bytes)` (after `mkdirTree('/binlogs')`) is all that is needed. `getAssemblyExports(getConfig().mainAssemblyName)`
   works without `dotnet.run()`; `Main` never executes.
7. **The bridge's `Exports.cs` did not upset the toolchain.** The `[UnmanagedCallersOnly]` C exports with
   pointer signatures compile, trim and relink fine under the wasm SDK (nothing references them, so ILLink drops
   them; no C thunks are generated). No exclusion was needed. The bridge project's `PublishAot`/`NativeLib`
   properties only matter when *it* is published, so referencing it from a wasm app is harmless.
8. **Blocking waits in the engine.** `Serialization.Read`, `SearchIndex` construction and
   `Build.WaitForBackgroundTasks()` use `Task.Run(...).Wait()`. On single-threaded browser-wasm the runtime
   runs those work items inline, and `PlatformUtilities.HasThreads` already gates the parallel paths, so the load
   and all searches completed without deadlocking; nothing had to change.
9. **Workload manifest pin / SDK band** as described under "Build and serve" (NETSDK1147 otherwise).
10. **Stray `msbuild.binlog` next to the csproj** after a build: the repo-root `Directory.Build.rsp` passes `/bl`
    to every MSBuild invocation. It is covered by the repo-root `*.binlog` ignore. The one binlog that must be
    tracked, `wwwroot/sample.binlog`, is re-included by this directory's `.gitignore` (`!wwwroot/sample.binlog`).
