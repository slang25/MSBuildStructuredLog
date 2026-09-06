#!/bin/bash
# Publishes libmslog.dylib (NativeAOT) and verifies its export surface
# against include/mslog.h.
#
# Usage: ./build-dylib.sh [output-dir]
#   ARCHS="arm64 x86_64"  build a universal binary via lipo (default: arm64)
#   CONFIGURATION=Debug   override configuration (default: Release)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIGURATION="${CONFIGURATION:-Release}"
ARCHS="${ARCHS:-arm64}"
OUT_DIR="${1:-$SCRIPT_DIR/out}"

slices=()
for arch in $ARCHS; do
    case "$arch" in
        arm64)  rid=osx-arm64 ;;
        x86_64) rid=osx-x64 ;;
        *) echo "Unknown arch: $arch" >&2; exit 1 ;;
    esac

    echo "== dotnet publish ($rid, $CONFIGURATION)"
    dotnet publish -c "$CONFIGURATION" -r "$rid" \
        "$SCRIPT_DIR/StructuredLogViewer.NativeBridge.csproj"

    slices+=("$REPO_ROOT/bin/StructuredLogViewer.NativeBridge/$CONFIGURATION/net10.0/$rid/publish/mslog.dylib")
done

mkdir -p "$OUT_DIR"
DYLIB="$OUT_DIR/libmslog.dylib"

if [ "${#slices[@]}" -gt 1 ]; then
    lipo -create "${slices[@]}" -output "$DYLIB"
else
    cp "${slices[0]}" "$DYLIB"
fi

install_name_tool -id "@rpath/libmslog.dylib" "$DYLIB"

echo "== export surface check"
declared=$(grep -oE '\bmslog_[a-z_]+\(' "$SCRIPT_DIR/include/mslog.h" | sed 's/($//;s/(//' | sort -u)
exported=$(nm -gU "$DYLIB" | awk '{print $3}' | grep '^_mslog_' | sed 's/^_//' | sort -u)

missing=$(comm -23 <(echo "$declared") <(echo "$exported"))
extra=$(comm -13 <(echo "$declared") <(echo "$exported"))

if [ -n "$missing" ]; then
    echo "ERROR: declared in mslog.h but not exported:" >&2
    echo "$missing" >&2
    exit 1
fi

if [ -n "$extra" ]; then
    echo "ERROR: exported but not declared in mslog.h:" >&2
    echo "$extra" >&2
    exit 1
fi

echo "OK: $(echo "$declared" | wc -l | tr -d ' ') exports match mslog.h"
echo "Built: $DYLIB"
