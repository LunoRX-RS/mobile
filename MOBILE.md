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
The core still owns playback state. A track is `PLAYING` only after the platform player confirms it. Lock-screen and headphone presses are queued by the plugin as `remote` events and sent back through the same core path as the in-app buttons (`platform/control.rs`).

### Feature gap vs desktop (verified against `../desktop` on 2026-10-25, after the parity pass)

Three honest categories:

**Not implemented here at all.** Backend: error reports and the activity log (`commands/reports.rs`, `core/errorlog`, `core/activity`), `discord_preview`, taste search (`core/playback/taste`), search cache and ranking (`core/search/{cache,rank}`). UI: TrackRail, the desktop QueuePanel (mobile uses the queue drawer over `NativeQueue`/`QueueScreen`), UpdateBanner / ErrorReportBanner (the updater is store-forbidden here anyway), Activity / Reports settings, the 11-zone settings IA (mobile keeps its own six categories with the storage rows inside the cache section).

**Added in the 2026-10-25 parity pass** (ported from `../desktop`): share links end to end — deep-link plugin + `commands/share` + `core/share_link` + `core/secrets` registry, the Shared screen, `lunorx://share/{token}` and `play.lunorx.xyz/s|/embed|/og/{token}` opening the app, paste-a-token fallback; `start_auto_sync` (60 s flush while signed in); `sync_check_playlists` remote-state labels and `sync_set_playlist_description` editor; `youtube_import_playlist_from_url` plus the editor's Import-from-link form; `offline_usage` / `offline_clear_transient` with the meter, Clear-cached and kept-offline list in Settings → Playback cache; Replay screen, LinkPlay (pasted YouTube links play), NoticeBar + `notify`/`attempt` on silent failures, SyncAllButton, share links shown again per playlist. Mobile drops the desktop Stats screen on purpose (2026-10-25 user decision): Replay is the only listening-stats screen here, in the phone More sheet and the tablet sidebar; the `stats_*` commands stay registered for Replay and Sync-all.

**Impossible on a phone — stays desktop-only.** Tray icon, app menu, MPRIS, libmpv audio-device picking, the self-updater (`tauri-plugin-updater`; app stores forbid self-update), single-instance, keyboard shortcuts, and Discord presence over the local IPC socket (no Discord app on iOS/Android).

**Present but gated off phones, not deleted.** Discord settings (commands still compiled and registered), app zoom (`setZoom` + capability remain), system / imported fonts (`platform_families()` returns empty on mobile) — all hidden via `DESKTOP_ONLY_GROUPS` / `DESKTOP_ONLY_CATEGORIES` in `src/lib/layout.ts`. `Make lyrics file` is hidden only on phones; it works on tablet widths.

**Share deep links — re-apply after regenerating `gen/`.** The iOS `lunorx` URL scheme needs no manual step on device builds: `bundle.iOS.infoPlist: "Info.ios.plist"` in `tauri.conf.json` merges `src-tauri/Info.ios.plist` (background audio, ATS, `CFBundleURLTypes`) into the generated Info.plist — edits made by hand to `gen/apple/lunorx_iOS/Info.plist` are overwritten. The merge applies to device archives only: a `--target aarch64-sim` product ships **without** those keys, so `simctl openurl lunorx://…` fails (LSApplicationWorkspace error 115) and background audio / local-ATS are missing in the simulator. `scripts/ios-build.sh` now re-inserts the three keys into the sim .app and re-signs ad-hoc after the build. `src-tauri/gen/` is otherwise gitignored: after `tauri android init` re-add the two `MainActivity` intent-filters (`lunorx://share`, and `autoVerify` https for `play.lunorx.xyz` with `/s/ /embed/ /og/` prefixes) to `gen/android/app/src/main/AndroidManifest.xml` — the file's comment carries the snippet. The `plugins.deep-link.mobile` block (hosts for `play.lunorx.xyz`) is deliberately **not** in `tauri.conf.json` yet: while it is set, the iOS archive injects the Associated Domains entitlement and fails (`Provisioning profile … doesn't include the Associated Domains capability` / `No Accounts`). Add it back once the App ID is provisioned for Associated Domains **and** `play.lunorx.xyz` serves `/.well-known/apple-app-site-claim` (iOS) and `/.well-known/assetlinks.json` (Android). Until then taps on `play.lunorx.xyz` links do not open the app by themselves; `lunorx://` links and pasting a link or token into Search or the playlist editor's Import-from-link still open the Shared screen.

