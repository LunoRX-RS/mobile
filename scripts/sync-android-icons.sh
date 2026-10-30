#!/bin/sh
# Copies the project's launcher icons into the generated Android project.
#
# `src-tauri/gen/` is git-ignored and `tauri android init` fills it with Tauri's default icon once; `tauri icon` only
# rewrites `src-tauri/icons/`. Without this step an APK keeps the default icon. Safe to run on every build.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/src-tauri/icons/android"
res="$root/src-tauri/gen/android/app/src/main/res"

# Nothing to do before `tauri android init` has produced the project.
[ -d "$res" ] || exit 0
[ -d "$src" ] || { echo "sync-android-icons: $src is missing; run 'bun run tauri icon' first" >&2; exit 1; }

cp -R "$src"/. "$res"/
# The default Tauri vector icon the adaptive icon would otherwise fall back to.
rm -f "$res/drawable/ic_launcher_background.xml" "$res/drawable-v24/ic_launcher_foreground.xml"
