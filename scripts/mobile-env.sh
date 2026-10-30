#!/bin/sh
# Runs a command with the environment a mobile build needs, e.g. `sh scripts/mobile-env.sh tauri ios dev`.
#
# rquickjs-sys (RustyPipe's signature solver) ships no prebuilt bindings for iOS or Android, so its build script runs
# bindgen, and bindgen needs the platform SDK headers plus a clang-valid target triple. Apple's clang rejects Rust's
# "aarch64-apple-ios-sim"; it wants "arm64-apple-ios-simulator".
set -eu

if command -v xcrun >/dev/null 2>&1; then
  sim_sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2>/dev/null || true)
  dev_sdk=$(xcrun --sdk iphoneos --show-sdk-path 2>/dev/null || true)
  [ -n "$sim_sdk" ] && export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios_sim="--target=arm64-apple-ios-simulator -isysroot $sim_sdk"
  [ -n "$sim_sdk" ] && export BINDGEN_EXTRA_CLANG_ARGS_x86_64_apple_ios="-isysroot $sim_sdk"
  [ -n "$dev_sdk" ] && export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios="-isysroot $dev_sdk"
fi

# Android: bindgen reads the NDK sysroot (NDK_HOME is what `tauri android` also expects).
if [ -n "${NDK_HOME:-}" ]; then
  for host in darwin-x86_64 linux-x86_64; do
    sysroot="$NDK_HOME/toolchains/llvm/prebuilt/$host/sysroot"
    [ -d "$sysroot" ] || continue
    for triple in aarch64_linux_android armv7_linux_androideabi i686_linux_android x86_64_linux_android; do
      export "BINDGEN_EXTRA_CLANG_ARGS_$triple=--sysroot=$sysroot"
    done
  done
fi

# Android: the AGP zip writer packs the current date as an MS-DOS date, which stops at year 2107. Under a Thai locale the
# JVM's calendar is Buddhist (year 2569), so the APK step fails with a bare VerifyException. Pin the JVM to a Gregorian locale.
export JAVA_TOOL_OPTIONS="-Duser.language=en -Duser.country=US ${JAVA_TOOL_OPTIONS:-}"

# Android: the generated project is git-ignored and keeps Tauri's default launcher icon unless the project's is copied in.
sh "$(dirname "$0")/sync-android-icons.sh"

exec "$@"
