#!/bin/sh
# Builds the iOS app together with the Live Activity extension. `tauri ios build` regenerates the Xcode project from
# gen/apple/project.yml, so the extension is patched in first; see scripts/ios-live-activity.py.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
python3 "$root/scripts/ios-live-activity.py"
cd "$root"
exec sh scripts/mobile-env.sh bun run tauri ios build "$@"
