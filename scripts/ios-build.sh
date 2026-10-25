#!/bin/sh
# Thin wrapper so `tauri ios build` runs through scripts/mobile-env.sh (bindgen SDK paths, Gregorian JVM locale).
# Extra arguments pass through, e.g. `sh scripts/ios-build.sh --target aarch64-sim --ci`.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
exec sh scripts/mobile-env.sh bun run tauri ios build "$@"
