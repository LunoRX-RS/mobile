#!/usr/bin/env python3
"""Adds the Dynamic Island / lock-screen Live Activity extension to the generated Xcode project.

`src-tauri/gen/` is gitignored and rebuilt by `tauri ios init`, so the extension is described here and patched into
`gen/apple/project.yml` before every build. Sources live outside `gen/`:

  src-tauri/ios/LiveActivity/Widget/      widget extension (SwiftUI, Info.plist)
  src-tauri/ios/LiveActivity/Intents/     transport buttons, compiled into the app and the extension
  src-tauri/plugins/lunor-native/ios/     shared attributes + the queue both sides use

Usage:
  python3 scripts/ios-live-activity.py            patch, then run xcodegen
  python3 scripts/ios-live-activity.py --check    verify the patch is in place, change nothing

The patch is idempotent: marked blocks are removed and re-inserted, so running it after every `tauri ios init` (or on an
already patched file) gives the same result. Signing: with APPLE_DEVELOPMENT_TEAM set, both the app and the extension
switch to automatic signing with that team; otherwise both get an ad-hoc "Sign to Run Locally" identity. The ad-hoc
codesign step is required, not optional: a build left to the linker's automatic signature (`flags=…linker-signed`) is
refused by securityd for Keychain access, which surfaces in-app as the "OS key store is unavailable or locked" error and
blocks Google sign-in.
"""

import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APPLE = ROOT / "src-tauri/gen/apple"
SPEC = APPLE / "project.yml"
REQUESTED = APPLE / "lunorx.xcodeproj/project.pbxproj"
INFO = ROOT / "src-tauri/Info.ios.plist"
CONF = ROOT / "src-tauri/tauri.conf.json"

BEGIN_SOURCES = "      # >>> lunor-live-activity sources"
END_SOURCES = "      # <<< lunor-live-activity sources"
BEGIN_DEPS = "      # >>> lunor-live-activity dependencies"
END_DEPS = "      # <<< lunor-live-activity dependencies"
BEGIN_TARGET = "  # >>> lunor-live-activity target"
END_TARGET = "  # <<< lunor-live-activity target"

# Ad-hoc "Sign to Run Locally"; the linker's automatic signature is refused by securityd for Keychain access.
LOCAL_SIGN = (
    "        CODE_SIGNING_ALLOWED: YES\n"
    "        CODE_SIGNING_REQUIRED: NO\n"
    '        CODE_SIGN_IDENTITY: "-"\n'
)
# Older revisions disabled signing entirely; upgrade those in place.
LEGACY_NO_SIGNING = (
    "        CODE_SIGNING_ALLOWED: NO\n"
    "        CODE_SIGNING_REQUIRED: NO\n"
    '        CODE_SIGN_IDENTITY: ""\n'
)

failures = []


def fail(message):
    failures.append(message)
    print("ios-live-activity: " + message, file=sys.stderr)


def strip_block(text, begin, end):
    if begin not in text and end not in text:
        return text
    if text.count(begin) != 1 or text.count(end) != 1:
        raise SystemExit("ios-live-activity: unbalanced markers %r / %r in project.yml" % (begin, end))
    head, _, rest = text.partition(begin)
    _, _, tail = rest.partition(end)
    return head + tail.lstrip("\n")


def insert_once(text, anchor, block):
    count = text.count(anchor)
    if count != 1:
        raise SystemExit("ios-live-activity: expected one %r in project.yml, found %d (structure changed?)" % (anchor, count))
    index = text.index(anchor)
    return text[:index] + block + text[index:]


def signing_block(team):
    if team:
        return "        CODE_SIGN_STYLE: Automatic\n        DEVELOPMENT_TEAM: %s\n" % team
    return LOCAL_SIGN


AUTOMATIC = re.compile(r"        CODE_SIGN_STYLE: Automatic\n        DEVELOPMENT_TEAM: \S+\n")


def patch_signing(text, team):
    # The app target is the only place that carries signing overrides; the extension block below is rebuilt each run.
    if LEGACY_NO_SIGNING in text:
        text = text.replace(LEGACY_NO_SIGNING, LOCAL_SIGN, 1)
    if team:
        if LOCAL_SIGN in text:
            return text.replace(LOCAL_SIGN, signing_block(team), 1)
        current = AUTOMATIC.search(text)
        if not current:
            raise SystemExit("ios-live-activity: signing block in project.yml not recognised")
        return text[: current.start()] + signing_block(team) + text[current.end() :]
    current = AUTOMATIC.search(text)
    if current:
        return text[: current.start()] + LOCAL_SIGN + text[current.end() :]
    if LOCAL_SIGN in text:
        return text
    raise SystemExit("ios-live-activity: signing block in project.yml not recognised")


def extension_target(team):
    lines = [
        BEGIN_TARGET,
        "  LunoRXLiveActivity:",
        "    type: app-extension",
        "    platform: iOS",
        '    deploymentTarget: "17.0"',
        "    sources:",
        "      - path: ../../ios/LiveActivity/Widget/LunoRXLiveActivityWidget.swift",
        "      - path: ../../ios/LiveActivity/Intents",
        "      - path: ../../plugins/lunor-native/ios/Sources/LunorActivityAttributes.swift",
        "      - path: ../../plugins/lunor-native/ios/Sources/LunorRemoteQueue.swift",
        "    settings:",
        "      base:",
        "        PRODUCT_NAME: LunoRXLiveActivity",
        "        PRODUCT_BUNDLE_IDENTIFIER: io.lunorx.mobile.LiveActivity",
        "        SKIP_INSTALL: YES",
        "        APPLICATION_EXTENSION_API_ONLY: YES",
        '        SWIFT_VERSION: "5.0"',
        '        TARGETED_DEVICE_FAMILY: "1,2"',
        '        MARKETING_VERSION: "1.0.0"',
        '        CURRENT_PROJECT_VERSION: "1.0.0.3"',
        "        GENERATE_INFOPLIST_FILE: NO",
        "        INFOPLIST_FILE: ../../ios/LiveActivity/Widget/Info.plist",
    ]
    lines.extend(signing_block(team).rstrip("\n").split("\n"))
    lines.extend(
        [
            "    dependencies:",
            "      - sdk: WidgetKit.framework",
            "      - sdk: SwiftUI.framework",
            "      - sdk: ActivityKit.framework",
            END_TARGET,
            "",
        ]
    )
    return "\n".join(lines)


