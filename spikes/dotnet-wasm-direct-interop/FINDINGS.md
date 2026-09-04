# Spike: Rust and .NET calling each other directly inside ONE WebAssembly module

**Question:** can Rust and .NET interop directly in the browser, without JavaScript in the call path?

**Answer: yes, verified in Chrome.** A Rust `staticlib` (target `wasm32-unknown-emscripten`) is linked by the .NET
wasm SDK into `dotnet.native.wasm`. C# calls Rust through `[DllImport]`; Rust calls C# through
`[UnmanagedCallersOnly(EntryPoint = "...")]` symbols. Both directions are plain wasm `call` instructions inside the
same module, sharing one linear memory and one `malloc`. The only JavaScript is `dotnet.js` booting the runtime.

Everything below marked **verified** was observed in this session; anything marked *assumed* was not run.

---

## Layout

```
spikes/dotnet-wasm-direct-interop/
  run.sh                     build Rust -> strip features section -> dotnet publish -> serve
  serve.py                   static file server (correct .wasm MIME); not part of the interop path
  global.json                pins SDK 10.0.203 (the band that has wasm-tools installed)
  Directory.Build.props      empty: blocks the repo-root Directory.Build.props
  Directory.Build.targets    empty: blocks the repo-root Directory.Build.targets
  Directory.Packages.props   ManagePackageVersionsCentrally=false (repo uses CPM; spike needs no packages)
  spike-rs/                  Rust staticlib crate (Cargo.toml, src/lib.rs)
  SpikeApp/                  .NET browser-wasm app
    SpikeApp.csproj          WasmBuildNative + NativeFileReference + TrimmerRootDescriptor
    Program.cs               Engine (exports called BY Rust), Native (imports OF Rust), Main (drives the test)
    ILLink.Descriptors.xml   roots the Engine exports against trimming
    wwwroot/index.html       page
    wwwroot/main.js          the only JS: boots dotnet.js, mirrors log lines into the page
  FINDINGS.md                this file
```

## Exact reproduction

```sh
cd spikes/dotnet-wasm-direct-interop
./run.sh                 # builds everything and serves http://127.0.0.1:8931/
# or: ./run.sh --no-serve
```

What `run.sh` does, spelled out (all paths are what is on this machine):

```sh
# 0. use the dotnet root that actually has the wasm-tools packs (the `dotnet` on PATH is a dotnetup
#    install under ~/Library/Application Support/dotnetup with NO workloads)
export DOTNET_ROOT=/usr/local/share/dotnet

# 1. pin loose workload manifests to a version whose packs are installed (see Blockers #1)
cp -R /usr/local/share/dotnet/sdk-manifests/10.0.100 .sdk-manifests-pin/10.0.100
cp -R /usr/local/share/dotnet/sdk-manifests/10.0.200 .sdk-manifests-pin/10.0.200
rm -rf .sdk-manifests-pin/10.0.100/microsoft.net.workload.*/{10.0.109,10.0.110,10.0.111}
export DOTNETSDK_WORKLOAD_MANIFEST_IGNORE_DEFAULT_ROOTS=1
export DOTNETSDK_WORKLOAD_MANIFEST_ROOTS=$PWD/.sdk-manifests-pin

# 2. Rust staticlib (no emcc involved; rustc only emits objects into an archive)
rustup +1.93 target add wasm32-unknown-emscripten
(cd spike-rs && cargo +1.93 build --release --target wasm32-unknown-emscripten)

# 3. strip the `target_features` custom section from every Rust object (see Blockers #2)
EM=/usr/local/share/dotnet/packs/Microsoft.NET.Runtime.Emscripten.3.1.56.Sdk.osx-arm64/10.0.8/tools/bin
mkdir -p spike-rs/out/objs && cd spike-rs/out/objs
$EM/llvm-ar x ../../target/wasm32-unknown-emscripten/release/libspike.a
for o in *.o; do $EM/llvm-objcopy --remove-section=target_features "$o"; done
$EM/llvm-ar rcs ../libspike.a *.o
cd ../../..

# 4. publish (relinks dotnet.native.wasm with emcc, trims IL, writes the static site)
(cd SpikeApp && /usr/local/share/dotnet/dotnet publish -c Release)

# 5. serve
python3 serve.py SpikeApp/bin/Release/net10.0/publish/wwwroot 8931
```

