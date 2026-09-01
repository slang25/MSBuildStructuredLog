#!/bin/bash
# Builds (and optionally signs + notarizes) StructuredLogViewer.app.
#
# Usage: ./scripts/build-app.sh [output-dir]
#
# Env (same contract as build-macos.cake):
#   APPLE_CERT_NAME     Developer ID Application identity; unset = ad-hoc sign
#   APPLE_ID_EMAIL      Apple ID for notarytool (requires APPLE_CERT_NAME)
#   APPLE_ID_PASSWORD   app-specific password for notarytool
#   APPLE_TEAM_ID       team id for notarytool
#   CONFIGURATION       Debug|Release (default Release)
#   SKIP_DOTNET=1       reuse the previously built libmslog.dylib
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CONFIGURATION="${CONFIGURATION:-Release}"
OUT_DIR="${1:-$PROJECT_DIR/build/dist}"
DERIVED="$PROJECT_DIR/build/DerivedData"

cd "$PROJECT_DIR"

echo "== xcodegen"
xcodegen generate

echo "== xcodebuild ($CONFIGURATION)"
xcodebuild \
    -project StructuredLogViewer.xcodeproj \
    -scheme StructuredLogViewer \
    -configuration "$CONFIGURATION" \
    -derivedDataPath "$DERIVED" \
    build

APP="$DERIVED/Build/Products/$CONFIGURATION/StructuredLogViewer.app"
mkdir -p "$OUT_DIR"
rm -rf "$OUT_DIR/StructuredLogViewer.app"
cp -R "$APP" "$OUT_DIR/"
APP="$OUT_DIR/StructuredLogViewer.app"

if [ -n "${APPLE_CERT_NAME:-}" ]; then
    echo "== codesign ($APPLE_CERT_NAME)"
    codesign --force --options runtime --timestamp \
        --sign "$APPLE_CERT_NAME" "$APP/Contents/Frameworks/libmslog.dylib"
    codesign --force --options runtime --timestamp --deep \
        --sign "$APPLE_CERT_NAME" "$APP"
    codesign --verify --strict --verbose=2 "$APP"

    if [ -n "${APPLE_ID_EMAIL:-}" ] && [ -n "${APPLE_ID_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
        echo "== notarize"
        ZIP="$OUT_DIR/StructuredLogViewer.zip"
        ditto -c -k --keepParent "$APP" "$ZIP"
        xcrun notarytool submit "$ZIP" \
            --apple-id "$APPLE_ID_EMAIL" \
            --password "$APPLE_ID_PASSWORD" \
            --team-id "$APPLE_TEAM_ID" \
            --wait
        xcrun stapler staple "$APP"
        ditto -c -k --keepParent "$APP" "$ZIP"
        echo "Notarized: $ZIP"
    fi
fi

echo "Built: $APP"
