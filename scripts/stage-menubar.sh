#!/usr/bin/env bash
# =============================================================================
# stage-menubar.sh — build the menu bar app and stage it for nesting
# =============================================================================
#
# Usage: scripts/stage-menubar.sh [--debug]
#
# macOS requires a login item to be a whole `.app`, nested at
# `Contents/Library/LoginItems/`. The nesting itself is **declared**, not
# scripted: `bundle.macOS.files` in `tauri.conf.json` copies it during
# bundling. This script's job is only to put the bundle where that declaration
# points, and to refuse a bundle that would be wrong to nest.
#
# ── Why declared rather than copied afterwards ───────────────────────────────
#
# A post-bundle copy cannot work in the release pipeline. `release.yml` builds
# the `.app` *and* the `.dmg` in one `tauri build`, then signs **the DMG** — so
# an app nested after that is not in the artifact anyone installs, and
# rebuilding the DMG by hand would mean reimplementing Tauri's dmg bundler
# (background image, window geometry and all).
#
# `bundle.macOS.files` runs inside bundling, before the DMG is made, so both
# the DMG and the updater archive carry it. It was measured, not assumed: it
# copies a whole directory and **preserves symlinks** (verified against a
# bundle containing one), which was the reason `ditto` would otherwise have
# been needed. And a missing source fails the bundle by name —
# `Failed to copy "loginitems/…" … does not exist` — which is the loud failure
# a forgotten step should produce.
#
# The staging directory mirrors `desktop/src-tauri/binaries/`, which holds
# staged sidecars for the same reason: a fixed path the config can name,
# whatever profile produced the artifact.
#
# ── What this refuses, and why each one ──────────────────────────────────────
#
#   - **`LSUIElement` must be true.** Without it the login item takes a Dock
#     icon and an app menu — a visible product bug rather than a subtle one.
#   - **The version must match the desktop app's.** A login item whose version
#     drifts from its host is how `SMAppService` starts refusing to refresh a
#     registration, and that reads as "the menu bar icon just stopped
#     appearing after an update". The two versions live in two config files, so
#     this is the only thing standing between them.
#   - **The executable must exist**, named by `CFBundleExecutable` rather than
#     guessed from the bundle name: Tauri names it after the *binary*
#     (`beekeeper-menubar`), not the product name ("Beekeeper Menu Bar").
#
# ── Known over-grant, named rather than discovered ───────────────────────────
#
# `block/apple-codesign-action` takes one `entitlements-plist-path`, and
# `Entitlements.plist` carries audio-input, camera and
# `cs.disable-library-validation`. So the nested tray app is signed with camera
# and microphone entitlements it never uses. Same team id and hardened runtime
# either way, so the risk is small — but it is real, and fixing it needs
# upstream support for a second entitlements file.
# =============================================================================

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

PROFILE=release
BUILD_ARGS=()
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE=debug
    BUILD_ARGS+=(--debug)
fi

MENUBAR_DIR="desktop/src-tauri/crates/beekeeper-menubar"
STAGE_DIR="desktop/src-tauri/loginitems"
BUNDLE_NAME="Beekeeper Menu Bar.app"
TAURI="$REPO/desktop/node_modules/.bin/tauri"

[[ -x "$TAURI" ]] || {
    echo "the tauri CLI is missing at $TAURI — run \`pnpm install\` first" >&2
    exit 1
}

PLIST_BUDDY=/usr/libexec/PlistBuddy
read_plist() { "$PLIST_BUDDY" -c "Print :$2" "$1" 2>/dev/null || true; }

# ── the two versions must agree, and they live in two files ─────────────────
DESKTOP_VERSION=$(python3 -c \
    'import json,sys;print(json.load(open(sys.argv[1]))["version"])' \
    desktop/src-tauri/tauri.conf.json)
MENUBAR_VERSION=$(python3 -c \
    'import json,sys;print(json.load(open(sys.argv[1]))["version"])' \
    "$MENUBAR_DIR/tauri.conf.json")
