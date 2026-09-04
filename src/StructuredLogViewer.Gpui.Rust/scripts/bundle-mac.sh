#!/usr/bin/env bash
#
# Wraps the built binary in StructuredLogViewer.app.
#
#   scripts/bundle-mac.sh [--debug]
#
# A bare Mach-O binary gets the generic executable icon in the Dock and no
# bundle identity, so this is what it takes to see assets/AppIcon.icns:
# macOS reads the icon from Contents/Resources via CFBundleIconFile. The
# bundle also declares .binlog, which is what makes "Open With" work.
#
# libmslog.dylib is copied in beside the executable, which is the first place
# Engine::locate looks — so the bundle is self-contained apart from code
# signing (unsigned is fine locally; Gatekeeper will want a signature to
# distribute).
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
cargo_flags=(--release)
if [[ "${1:-}" == "--debug" ]]; then
    profile=debug
    cargo_flags=()
fi

cargo build ${cargo_flags[@]+"${cargo_flags[@]}"}

app="target/StructuredLogViewer.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "target/$profile/structured-log-viewer-gpui" "$app/Contents/MacOS/StructuredLogViewer"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

dylib=${MSLOG_DYLIB:-../StructuredLogViewer.NativeBridge/out/libmslog.dylib}
if [[ -f "$dylib" ]]; then
    cp "$dylib" "$app/Contents/MacOS/"
else
    echo "warning: $dylib not found; build it with the bridge's build-dylib.sh" >&2
fi

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>            <string>en</string>
  <key>CFBundleDisplayName</key>                  <string>MSBuild Structured Log Viewer</string>
  <key>CFBundleExecutable</key>                   <string>StructuredLogViewer</string>
  <key>CFBundleIconFile</key>                     <string>AppIcon</string>
  <key>CFBundleIdentifier</key>                   <string>com.microsoft.msbuild.structuredlogviewer.gpui</string>
  <key>CFBundleInfoDictionaryVersion</key>        <string>6.0</string>
  <key>CFBundleName</key>                         <string>Structured Log Viewer</string>
  <key>CFBundlePackageType</key>                  <string>APPL</string>
  <key>CFBundleShortVersionString</key>           <string>0.1</string>
  <key>CFBundleVersion</key>                      <string>1</string>
  <key>LSMinimumSystemVersion</key>               <string>13.0</string>
  <key>NSHighResolutionCapable</key>              <true/>
  <key>NSHumanReadableCopyright</key>             <string>MIT licensed. https://msbuildlog.com</string>
  <key>NSPrincipalClass</key>                     <string>NSApplication</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key>      <string>MSBuild Binary Log</string>
      <key>CFBundleTypeRole</key>      <string>Viewer</string>
      <key>LSHandlerRank</key>         <string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array><string>com.microsoft.msbuild.binlog</string></array>
    </dict>
  </array>
  <key>UTImportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key>  <string>com.microsoft.msbuild.binlog</string>
      <key>UTTypeDescription</key> <string>MSBuild Binary Log</string>
      <key>UTTypeConformsTo</key>  <array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key>
        <array><string>binlog</string></array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
PLIST

# Ad-hoc signature: without one the bundle still runs, but the Dock icon and
# the bundle identity are cached per-signature, so re-bundling without it can
# leave a stale icon behind.
codesign --force --sign - "$app" >/dev/null 2>&1 || true
touch "$app"

echo "built $app"