### Podcasts (mobile only)

The desktop app has no podcast view. Shows are found through Apple's public iTunes Search API (the global and the Thai storefront, merged) and their episodes are read from the show's own RSS feed (`core/rss_podcast.rs`, `commands/podcasts.rs::podcast_shows` / `podcast_episodes`). An episode is a plain `https` audio file: `ProviderId::Podcast`, with the audio address as both the track id and the address. `PodcastHost` (`core/playback/mobile.rs`) probes the file for byte-range support and serves it to the native player through the loopback proxy (`AudioProxy::stream_open`), so it plays like a song: media card, lock screen, background. Every address that reaches the network passes `rss_podcast::public_https` (https only, a domain name, no local names, no credentials), redirects are checked hop by hop, and the image CSP now allows `https:` because show art lives on many hosts.

Choosing an episode while a song plays (or the reverse) stops the first: one provider plays at a time, and a queue never mixes them. Episodes queue behind each other from the list they were chosen in; autoplay's related-music radio is YouTube-only and is not used after an episode. Episodes are not recorded in play history or listening stats, and cannot be liked or added to playlists. A feed host that does not answer range requests cannot be played (typed as unsupported). Shows only on YouTube (the Thai "The Ghost Radio" has no public feed) stay reachable through the Podcasts screen's YouTube fallback (`core/podcasts.rs`: long YouTube videos), which plays through the page player and can carry YouTube's ads and stop in the background.

## The `lunor-native` plugin

`src-tauri/plugins/lunor-native/` is a Tauri plugin with no webview commands: only Rust can call it, so remote content cannot reach native audio or secrets. Swift (`ios/Sources/LunorNativePlugin.swift`) and Kotlin (`android/.../LunorNativePlugin.kt`) implement one JSON contract:

`load {url}` · `pageLoad {videoId}` (Android only; iOS rejects it) · `play` · `pause` · `stop` · `seek {positionMs}` · `setVolume {volume}` · `poll` → `{status, positionMs, durationMs, remote[]}` · `nowPlaying {...}` · `signIn` → `{cookies[]}` · Android only: `secretGet/Set/Delete`.

`poll` is read by `core/playback/mobile.rs::watch` every 250 ms while a track is loaded (1 s when idle). The platform latches `ended`/`failed`, so each transition is seen once.

Only `file://` URLs (the disk cache) and `http://127.0.0.1` (loopback proxy) are accepted as sources.

### YouTube page fallback (Android)

When the direct stream is refused (`Failed` or `MediaRefused`), `core/playback/routing.rs::RoutingHost` asks the plugin to `pageLoad` the track: `PagePlayer.kt` opens `https://www.youtube.com/watch?v=<id>` in a hidden, attached 1x1 web view (no JavaScript bridge, navigation limited to the sign-in host rules, `m.youtube.com` included) and reads the page's own player state from its title. This is the same fallback the desktop app has. While the page holds the track, `play/pause/seek/setVolume/stop/poll` act on it; every new load starts from the direct player again. If the page fails too, the direct refusal is the error shown. Not done: sound effects in the page, the lock-screen/notification card (the media session follows ExoPlayer, not the page), and playback with the screen off is not verified. iOS has no page player.

### Launcher icon

`src-tauri/gen/` is git-ignored and `tauri android init` writes Tauri's default icon there once. `scripts/sync-android-icons.sh` (run by `mobile-env.sh` before every command) copies `src-tauri/icons/android/*` into it, so each APK carries the project icon. A launcher caches icons; if the old one stays, uninstall the app once (this clears sign-in and cache).


## iOS audio format

AVPlayer cannot play Opus in WebM. On iOS the stream selection (`extract.rs::platform_playable`, `visionos.rs::select`) keeps AAC in MP4 only. The cache names files `*.audio`, which AVPlayer cannot sniff, so the plugin passes `AVURLAssetOutOfBandMIMETypeKey: audio/mp4`. That key is not in Apple's public documentation; if a future iOS rejects it, rename the cache file to `.m4a` instead.

