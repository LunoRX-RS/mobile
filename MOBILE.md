# LUNOR-X mobile (iPhone, iPad, Android)

This folder is the desktop app reshaped for phones and tablets. It shares the Rust core (`src-tauri/src/core`, `commands`, `sync`) and the React UI (`src/`) with the desktop app. What differs is described here.

## What differs from desktop

| Area | Desktop | Mobile |
|---|---|---|
| Audio output | libmpv on its own thread (`core/playback/native.rs`) | AVPlayer (iOS) or Media3/ExoPlayer (Android) behind the `lunor-native` plugin (`core/playback/mobile.rs`) |
| Lock screen / notification | macOS Now Playing, media keys (`platform/macos_media.rs`) | `MPNowPlayingInfoCenter` + remote commands (iOS), Media3 `MediaSession` + foreground service (Android) |
| Secrets | `keyring` (Keychain, Credential Manager, Secret Service) | iOS: Keychain, class `AfterFirstUnlockThisDeviceOnly` so items stay off backups and readable while locked (`platform/ios_keyring.rs`). Android: Keystore AES-GCM through the plugin (`platform/android_keyring.rs`), app backup off. No plaintext fallback |
| Google sign-in | Second Tauri window | Tauri cannot open a second window on mobile, so the plugin shows a full-screen native web view (`signIn`). Core validates and stores the cookies exactly as on desktop (`youtube.rs::finalize`) |
| Navigation | Sidebar + top bar | `< 700 px`: bottom tab bar + mini player. `700-1099 px` (iPad portrait): icon rail. `>= 1100 px` (iPad landscape): full sidebar. See `src/lib/layout.ts` |
| Not present | | Discord presence, system/imported fonts, app zoom, keyboard shortcuts, single-instance, `Make lyrics file` on phones |

The core still owns playback state. A track is `PLAYING` only after the platform player confirms it. Lock-screen and headphone presses are queued by the plugin as `remote` events and sent back through the same core path as the in-app buttons (`platform/control.rs`).

## The `lunor-native` plugin

`src-tauri/plugins/lunor-native/` is a Tauri plugin with no webview commands: only Rust can call it, so remote content cannot reach native audio or secrets. Swift (`ios/Sources/LunorNativePlugin.swift`) and Kotlin (`android/.../LunorNativePlugin.kt`) implement one JSON contract:

`load {url}` · `play` · `pause` · `stop` · `seek {positionMs}` · `setVolume {volume}` · `poll` → `{status, positionMs, durationMs, remote[]}` · `nowPlaying {...}` · `signIn` → `{cookies[]}` · Android only: `secretGet/Set/Delete`.

`poll` is read by `core/playback/mobile.rs::watch` every 250 ms while a track is loaded (1 s when idle). The platform latches `ended`/`failed`, so each transition is seen once.

Only `file://` URLs (the disk cache) and `http://127.0.0.1` (loopback proxy) are accepted as sources.

## iOS audio format

AVPlayer cannot play Opus in WebM. On iOS the stream selection (`extract.rs::platform_playable`, `visionos.rs::select`) keeps AAC in MP4 only. The cache names files `*.audio`, which AVPlayer cannot sniff, so the plugin passes `AVURLAssetOutOfBandMIMETypeKey: audio/mp4`. That key is not in Apple's public documentation; if a future iOS rejects it, rename the cache file to `.m4a` instead.

## Build and run