Note: `dotnet build` also produces a working `dotnet.native.wasm` (in `bin/Release/net10.0/wwwroot/_framework/`),
but it does not copy `index.html`/`main.js` there; `dotnet publish` composes the complete site. `dotnet run` (the
WasmAppHost) would also work but was not used.

## Versions (verified)

| Component | Version |
|---|---|
| .NET SDK | 10.0.203 (`/usr/local/share/dotnet`), workload `wasm-tools`, manifest pinned to 10.0.108 |
| Microsoft.NET.Runtime.WebAssembly.Sdk | 10.0.8 |
| Microsoft.NETCore.App.Runtime.Mono.browser-wasm | 10.0.8 (runtime reports itself as `.NET 10.0.8`) |
| Emscripten (bundled by the workload) | 3.1.56; its clang/wasm-ld are **LLVM 19.1.0** |
| Rust | rustc 1.93.1, **LLVM 21.1.8**, target `wasm32-unknown-emscripten` (rust-std prebuilt, panic=abort profile) |
| Browser | Chrome via the DevTools MCP |

Mixing LLVM 21 objects into an LLVM 19 link worked: `wasm-ld` accepted the objects with no complaints (the only
incompatibility was the advisory `target_features` section, Blocker #2).

## The MSBuild properties that matter

```xml
<Project Sdk="Microsoft.NET.Sdk.WebAssembly">   <!-- sets RuntimeIdentifier=browser-wasm itself -->
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Exe</OutputType>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <WasmBuildNative>true</WasmBuildNative>          <!-- relink dotnet.native.wasm with emcc (implied by NativeFileReference, made explicit) -->
    <WasmAllowUndefinedSymbols>false</WasmAllowUndefinedSymbols>  <!-- default; fail the link if an engine_* export is missing -->
    <WasmNativeStrip>false</WasmNativeStrip>         <!-- optional: keep names for readable DevTools stacks; costs 8x size -->
    <WasmEmitSymbolMap>true</WasmEmitSymbolMap>      <!-- optional -->
  </PropertyGroup>
  <ItemGroup>
    <NativeFileReference Include="../spike-rs/out/libspike.a" />   <!-- file name minus extension == [DllImport("libspike")] module name -->
    <TrimmerRootDescriptor Include="ILLink.Descriptors.xml" />     <!-- REQUIRED: keep UnmanagedCallersOnly exports through IL trimming -->
  </ItemGroup>
</Project>
```

Nothing else is needed for the exports. Specifically, **no `EmccExportedFunction`, no `EmccExtraLDFlags`, no
`-sEXPORTED_FUNCTIONS`** entries were required; those control exports to *JavaScript*, which is not what we want.

### How the SDK wires it (from `Microsoft.NET.Runtime.WebAssembly.Sdk/10.0.8/Sdk/*.targets`)

* `WasmApp.Common.targets` `_GenerateManagedToNative` runs the `ManagedToNativeGenerator` task over all assemblies,
  with `PInvokeModules` = the `FileName` of every `NativeFileReference` (+ `libSystem.Native` etc). It writes
  `obj/.../wasm/for-publish/pinvoke-table.h`.
* For each `[DllImport("libspike")]` it emits a C prototype and an entry in `libspike_imports[]`, registered in
  `pinvoke_tables[]` under the key `"libspike"`. Mono's `wasm_dl_load` (runtime.c) resolves a DllImport module name by
  `bsearch` on that table, so the DllImport string must equal the `.a` file name without extension (not `"*"`, not
  `"__Internal"`, which the generator ignores).
* For each `[UnmanagedCallersOnly(EntryPoint = "engine_version")]` it emits a **real C function with that name**:

  ```c
  __attribute__((export_name("engine_version")))
  void * engine_version () {
      typedef void (*InterpEntry_T11) (int*, int*);
      void * result;
      if (!(InterpEntry_T11)wasm_native_to_interp_ftndescs [11].func) {
          mono_wasm_marshal_get_managed_wrapper ("SpikeApp", "", "Engine", "Version", 100663315, 0);
      }
      ((InterpEntry_T11)wasm_native_to_interp_ftndescs [11].func) ((int*)&result, (int*)wasm_native_to_interp_ftndescs [11].arg);
      return result;
  }
  ```

  `pinvoke.c` is compiled with `-DGEN_PINVOKE` and includes this header, so `pinvoke.o` *defines* `engine_version`,
  and `wasm-ld` resolves the Rust object's `U engine_version` against it. The thunk resolves the managed method itself
  on first call (`mono_wasm_marshal_get_managed_wrapper`) -- **verified**: the final build has no `&Engine.Version`
  "warm-up" in C# and the first call from Rust succeeded. (Older SDKs needed the managed side to take a function
  pointer first; not needed on 10.0.8.)