**Duration.** YouTube progressive-fragmented m4a carries a placeholder `moov` whose timescale pair makes AVFoundation report exactly twice the true duration (measured on 8/8 cached streams; no asset option fixes it). Position still advances 1:1 with wall time, so after the real end AVPlayer runs silent until the doubled timeline ends. The plugin therefore parses the first top-level `sidx` box from the cached file (its entry durations sum to the true length; YouTube muxes the box with a 24-byte header, four longer than ISO 14496-12, so the code finds the entry table by self-validation) and reports that as `durationMs` in `poll` whenever the asset value exceeds it by >5%. It also ends the track when position passes the `sidx` end, so "ended", autoplay and the lock-screen cards fire at the true time. The corrected duration flows through core into `nowPlaying` and the LRCLIB lyrics lookup (±12 s match window), which the doubled value had been silently poisoning. If `sidx` is absent or unparsable, the asset duration is used unchanged.

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

### Lock screen and Dynamic Island (native only)

The lock screen and the Dynamic Island are iOS' native Now Playing presentation — no app extension, no Live Activity. `nowPlaying` fills `MPNowPlayingInfoCenter` (artwork, title, artist, elapsed, rate) and `MPRemoteCommandCenter` serves prev / play-pause / next, which iOS renders on the lock screen, in the island, on AirPods and as "Now Playing on iPhone" in macOS Control Center.

A custom lyrics card used to exist as a WidgetKit Live Activity (`LunoRXLiveActivity`) and was removed at the user's request. Two hard platform facts drove the trouble: every Live Activity must occupy a Dynamic Island slot (the SDK has no zero-region presentation; empty views render as a black pill), so ours always stacked a second pill beside the native Now Playing one; and ActivityKit caps attributes + state at a combined 4 KB, which the artwork/lyric payload repeatedly exceeded — over the limit the system rejects the whole activity and nothing renders. Rust and the plugin contract are unchanged apart from dropping the payload's optional `lyric`/`lyricWord`; `scripts/ios-build.sh` is now a plain `mobile-env.sh` wrapper around `tauri ios build`.

Signing lives in `gen/apple/project.yml` (`CODE_SIGN_STYLE: Automatic`, `DEVELOPMENT_TEAM`); `tauri ios init` regenerates that file, so re-add those two settings after a re-init. Device builds then run from the command line with the existing profile for `io.lunorx.mobile` (the extension's App ID/profile are no longer needed).

Simulator builds must go through a real codesign pass. Never set `CODE_SIGNING_ALLOWED: NO`. A binary left to the linker's automatic signature (`codesign -dvv` shows `flags=…linker-signed`) is refused by securityd for Keychain access, so `store.get()` errors and Connections shows "The OS key store is unavailable or locked" instead of "Not connected" — Google sign-in can never complete on such a build. A correctly signed simulator build shows `flags=0x2(adhoc)`.

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
- Physical iPhone: builds install and run (free-team profile for `io.lunorx.mobile`, expires 2026-10-31); the system lock-screen card and Dynamic Island were confirmed there. The custom Live Activity lyrics card was later removed (see "Lock screen and Dynamic Island"); the `io.lunorx.mobile.LiveActivity` profile it needed is no longer used. Android emulator after this run of Live Activity work: not re-run.
- Google sign-in on both platforms (needs a real account; Google may refuse embedded web views, especially on Android).

When Google sign-in stops on a Google page on a physical iPhone, the sign-in window logs host-only lines as `[youtube sign-in]` in `xcrun devicectl device console` (or in `tmp/lunorx-signin.log` when the app is launched with `LUNORX_SIGNIN_LOG=1`, then copied with `xcrun devicectl device copy from --domain-type tmp`). A `navigation blocked` line names the hop the allowlist refused; a run of `loaded: accounts.google.com` with no `closed: session captured` means Google refused the embedded view instead. The Swift (`loginNavigationAllowed`), Kotlin (`SignInHosts`) and Rust (`login_navigation`) rules must stay in step.
- Android Keystore secrets at runtime, the long-press context menu, the iPad layouts beyond one screenshot, and Windows/Linux desktop builds after these changes.
- Release builds (`tauri ios build`, `tauri android build` without `--debug`).

## Release builds (2026-10-23)

- Android: `tauri android build --apk --target aarch64` gives an unsigned 22 MB APK (debug is 340 MB). The merged manifest has `allowBackup=false` and `usesCleartextTraffic=false`, and the app is not debuggable. Signed with the local debug key for a test only, it installed on the emulator, loaded feeds, searched and played a track with no crash. Ship signing needs your own keystore.
- iOS: `tauri ios build --target aarch64-sim --ci` gives a 17 MB simulator `.app` that launches and loads the live feed. A device or App Store build needs `bundle > iOS > developmentTeam` (or `APPLE_DEVELOPMENT_TEAM`). Playback in the iOS release build and the new Keychain class at runtime were not exercised.
