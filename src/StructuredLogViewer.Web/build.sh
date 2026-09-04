#!/usr/bin/env bash
# Builds the web viewer: gpui UI (Rust → wasm32 via trunk) plus the .NET
# engine (browser-wasm, published by engine/build.sh), staged into dist/.
#
#   ./build.sh              build everything, then serve http://127.0.0.1:8780/
#   ./build.sh --no-serve   build only
#   ./build.sh --ui-only    skip the .NET publish (reuse the last one)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UI="$HERE/../StructuredLogViewer.Gpui.Rust"
DIST="$HERE/dist"
PORT="${PORT:-8780}"

UI_ONLY=0; SERVE=1
for arg in "$@"; do
  case "$arg" in
    --ui-only) UI_ONLY=1 ;;
    --no-serve) SERVE=0 ;;
  esac
done

# 1. Engine: .NET browser-wasm published to engine/**/publish/wwwroot.
if [ "$UI_ONLY" = 0 ]; then
  ( cd "$HERE/engine" && ./build.sh )
fi
ENGINE_WWWROOT="$(find "$HERE/engine" -type d -path '*publish/wwwroot' | head -1)"
[ -n "$ENGINE_WWWROOT" ] || { echo "engine publish output not found under $HERE/engine" >&2; exit 1; }

# 2. UI: trunk writes dist/ (index.html, the wasm, the wasm-bindgen shim).
( cd "$UI" && trunk build --release )

# 3. Stage the engine next to the UI so one static server serves both.
cp -R "$ENGINE_WWWROOT/_framework" "$DIST/_framework"
cp "$ENGINE_WWWROOT/engine-worker.js" "$DIST/engine-worker.js"
[ -f "$ENGINE_WWWROOT/sample.binlog" ] && cp "$ENGINE_WWWROOT/sample.binlog" "$DIST/sample.binlog"
du -sh "$DIST"/*.wasm "$DIST/_framework" 2>/dev/null || true

if [ "$SERVE" = 1 ]; then
  echo "Serving $DIST on http://127.0.0.1:$PORT/?binlog=sample.binlog  (Ctrl-C to stop)"
  exec python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$DIST"
fi