Tools: Xcode (with an iOS simulator), `rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android`, `rustup component add llvm-tools`, CocoaPods, Android SDK + NDK 27, JDK 17+ (Android Studio's JBR works), bun.

Run every mobile command through `scripts/mobile-env.sh`. It sets what the build scripts need:

- **bindgen paths for `rquickjs-sys`.** RustyPipe's signature solver ships no prebuilt bindings for iOS or Android. `src-tauri/.cargo/config.toml` holds the iOS SDK paths (Xcode strips shell variables from build phases); the script adds the Android NDK sysroot from `NDK_HOME`.
- **A Gregorian JVM locale.** Under a Thai locale the JVM calendar is Buddhist (year 2569) and the Android Gradle zip writer fails with a bare `VerifyException` (MS-DOS dates stop at 2107).

```sh
# iOS simulator, static frontend (see "Dev server" below)
sh scripts/mobile-env.sh bun run tauri ios dev "iPhone 17" --no-dev-server-wait \
  -c '{"build":{"devUrl":null,"beforeDevCommand":"true"}}'

# Android debug APK
export ANDROID_HOME=~/Library/Android/sdk NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
sh scripts/mobile-env.sh bun run tauri android build --debug --apk --target aarch64
```

After `tauri ios init`, create the symlink the Xcode script phase needs (the CLI writes `node tauri ...` and runs it from `gen/apple`):

```sh
ln -sf ../../../node_modules/@tauri-apps/cli/tauri.js src-tauri/gen/apple/tauri
```

### Live Activity (Dynamic Island)

The lock-screen card and Dynamic Island are a WidgetKit app extension (`LunoRXLiveActivity`). Tauri regenerates the Xcode project from `gen/apple/project.yml` on every build, so the extension is patched in first:

```sh
sh scripts/ios-build.sh --target aarch64-sim --ci   # patch, then tauri ios build (arguments pass through)
python3 scripts/ios-live-activity.py --check        # verify the patch, change nothing
```

Sources live outside `gen/`: `src-tauri/ios/LiveActivity/Widget` (SwiftUI), `src-tauri/ios/LiveActivity/Intents` (transport buttons), and the shared `LunorActivityAttributes` / `LunorRemoteQueue` / `LunorLiveActivityManager` in the plugin. Re-run the patch after any `tauri ios init`. A device build needs an `io.lunorx.mobile.LiveActivity` App ID and the same team as the app; with `APPLE_DEVELOPMENT_TEAM` set the script moves both targets to automatic signing. iOS 17 is the minimum (`LiveActivityIntent`). The activity is updated by `nowPlaying` and ended by `stop`; buttons only queue `previous`/`toggle`/`next`, which `poll` hands to Rust like a lock-screen press.

Two cards on the lock screen, by design: the system Now Playing card keeps transport, AirPods/Control Center buttons and its accurate clocks, while the LunoRX card is the lyrics card — the line core shows right now, plus art, title and a slim progress bar. `nowPlaying` always fills `MPNowPlayingInfoCenter` and also feeds the manager; the lyric line arrives as an optional `lyric` field on the same payload (Rust picks it from the LRCLIB sidecar in `platform/mobile_media.rs`, keyed by track id, and `prefetch_lyrics` starts the lookup when no cache entry exists yet). Without synced lyrics the LunoRX card falls back to title-as-hero. Android ignores the extra field (Jackson has unknown-properties disabled).

Device builds — one-time extension signing. The main app reuses an already-installed development profile, but the new `io.lunorx.mobile.LiveActivity` extension has none, and command-line `xcodebuild` cannot create one without an Apple ID in Xcode's Accounts (it fails with `No Accounts` / `No profiles for 'io.lunorx.mobile.LiveActivity'`). Once, open `src-tauri/gen/apple/lunorx.xcodeproj` in Xcode, add your Apple ID under Settings → Accounts, then on the `LunoRXLiveActivity` target enable "Automatically manage signing" and pick the team; Xcode registers the App ID and creates the profile. After that, `APPLE_DEVELOPMENT_TEAM=<team> sh scripts/ios-build.sh` builds for device from the command line. (Alternative: register the App ID and an iOS App Development profile on developer.apple.com and install it.) Simulator builds need none of this.

Simulator builds must go through a real codesign pass. Without `APPLE_DEVELOPMENT_TEAM` the script sets an ad-hoc "Sign to Run Locally" identity (`CODE_SIGN_IDENTITY: "-"`); never set `CODE_SIGNING_ALLOWED: NO`. A binary left to the linker's automatic signature (`codesign -dvv` shows `flags=…linker-signed`) is refused by securityd for Keychain access, so `store.get()` errors and Connections shows "The OS key store is unavailable or locked" instead of "Not connected" — Google sign-in can never complete on such a build. A correctly signed simulator build shows `flags=0x2(adhoc)`.

### Dev server

In the iOS simulator the Next dev server's page loads from `tauri://localhost` and React never hydrates (no error is raised). The static export (`bun run build`, what ships) works. Iterate on layout in a desktop browser at 393 px width against `bun run dev` (port 1421, so it does not clash with the desktop app on 1420), and confirm in the simulator with the static export.

## Verified, and what is not

Verified on 2026-10-23:

- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` (host), `cargo clippy --locked --lib --target aarch64-apple-ios-sim` and `--target aarch64-linux-android` with `-D warnings`: clean. `cargo test --locked`: 572 unit tests plus the integration tests pass. `bun run lint` and `bun run build` pass.
- iPhone 17 simulator (iOS 27.0): the app runs, YouTube search and the live feed work, and a track loads, plays, seeks, pauses, resumes and stops through AVPlayer with core state following (position advanced about 3 s per 3 s; duration read from the player).
- Android emulator (Pixel API 36, arm64): debug APK builds; the same load / play / seek / pause / resume / stop sequence passes through ExoPlayer, a Media3 session registers, and the tab bar clears the gesture bar. A real bug found there and fixed: calling `startForegroundService` without a playing player killed the app after about ten seconds. The plugin now binds a `MediaController`, and Media3 moves the service to the foreground itself.

Verified on 2026-10-25:

- iPhone 18 Pro simulator (iOS 27.0), ad-hoc codesigned release build (`flags=0x2(adhoc)`): Connections reads "Not connected. Public tracks can still play.", i.e. `restore()` → Keychain `store.get()` succeeds in-process. The earlier "OS key store is unavailable or locked" was a stale linker-signed install, not a Keychain code defect; `platform/ios_keyring.rs` is unchanged. Completing Google sign-in still needs a real account (see Not verified).

Not verified:

- Audible output (the position advances, but nobody listened). Lock-screen controls, background playback with the screen off, audio interruptions and headphone unplug.
- Anything on a physical device. No code signing is configured (`APPLE_DEVELOPMENT_TEAM`, Android keystore).
- Google sign-in on both platforms (needs a real account; Google may refuse embedded web views, especially on Android).

When Google sign-in stops on a Google page on a physical iPhone, the sign-in window logs host-only lines as `[youtube sign-in]` in `xcrun devicectl device console` (or in `tmp/lunorx-signin.log` when the app is launched with `LUNORX_SIGNIN_LOG=1`, then copied with `xcrun devicectl device copy from --domain-type tmp`). A `navigation blocked` line names the hop the allowlist refused; a run of `loaded: accounts.google.com` with no `closed: session captured` means Google refused the embedded view instead. The Swift (`loginNavigationAllowed`), Kotlin (`SignInHosts`) and Rust (`login_navigation`) rules must stay in step.
- Android Keystore secrets at runtime, the long-press context menu, the iPad layouts beyond one screenshot, and Windows/Linux desktop builds after these changes.
- Release builds (`tauri ios build`, `tauri android build` without `--debug`).

## Release builds (2026-10-23)

- Android: `tauri android build --apk --target aarch64` gives an unsigned 22 MB APK (debug is 340 MB). The merged manifest has `allowBackup=false` and `usesCleartextTraffic=false`, and the app is not debuggable. Signed with the local debug key for a test only, it installed on the emulator, loaded feeds, searched and played a track with no crash. Ship signing needs your own keystore.
- iOS: `tauri ios build --target aarch64-sim --ci` gives a 17 MB simulator `.app` that launches and loads the live feed. A device or App Store build needs `bundle > iOS > developmentTeam` (or `APPLE_DEVELOPMENT_TEAM`). Playback in the iOS release build and the new Keychain class at runtime were not exercised.
