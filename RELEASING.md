# Releasing

A release is a version tag. Pushing the tag builds the installers and publishes them on GitHub Releases.

## Steps

1. Set the same version in `package.json` and `src-tauri/Cargo.toml` (and `src-tauri/Cargo.lock` via `cargo check`). `tauri.conf.json` has no version on purpose: Tauri reads it from `Cargo.toml`.
2. Commit, then tag and push:

   ```sh
   git tag v1.0.0
   git push origin v1.0.0
   ```

   A version with a suffix (`v1.0.0-rc.1`) is published as a pre-release.
3. The `Release` workflow checks that the tag matches both files, runs lint, build, `cargo fmt`, `clippy` and tests, builds every platform, and attaches the installers plus `SHA256SUMS.txt` to the release.

To build installers without publishing, run the workflow by hand (Actions → Release → Run workflow). The installers stay as workflow artifacts.

## What it produces

| Platform | Files | Status |
|---|---|---|
| macOS Apple silicon, Intel | `.dmg` | libmpv and its libraries are copied into the app, so Homebrew is not needed on the user's Mac |
| Linux x86_64 | `.deb`, `.AppImage` | not verified on a real machine; the `.deb` depends on `libmpv2` |
| Windows x86_64 | `-setup.exe` (NSIS) | not verified on a real machine; `libmpv-2.dll` is installed next to the app |

Linux and Windows jobs are allowed to fail without blocking the release. Check the release assets to see which ones were built.

## Signing (optional)

Without these repository secrets the macOS app is ad-hoc signed and users must approve it in System Settings the first time.

| Secret | Value |
|---|---|
| `MACOS_SIGN_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_CERTIFICATE` | base64 of the `.p12` certificate |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | notarization (app-specific password) |

Windows installers are not code-signed.

## Before a public release

- `rustypipe` is GPL-3.0 and `libmpv` is LGPL. [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) lists the components and what needs attention. The notices exist, but the GPL-3.0 question for the combined work is **not resolved**: decide it before any public release. The `LICENSE` file covers only this repository's own code.
- The notices file ships with the source. Copy it into the installers too once the licence question is settled.
- Run the app from each installer on a clean machine. Only the macOS bundling step has been run locally, on a debug build.
