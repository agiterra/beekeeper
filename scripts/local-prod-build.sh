#!/usr/bin/env bash
# local-prod-build.sh — build the local production Buzz.app from a build/* tag
# and install it to /Applications.
#
# The bundle is the stock Buzz identity (productName "Buzz", identifier
# xyz.block.buzz.app, release-profile keyring service "buzz-desktop"), so it
# coexists with `just desktop-standalone` dev instances (identifier
# xyz.block.buzz.app.dev*). It is unsigned (linker ad-hoc), exactly like
# `just desktop-release-build` — macOS will ask for the login keychain once on
# the first launch after every update; that is expected and accepted.
#
# Builds happen in a dedicated detached worktree (default
# ~/Code/lightyear/buzz-prod, override with BUZZ_PROD_WORKTREE) so they never
# contend with the assembly worktree or a running dev instance. The bundle
# additionally carries buzz-session-provider as a sidecar (delta config
# tauri.local-prod.conf.json — not part of the tagged tree, so it is copied in
# from this checkout before the build).
#
# Usage: scripts/local-prod-build.sh [build-tag] [--no-install]
#   build-tag     defaults to the newest build/* tag (version sort)
#   --no-install  build and verify only; skip the /Applications install
set -euo pipefail

GLUE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROD_ROOT="${BUZZ_PROD_WORKTREE:-$HOME/Code/lightyear/buzz-prod}"
TARGET=aarch64-apple-darwin

TAG=""
NO_INSTALL=false
for arg in "$@"; do
  case "$arg" in
    --no-install) NO_INSTALL=true ;;
    -*) echo "unknown flag: $arg" >&2; exit 1 ;;
    *) TAG="$arg" ;;
  esac
done

git -C "$GLUE_ROOT" fetch --tags --quiet origin
if [[ -z "$TAG" ]]; then
  TAG="$(git -C "$GLUE_ROOT" tag -l 'build/*' | sort -V | tail -1)"
  [[ -n "$TAG" ]] || { echo "no build/* tags found" >&2; exit 1; }
fi
git -C "$GLUE_ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
  || { echo "tag not found: $TAG" >&2; exit 1; }
echo "==> building Buzz.app from $TAG"

# ── worktree, detached at the tag ────────────────────────────────────────────
if [[ ! -d "$PROD_ROOT" ]]; then
  git -C "$GLUE_ROOT" worktree add --detach "$PROD_ROOT" "$TAG"
else
  git -C "$PROD_ROOT" checkout --detach --quiet "$TAG"
fi

cd "$PROD_ROOT"
export PATH="$PROD_ROOT/bin:$PATH"

# The delta config lives on integration/glue, not in the tagged tree; copy it
# into the (gitignored) target dir immediately so the rest of the build is
# immune to branch switches in the glue checkout while this runs.
mkdir -p desktop/src-tauri/target
cp "$GLUE_ROOT/desktop/src-tauri/tauri.local-prod.conf.json" desktop/src-tauri/target/local-prod.conf.json

# ── release sidecars + coding-session provider ───────────────────────────────
cargo build --release -p buzz-acp -p buzz-agent -p buzz-backend-kubernetes \
  -p buzz-dev-mcp -p buzz-cli -p git-credential-nostr -p buzz-session-provider \
  -p buzz-shell-host
./scripts/bundle-sidecars.sh
cp target/release/buzz-session-provider "desktop/src-tauri/binaries/buzz-session-provider-$TARGET"
chmod 755 "desktop/src-tauri/binaries/buzz-session-provider-$TARGET"

# ── bundle ───────────────────────────────────────────────────────────────────
pnpm install
(cd desktop && pnpm tauri build --target "$TARGET" --features mesh-llm --bundles app \
  --config "$PROD_ROOT/desktop/src-tauri/target/local-prod.conf.json")

APP="$PROD_ROOT/desktop/src-tauri/target/$TARGET/release/bundle/macos/Buzz.app"

# ── seal + verify ────────────────────────────────────────────────────────────
[[ -d "$APP" ]] || { echo "bundle missing: $APP" >&2; exit 1; }
[[ -x "$APP/Contents/MacOS/buzz-session-provider" ]] \
  || { echo "buzz-session-provider missing from bundle" >&2; exit 1; }
# Tauri leaves the bundle unsigned (only per-binary linker ad-hoc signatures,
# no CodeResources seal). Ad-hoc sign the whole bundle so it verifies as a
# unit; the signature still changes every build, hence the per-update keychain
# prompt.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP" \
  || { echo "bundle signature verification failed" >&2; exit 1; }
echo "==> bundle OK: $APP"

# ── install ──────────────────────────────────────────────────────────────────
if $NO_INSTALL; then
  echo "==> --no-install: skipping /Applications install"
  exit 0
fi
if pgrep -f "/Applications/Buzz.app/Contents/MacOS/Buzz" >/dev/null; then
  echo "Buzz.app is running — quit it, then re-run the install" >&2
  exit 1
fi
rm -rf /Applications/Buzz.app
ditto "$APP" /Applications/Buzz.app
echo "==> installed $TAG -> /Applications/Buzz.app"
echo "    First launch will ask for the login keychain once — expected after every update."
