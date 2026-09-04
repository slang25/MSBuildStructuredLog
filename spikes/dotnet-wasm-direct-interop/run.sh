#!/usr/bin/env bash
# Builds the Rust staticlib, links it into dotnet.native.wasm via the .NET wasm SDK, publishes the
# static site and serves it. Everything is pinned to what is installed on this machine (see FINDINGS.md).
#
#   ./run.sh            build + publish + serve on http://127.0.0.1:8931/
#   ./run.sh --no-serve build + publish only
#   PORT=9000 ./run.sh  serve on another port
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- toolchain pins -----------------------------------------------------------------------------
# The dotnet on PATH is a dotnetup install with NO workloads; this root has SDK 10.0.203 + wasm-tools.
DOTNET_HOME=/usr/local/share/dotnet
DOTNET="$DOTNET_HOME/dotnet"
EMSDK_BIN="$DOTNET_HOME/packs/Microsoft.NET.Runtime.Emscripten.3.1.56.Sdk.osx-arm64/10.0.8/tools/bin"
RUST_TOOLCHAIN=1.93          # rustc 1.93.1 / LLVM 21.1.8
PORT="${PORT:-8931}"

# --- 1. pinned loose workload manifests ---------------------------------------------------------
# SDK 10.0.203 resolves the newest loose manifest (10.0.111 -> packs 10.0.11) but only packs <= 10.0.8
# are installed (NETSDK1147 without sudo to fix). Point the SDK at a copy with manifests > 10.0.108 removed.
PIN="$HERE/.sdk-manifests-pin"
if [ ! -d "$PIN/10.0.100" ]; then
  mkdir -p "$PIN"
  cp -R "$DOTNET_HOME/sdk-manifests/10.0.100" "$PIN/10.0.100"
  cp -R "$DOTNET_HOME/sdk-manifests/10.0.200" "$PIN/10.0.200"
  for m in "$PIN"/10.0.100/microsoft.net.workload.*; do
    for v in 10.0.109 10.0.110 10.0.111; do rm -rf "$m/$v"; done
  done
fi
export DOTNET_ROOT="$DOTNET_HOME"
export DOTNETSDK_WORKLOAD_MANIFEST_IGNORE_DEFAULT_ROOTS=1
export DOTNETSDK_WORKLOAD_MANIFEST_ROOTS="$PIN"

# --- 2. Rust staticlib for wasm32-unknown-emscripten (no emcc needed to build a staticlib) --------
rustup "+$RUST_TOOLCHAIN" target add wasm32-unknown-emscripten >/dev/null
( cd "$HERE/spike-rs" && cargo "+$RUST_TOOLCHAIN" build --release --target wasm32-unknown-emscripten )

# --- 3. Strip the wasm `target_features` custom section from the Rust objects --------------------
# rustc's LLVM 21 tags objects with `bulk-memory-opt` / `call-indirect-overlong`; emcc 3.1.56 forwards
# every feature it finds to its (older) wasm-opt, which rejects those two names. The code itself links
# and runs fine with the SDK's flags, so drop the advisory section and re-archive.
OUT="$HERE/spike-rs/out"; rm -rf "$OUT"; mkdir -p "$OUT/objs"
( cd "$OUT/objs" \
  && "$EMSDK_BIN/llvm-ar" x "$HERE/spike-rs/target/wasm32-unknown-emscripten/release/libspike.a" \
  && for o in *.o; do "$EMSDK_BIN/llvm-objcopy" --remove-section=target_features "$o"; done \
  && "$EMSDK_BIN/llvm-ar" rcs "$OUT/libspike.a" *.o )
"$EMSDK_BIN/llvm-nm" "$OUT/libspike.a" 2>/dev/null | grep -E ' [TU] (spike_|engine_)' | sort -u

# --- 4. .NET publish: relinks dotnet.native.wasm with libspike.a, trims, writes the static site ----
PUB="$HERE/SpikeApp/bin/Release/net10.0/publish"
rm -rf "$PUB"
( cd "$HERE/SpikeApp" && "$DOTNET" publish -c Release )
ls -la "$PUB"/wwwroot/_framework/dotnet.native.*.wasm

# --- 5. serve (plain static files; the only JS is wwwroot/main.js which boots dotnet.js) -----------
if [ "${1:-}" != "--no-serve" ]; then
  echo "Serving $PUB/wwwroot on http://127.0.0.1:$PORT/  (Ctrl-C to stop)"
  exec python3 "$HERE/serve.py" "$PUB/wwwroot" "$PORT"
fi