* `BrowserWasmApp.targets` `_BrowserWasmWriteRspForLinking` puts every `NativeFileReference` into
  `_WasmNativeFileForLinking`, which becomes an argument in `emcc-link.rsp`; `_BrowserWasmLinkDotNet` runs
  `emcc @emcc-default.rsp -msimd128 @emcc-link.rsp @obj/.../emcc-link.rsp`. Link flags of note: `-sWASM_BIGINT=1`,
  `-fwasm-exceptions`, `-sALLOW_MEMORY_GROWTH=1`, `--export-table --growable-table`, `-O2` link / `-Oz` compile.
* The `export_name` attribute means the `engine_*` functions are *also* exported from the wasm module (visible from
  JS as `Module.wasmExports.engine_version`). That is a side effect, not something the Rust->C# path uses.

## What was verified in the browser (console text, copied from Chrome DevTools)

```
[c#] Main started; .NET 10.0.8 on Browser, ProcessorCount=1
[c#] spike_add(20, 22) = 42
[c#] spike_sum_bytes(pinned managed byte[256] @ 0x805630) = 32640 (expected 32640)
[c#] spike_sum_bytes(NativeMemory.Alloc(16) @ 0x158fd00) = 16 (expected 16)
[c#]   engine_add(40, 2) called from Rust
[c#]   engine_version() called from Rust
[c#]   engine_free(0x7f9e98) called from Rust
[c#]   engine_search(0x740b68, 3) called from Rust; query="Csc"
[c#]   engine_free(0x15c9530) called from Rust
[c#] spike_run returned in 7.13 ms; report (allocated by Rust @ 0x15c9628, freed by Rust):
      [rust] spike_run called with query "Csc" (3 bytes read directly at 0x805c70)
      [rust] engine_add(40, 2) = 42
      [rust] engine_version() = "StructuredLogger stub engine 0.1 on .NET 10.0.8"
      [rust] engine_search("Csc") = {"query":"Csc","count":2,"results":[{"name":"Csc","kind":"Task"},{"name":"CscTask","kind":"Task"}]}
      [rust] round trip complete; this report was built with std::format! and Vec, and is malloc'd by Rust
[c#] Engine callbacks observed from Rust: engine_add x1, engine_version x1, engine_search x1, engine_free x2
[c#] ROUND TRIP OK
[c#] Thread: ManagedThreadId=1, IsThreadPoolThread=False, ProcessorCount=1
[c#] Main finished; page button will run a 3s synchronous Rust spin on the main thread.
[c#] SpinOnMainThread(3s): Rust spun for 3.08s synchronously on ManagedThreadId=1 (x=7fcfdb05ffd56ca7)
[js] button handler: C#/Rust call returned after 3078 ms; rAF ticks during the call: 0 (0 means the page was frozen)
```

(The only other console entry was a 404 for `favicon.ico`; every app asset loaded with 200.) The page showed
the same text. The 7 ms for `spike_run` is dominated by the first-call thunk resolution and the
`Console.WriteLine`s inside the callbacks; it is not a steady-state number.

