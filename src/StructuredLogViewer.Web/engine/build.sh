#!/usr/bin/env bash
# Builds and publishes the browser-wasm engine with the toolchain pinned to what is installed on this
# machine (see README.md), then prints the publish wwwroot path.
#
#   ./build.sh            publish (Release)
#   ./build.sh --serve    publish, then serve the wwwroot on http://127.0.0.1:${PORT:-8940}/
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- toolchain pins (copied from spikes/dotnet-wasm-direct-interop/run.sh) -----------------------
# The dotnet on PATH is a dotnetup install with no workloads; this root has SDK 10.0.203 + wasm-tools.
DOTNET_HOME="${DOTNET_HOME:-/usr/local/share/dotnet}"
DOTNET="$DOTNET_HOME/dotnet"
PORT="${PORT:-8940}"

# SDK 10.0.203 resolves the newest loose workload manifest (10.0.111 -> packs 10.0.11) but only packs
# <= 10.0.8 are installed (fixing that needs sudo). Point the SDK at a copy of the manifests with
# everything above 10.0.108 removed so 10.0.108 -> packs 10.0.8 resolves.
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
export DOTNET_CLI_TELEMETRY_OPTOUT=1
export DOTNET_NOLOGO=1

# --- publish --------------------------------------------------------------------------------------
PUB="$HERE/bin/Release/net10.0/publish"
# Publish outputs from different modes (AOT vs interpreter) do not overwrite cleanly; start fresh.
rm -rf "$PUB"
# Mono AOT by default: 5x faster load and 3-5x faster search than the interpreter on the sample
# binlog, for ~3x the download (31 MB vs 9.7 MB uncompressed). AOT=0 ./build.sh for the interpreter.
AOT_FLAG="-p:RunAOTCompilation=true"; [ "${AOT:-1}" = "0" ] && AOT_FLAG=""
( cd "$HERE" && "$DOTNET" publish StructuredLogViewer.WebEngine.csproj -c Release $AOT_FLAG "$@" )
WWW="$PUB/wwwroot"
echo
echo "publish wwwroot: $WWW"
du -sh "$WWW/_framework" | sed 's/^/_framework: /'
ls -la "$WWW/_framework/dotnet.native.wasm" "$WWW/engine-worker.js" "$WWW/test.html" "$WWW/sample.binlog" 2>/dev/null || true

if [ "${1:-}" = "--serve" ]; then
  echo "Serving $WWW on http://127.0.0.1:$PORT/test.html  (Ctrl-C to stop)"
  exec python3 "$HERE/serve.py" "$WWW" "$PORT"
fi