if [[ "$DESKTOP_VERSION" != "$MENUBAR_VERSION" ]]; then
    echo "refusing to stage: the menu bar app declares version $MENUBAR_VERSION" >&2
    echo "and the desktop app declares $DESKTOP_VERSION. A login item whose" >&2
    echo "version drifts from its host is how SMAppService stops refreshing the" >&2
    echo "registration, which reads as the icon quietly not appearing after an" >&2
    echo "update. Set \"version\" in $MENUBAR_DIR/tauri.conf.json to $DESKTOP_VERSION." >&2
    exit 1
fi

# ── the frontend stub must be in the checkout ────────────────────────────────
# Checked here, before Tauri is invoked, because Tauri's own message for this
# is actively misleading: "Unable to find your web assets, did you forget to
# build your web app?" — there is no web app to build, and never will be. The
# directory holds one tracked HTML file that nothing loads.
#
# This is not hypothetical. It lived in `dist/` first, `desktop/.gitignore`
# ignores `dist`, and so it was never committed: the worktree that wrote it
# built fine and every fresh checkout — CI included — could not build this
# crate at all. Exactly the shape of ledger 296.
FRONTEND=$(python3 -c \
    'import json,sys;print(json.load(open(sys.argv[1]))["build"]["frontendDist"])' \
    "$MENUBAR_DIR/tauri.conf.json")
if [[ ! -f "$MENUBAR_DIR/$FRONTEND/index.html" ]]; then
    echo "refusing to stage: $MENUBAR_DIR/$FRONTEND/index.html is missing." >&2
    echo "It is a tracked stub, not build output — nothing generates it. If this" >&2
    echo "is a clean checkout, the file is being ignored: check" >&2
    echo "\`git check-ignore -v $MENUBAR_DIR/$FRONTEND/index.html\`." >&2
    exit 1
fi

# ── build ────────────────────────────────────────────────────────────────────
(cd "$MENUBAR_DIR" && "$TAURI" build "${BUILD_ARGS[@]}" --bundles app)
BUILT="desktop/src-tauri/target/$PROFILE/bundle/macos/$BUNDLE_NAME"
[[ -d "$BUILT" ]] || { echo "the menu bar bundle was not produced at $BUILT" >&2; exit 1; }

# ── refuse a bundle that would be wrong to nest ──────────────────────────────
PLIST="$BUILT/Contents/Info.plist"
LSUI=$(read_plist "$PLIST" LSUIElement)
if [[ "$LSUI" != "true" && "$LSUI" != "1" ]]; then
    echo "refusing to stage: LSUIElement is ${LSUI:-<unset>}, so this would take" >&2
    echo "a Dock icon and an app menu instead of being an accessory." >&2
    exit 1
fi
BUNDLE_VERSION=$(read_plist "$PLIST" CFBundleVersion)
[[ "$BUNDLE_VERSION" == "$DESKTOP_VERSION" ]] || {
    echo "refusing to stage: the built bundle says CFBundleVersion" >&2
    echo "$BUNDLE_VERSION but the desktop app is $DESKTOP_VERSION." >&2
    exit 1
}
EXECUTABLE=$(read_plist "$PLIST" CFBundleExecutable)
[[ -n "$EXECUTABLE" ]] || { echo "the bundle declares no CFBundleExecutable" >&2; exit 1; }
[[ -x "$BUILT/Contents/MacOS/$EXECUTABLE" ]] || {
    echo "the bundle has no executable at Contents/MacOS/$EXECUTABLE" >&2
    exit 1
}

# ── stage ────────────────────────────────────────────────────────────────────
mkdir -p "$STAGE_DIR"
rm -rf "$STAGE_DIR/$BUNDLE_NAME"
# `ditto`, not `cp -R`: symlinks, resource forks and modes. The repo already
# reaches for it for exactly this, in scripts/app-from.sh and release.yml.
ditto "$BUILT" "$STAGE_DIR/$BUNDLE_NAME"

echo "Staged $BUNDLE_NAME ($PROFILE) at $STAGE_DIR"
echo "  version $BUNDLE_VERSION, LSUIElement true, executable $EXECUTABLE"
echo "  tauri.conf.json's bundle.macOS.files nests it at Contents/Library/LoginItems"