| Direction | Mechanism | Verified |
|---|---|---|
| .NET -> Rust | `[DllImport("libspike")] static extern int spike_add(int,int)` -> Mono interp P/Invoke -> wasm `call $spike_add` | yes (`spike_add`, `spike_sum_bytes`, `spike_run`, `spike_free`, `spike_spin`) |
| Rust -> .NET | `extern "C" { fn engine_search(*const u8, usize) -> *mut c_char; }` -> generated C thunk -> Mono `interp_entry` | yes (`engine_add`, `engine_version`, `engine_search`, `engine_free`) |
| JS in the call path | none. `main.js` only calls `dotnet.create()` / `dotnet.run()` and, for display, receives log lines via a `[JSImport]` that is not part of the interop | yes (see `wwwroot/main.js`) |

## Strings and memory

* **One linear memory.** Rust received a pointer to a pinned managed `byte[]` (`0x805630`, inside Mono's GC heap)
  and to a `NativeMemory.Alloc` block and read both directly (`spike_sum_bytes` returned the right sums). C# received
  a pointer Rust got from `CString::into_raw` and read it with `Marshal.PtrToStringUTF8`. No copying at the boundary.
* **One allocator.** Rust's default global allocator on `wasm32-unknown-emscripten` is `System` = emscripten's
  `malloc`; .NET's `NativeMemory.Alloc/Free` go through `SystemNative_Malloc` = the same `malloc`. In the test, C#
  allocated the strings and Rust handed them back via `engine_free` (C# -> `NativeMemory.Free`); the reverse for the
  report (`spike_free` -> `CString::from_raw`). Because it is the same heap, either side could free the other's
  allocation directly; the explicit pairing is just hygiene.
* **Convention that worked:** UTF-8 `ptr + len` inwards (no NUL needed, C# uses `Encoding.UTF8.GetString(byte*, int)`),
  malloc'd NUL-terminated UTF-8 outwards, plus a free function per owner. `nuint`/`usize` and `int32` map 1:1;
  `uint64` crosses fine (`-sWASM_BIGINT` is on).
* **Managed pointers must be pinned** for the duration of the call (`fixed`), as on any platform. Mono's wasm GC is
  non-moving today (SGen in wasm does not compact), but rely on `fixed`, not on that.

## Threads / blocking (verified)

* The runtime is **single-threaded**: `ProcessorCount=1`, `ManagedThreadId=1`, `IsThreadPoolThread=False`. Everything
  (dotnet.js, Mono, Rust) runs on the browser main thread when booted from a page script.
* A long synchronous call **freezes the page**: a 3 s Rust spin called from C# from a click handler produced
  `rAF ticks during the call: 0`; the handler returned after 3078 ms.
* *Assumed, not run:* the standard fix is to boot the whole runtime inside a Web Worker (`dotnet.js` supports it;
  the same wasm module then lives in the worker and the page talks to it via `postMessage`). The .NET 10 SDK also
  has `WasmEnableThreads=true` (experimental multithreaded Mono, needs COOP/COEP headers -- `serve.py` already sends
  them); it does not change the fact that native code called synchronously from the UI thread blocks it.

## Size (verified)

| Artifact | Bytes |
|---|---|
| `dotnet.native.wasm`, publish, `WasmNativeStrip=false` (as checked in, names kept) | 12,309,595 (gz 4,503,838; br 3,151,001) |
| `dotnet.native.wasm`, publish, `WasmNativeStrip=true` | **1,535,557** (br 484,400) |
| prebuilt `dotnet.native.wasm` shipped in the runtime pack (what you get without `WasmBuildNative`) | 3,002,101 |
| whole published `wwwroot` (all trimmed assemblies + runtime, unstripped) | ~43 MB on disk, most of it `.gz`/`.br` duplicates |
| `libspike.a` input (Rust std + spike, 238 objects) | 10.5 MB, of which ~9 KB of function bytes survive dead-stripping (by symbol name; estimate) |

The relinked module is *smaller* than the prebuilt one because relinking dead-strips unused Mono icalls
(`WasmLinkIcalls`) and unused runtime pieces; the Rust code and the parts of `std` it pulls in are noise at this scale.
A real gpui-sized Rust body would obviously not be.

## Blockers hit, and how each was resolved

1. **NETSDK1147 / no workloads on the default `dotnet`.** `which dotnet` is a dotnetup install with no workloads; the
   `/usr/local/share/dotnet` install has SDK 10.0.203 and `wasm-tools` packs 10.0.2/3/7/8, but its newest loose
   manifest (10.0.111) demands packs 10.0.11 that are not installed, and installing needs sudo. Fixed without sudo:
   `global.json` pins 10.0.203, `run.sh` sets `DOTNET_ROOT=/usr/local/share/dotnet` and points
   `DOTNETSDK_WORKLOAD_MANIFEST_ROOTS` at a copy of the manifests with 10.0.109-111 deleted (so 10.0.108 -> packs
   10.0.8 resolves). `dotnet workload list` then shows `wasm-tools 10.0.108/10.0.100`.
2. **emcc's post-link `wasm-opt` rejected `--enable-bulk-memory-opt` / `--enable-call-indirect-overlong`.** The link
   itself succeeded; emcc 3.1.56 then reads the merged `target_features` section and forwards every feature name to
   its bundled Binaryen, which predates those two LLVM 21 names. Fixed by `llvm-objcopy --remove-section=target_features`
   on each Rust object and re-archiving (`run.sh` step 3). The instructions the Rust code uses (bulk memory etc.) are
   all already enabled by the SDK's own flags, so nothing is lost. Alternative not tried: an older Rust (1.82-1.84,
   LLVM 19) would avoid it without the strip.
3. **`undefined symbol: engine_*` on `dotnet publish`.** Publish trims IL by default; nothing in managed code
   referenced `Engine.*` (only Rust does), so ILLink removed them, the generator emitted no thunks, and `wasm-ld`
   failed. Fixed with `ILLink.Descriptors.xml` (`<type fullname="Engine" preserve="all" />`) via
   `TrimmerRootDescriptor`. `[DynamicDependency]` or `TrimmerRootAssembly` would also work.
4. **Mono assertion `interp.c:2393` inside `engine_search`.** First version used `JsonSerializer.Serialize` on an
   anonymous type; under trimming the reflection serializer is disabled and it threw. A managed exception escaping
   an `UnmanagedCallersOnly` method is fatal in Mono wasm (assert in `interp_entry`, runtime dead). Fixed by
   hand-building the JSON and wrapping every export body in `try/catch`. **Rule: exports must never throw.**
5. Minor: `dotnet build` does not stage `index.html`/`main.js` into `bin/.../wwwroot` (only `_framework/`), so serve
   the `publish` output; port 8765 was already taken by another spike's server, so this one uses 8931.

Not a blocker, but worth knowing: `WasmNativeStrip=false` makes the wasm 8x larger; it was left on in the checked-in
csproj purely so DevTools stack traces name `engine_search` / `interp_entry` etc.

## Things not done

* The C# side is a stub engine; `src/StructuredLogger/StructuredLogger.csproj` was not referenced. The boundary shape
  (UTF-8 query in, malloc'd JSON out) is the one the real engine would use; wiring it up is a `ProjectReference`
  plus rooting whatever the exports touch. Not attempted, so unverified.
* `WasmEnableThreads` and running in a Worker: described from documentation, not run.
* Mono AOT (`RunAOTCompilation=true`) not tried; it changes nothing about the linking story (AOT'd IL becomes more
  objects in the same link) but would speed up the C# side of the round trip.

---

## What this means for gpui

gpui's web platform is **wasm-bindgen based**: it compiles for `wasm32-unknown-unknown` and its DOM/WebGPU access is
generated JS glue bound at instantiation. Objects for `wasm32-unknown-unknown` cannot be linked into an emscripten
module in any useful way: wasm-bindgen's `__wbindgen_*` imports and its post-processing (`wasm-bindgen` CLI rewriting
the module) are incompatible with emcc's link, and gpui's platform layer expects the wasm-bindgen ABI (`JsValue`
handles, externref tables) rather than emscripten's `EM_JS`/`library.js` conventions. So:

* **The UI would be a separate wasm module** from the one built in this spike. The spike proves the *engine* module
  can be a .NET module with Rust linked in; it does not (and cannot) put gpui in it.

Options for joining the two modules, honestly:

**(a) A generated JS import shim at the boundary.** Instantiate `dotnet.native.wasm` (this spike) and
`gpui_app.wasm` separately; give each a small table of imports that the other module implements. A call from gpui
into the engine is a wasm `call` to an *import*, which lands in a JS function that copies the argument bytes
from gpui's memory into the engine's memory (`new Uint8Array(engineMemory.buffer, ptr, len).set(...)`), calls the
engine's export, and copies the result back. JS is only at the boundary; inside each module everything is direct
wasm->wasm as proven here. The copy is unavoidable because the two modules own different linear memories. Cost: a
few hundred ns per call plus bytes copied; fine for "search / expand node / get row text" granularity, not fine for
per-pixel chatter. This is exactly the shape wasm-bindgen already uses for gpui's own DOM calls, so it is the least
surprising to build and debug. Generating the shim from the C# `[UnmanagedCallersOnly]` list and the Rust `extern`
list is mechanical.

**(b) Shared imported `WebAssembly.Memory`.** Both modules could import the same `Memory` object (emscripten supports
`-sIMPORTED_MEMORY`, and rustc/wasm-bindgen can import memory with `--import-memory`), so pointers would be valid in
both. The problem is the **two allocators**: emscripten's dlmalloc (used by Mono, by `NativeMemory`, and by Rust std
when linked into this module) and gpui's Rust allocator (its own dlmalloc/wee_alloc in the other module) would both
believe they own the heap starting at their `__heap_base`, and both would call `memory.grow`. Making it work means
one side must not have an allocator (e.g. gpui's module allocates *through* an imported `engine_malloc`), which means
patching the global allocator in the gpui build and accepting that Mono's GC heap, the emscripten stack and gpui's
data all live in one 4 GB space with one growth policy. Also, `--import-memory` with wasm-bindgen and emscripten's
`ALLOW_MEMORY_GROWTH` in the same page is under-tested territory. Possible, but fragile, and it does not remove JS
from the boundary anyway (the two modules still cannot `call` each other's functions without an import that JS wires
up at instantiation -- that part is cheap, no copying, the imports are just function references).

**(c) Multi-memory.** The wasm multi-memory proposal (shipping in Chrome 120+, Firefox 125+, Safari not yet) lets one
module address several memories, so a module could read the other's memory directly. But neither rustc/LLVM nor
emscripten's toolchain emits multi-memory code from C/Rust source today (LLVM has no `memory(N)` addressing at the
language level; it exists for hand-written wasm and `wasm-merge`). Binaryen's `wasm-merge` can fuse two modules into
one with two memories, but Mono's runtime and gpui's wasm-bindgen glue both assume "memory 0". Not practical now.

**(d) Move the UI into this module.** Impossible unless gpui had a non-wasm-bindgen web platform (an emscripten
target that draws through `EM_JS`/WebGPU bindings and gets input via emscripten's HTML5 API). Nothing like that exists
in gpui and writing it is a platform port, not a linking exercise.

**Recommendation: (a).** Keep the .NET engine module exactly as this spike builds it (Rust helpers linked in where they
earn their keep, e.g. a Rust search/index that calls `engine_*` directly), keep gpui as its own wasm-bindgen module,
and generate a thin JS shim that wires the two instantiations together with byte-copying imports. Reasons:

1. It is the only option that works with today's toolchains as shipped (no allocator surgery, no unshipped proposals).
2. JS stays at the module boundary only, which is what the question was really about: the hot paths inside each
   module are direct wasm calls, verified here.
3. It composes with running the engine module in a Web Worker later (the shim becomes `postMessage`-shaped for the
   async cases), which the blocking result above says you will want for binlog loading anyway.
4. The API surface between UI and engine is naturally coarse (queries returning JSON / row batches), so the boundary
   copy is not where the time goes.

If, later, the amount of data crossing per frame turns out to matter (e.g. virtualised tree rows for a 100k-node
build), revisit (b) for the *engine's* output buffers only: have the engine write into a `SharedArrayBuffer`-backed
region that the UI module imports as its memory, while the engine module keeps its own memory for everything else.
