#!/usr/bin/env bash
# Makes a built LunoRX.app self-contained and packs it into a DMG.
#   usage: bundle-macos.sh <path/to/App.app> <output.dmg>
# Needs: Homebrew mpv and dylibbundler. Signing is optional (see below).
# Env (all optional): MACOS_SIGN_IDENTITY, APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD
set -euo pipefail

APP="$1"
DMG="$2"
EXE="$(find "$APP/Contents/MacOS" -type f -perm -u+x | head -n 1)"
FRAMEWORKS="$APP/Contents/Frameworks"
BREW="$(brew --prefix)"

# 1. Copy libmpv and everything it links to into the app and point the executable at the copies.
dylibbundler -od -b -cd -x "$EXE" -d "$FRAMEWORKS" -p "@executable_path/../Frameworks/" -s "$BREW/lib" -s "$BREW/opt/mpv/lib"

# 2. Fail the build if the app would still need Homebrew on the user's machine.
if otool -L "$EXE" "$FRAMEWORKS"/*.dylib | grep -E "$BREW|/usr/local/(opt|Cellar)" ; then
  echo "error: the bundle still links to Homebrew paths" >&2
  exit 1
fi

# 3. Sign. Without an identity the app gets an ad-hoc signature: it runs, but Gatekeeper asks the user to approve it.
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  find "$FRAMEWORKS" -name '*.dylib' -print0 | xargs -0 -n 1 codesign --force --timestamp --options runtime --sign "$MACOS_SIGN_IDENTITY"
  codesign --force --timestamp --options runtime --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  codesign --force --deep --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"

# 4. DMG with the usual drag-to-Applications layout.
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "LunoRX" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"

# 5. Notarize and staple when Apple credentials are present.
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$DMG"
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$DMG"
  fi
fi
echo "Created $DMG"
