#!/bin/sh
# Thin wrapper so `tauri ios build` runs through scripts/mobile-env.sh (bindgen SDK paths, Gregorian JVM locale).
# Extra arguments pass through, e.g. `sh scripts/ios-build.sh --target aarch64-sim --ci`.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

sh scripts/mobile-env.sh bun run tauri ios build "$@"

# tauri merges bundle.iOS.infoPlist (src-tauri/Info.ios.plist) only into device archives; the simulator .app gets
# none of it, so deep links, background audio and the ATS local-networking exception silently vanish there.
# Re-apply the same keys to the sim product and re-sign ad-hoc.
for arg in "$@"; do
  [ "$arg" = "aarch64-sim" ] || continue
  for dd in "$HOME"/Library/Developer/Xcode/DerivedData/lunorx-*/Build/Products/release-iphonesimulator/LunoRX.app; do
    [ -d "$dd" ] || continue
    plutil -extract CFBundleURLTypes raw "$dd/Info.plist" >/dev/null 2>&1 ||
      plutil -insert CFBundleURLTypes -json '[{"CFBundleTypeRole":"Viewer","CFBundleURLName":"io.lunorx.mobile.share","CFBundleURLSchemes":["lunorx"]}]' "$dd/Info.plist"
    plutil -extract UIBackgroundModes raw "$dd/Info.plist" >/dev/null 2>&1 ||
      plutil -insert UIBackgroundModes -json '["audio"]' "$dd/Info.plist"
    plutil -extract NSAppTransportSecurity raw "$dd/Info.plist" >/dev/null 2>&1 ||
      plutil -insert NSAppTransportSecurity -json '{"NSAllowsLocalNetworking":true}' "$dd/Info.plist"
    codesign --force --sign - "$dd"
    echo "ios-build: merged Info.ios.plist keys into $dd"
  done
done