def patched(text, team):
    text = strip_block(text, BEGIN_SOURCES, END_SOURCES)
    text = strip_block(text, BEGIN_DEPS, END_DEPS)
    text = strip_block(text, BEGIN_TARGET, END_TARGET)

    text = re.sub(
        r"(?m)^(    iOS: )[\d.]+$",
        r"\g<1>17.0",
        text,
        count=1,
    )
    if "    iOS: 17.0" not in text:
        raise SystemExit("ios-live-activity: options.deploymentTarget.iOS not found in project.yml")

    text = patch_signing(text, team)

    sources = "\n".join(
        [
            BEGIN_SOURCES,
            "      - path: ../../ios/LiveActivity/Intents",
            END_SOURCES,
        ]
    )
    text = insert_once(text, "\n    info:\n", "\n" + sources)

    deps = "\n".join(
        [
            BEGIN_DEPS,
            "      - target: LunoRXLiveActivity",
            "        embed: true",
            "      - sdk: ActivityKit.framework",
            END_DEPS,
        ]
    )
    text = insert_once(text, "\n    preBuildScripts:\n", "\n" + deps)

    if not text.endswith("\n"):
        text += "\n"
    text += extension_target(team)
    return text


def verify_plist():
    try:
        with INFO.open("rb") as handle:
            data = plistlib.load(handle)
    except Exception as error:  # missing or malformed counts as not patched
        fail("cannot read src-tauri/Info.ios.plist: %s" % error)
        return
    if data.get("NSSupportsLiveActivities") is not True:
        fail("src-tauri/Info.ios.plist is missing NSSupportsLiveActivities=true")


def verify_conf():
    try:
        data = json.loads(CONF.read_text())
    except Exception as error:
        fail("cannot read src-tauri/tauri.conf.json: %s" % error)
        return
    minimum = data.get("bundle", {}).get("iOS", {}).get("minimumSystemVersion")
    if minimum != "17.0":
        fail("tauri.conf.json bundle.iOS.minimumSystemVersion is %r, expected 17.0" % (minimum,))


def verify_spec(text):
    if "    iOS: 17.0" not in text:
        fail("project.yml deployment target is not 17.0")
    for marker in (BEGIN_SOURCES, END_SOURCES, BEGIN_DEPS, END_DEPS, BEGIN_TARGET, END_TARGET):
        if text.count(marker) != 1:
            fail("project.yml marker %r is missing or duplicated" % marker)
    if "      - path: ../../ios/LiveActivity/Intents\n" not in text:
        fail("the app target does not compile the Live Activity intents")
    if "        embed: true\n" not in text or "- target: LunoRXLiveActivity" not in text:
        fail("the app target does not embed LunoRXLiveActivity")


def verify_pbxproj():
    if not REQUESTED.exists():
        fail("lunorx.xcodeproj is missing; run without --check to generate it")
        return
    project = REQUESTED.read_text()
    # xcodegen on recent Xcode writes "Embed Foundation Extensions"; older versions say "Embed App Extensions".
    if not re.search(r"LunoRXLiveActivity\.appex in Embed (Foundation|App) Extensions", project):
        fail("project.pbxproj has no Embed Extensions entry for LunoRXLiveActivity")
    if "PRODUCT_BUNDLE_IDENTIFIER = io.lunorx.mobile.LiveActivity;" not in project:
        fail("project.pbxproj has no io.lunorx.mobile.LiveActivity target")


def run_xcodegen():
    tool = shutil.which("xcodegen")
    if not tool:
        raise SystemExit("ios-live-activity: xcodegen not found (brew install xcodegen)")
    subprocess.run([tool, "generate", "--spec", "project.yml"], cwd=APPLE, check=True)


def main(argv):
    check = "--check" in argv[1:]
    if not SPEC.exists():
        print(
            "ios-live-activity: %s not found. Run `tauri ios init` (or a first `tauri ios build`) first." % SPEC,
            file=sys.stderr,
        )
        return 2

    team = os.environ.get("APPLE_DEVELOPMENT_TEAM", "").strip()
    original = SPEC.read_text()
    updated = patched(original, team)

    verify_plist()
    verify_conf()
    verify_spec(updated)

    if check:
        if updated != original:
            fail("project.yml is not patched; run scripts/ios-build.sh or this script without --check")
        verify_pbxproj()
    else:
        if updated != original:
            temporary = SPEC.with_name(SPEC.name + ".tmp")
            temporary.write_text(updated)
            temporary.replace(SPEC)
            print("ios-live-activity: patched gen/apple/project.yml")
        else:
            print("ios-live-activity: gen/apple/project.yml already patched")
        run_xcodegen()
        verify_pbxproj()

    if failures:
        return 1
    print("ios-live-activity: ok" + (" (new-session build)" if team else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
