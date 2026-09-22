#!/usr/bin/env bash
# local-prod-build.sh — build the local production Beekeeper.app from any
# commit-ish and install it to /Applications.
#
# The bundle is the release Beekeeper identity (productName "Beekeeper",
# identifier io.agiterra.beekeeper.app, release-profile keyring service
# "beekeeper-desktop"), so it coexists both with `just desktop-standalone` dev
# instances (identifier io.agiterra.beekeeper.app.dev*) and with a stock
# Buzz.app. It is unsigned (linker ad-hoc), exactly like
# `just desktop-release-build` — macOS will ask for the login keychain once on
# the first launch after every update; that is expected and accepted.
#
# Builds happen in a dedicated detached worktree — a sibling of this clone's
# main worktree named `<clone>-prod` (override with BUZZ_PROD_WORKTREE) — so
# they never contend with the working checkout or a running dev instance. It is
# derived from the main worktree rather than the invoking directory, so every
# worktree of a clone shares one prod tree; "/Applications/Beekeeper.app" is a
# single destination, so a second one would only fight over it. The bundle
# additionally carries buzz-session-provider as a sidecar, via the tracked
# delta config desktop/src-tauri/tauri.local-prod.conf.json.
#
# Usage: scripts/local-prod-build.sh [rev] [--no-install]
#   rev           any commit-ish; defaults to the newest build/* tag if one
#                 exists, else the current branch's upstream, else HEAD
#   --no-install  build and verify only; skip the /Applications install
set -euo pipefail

SRC_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Absolute path of a checkout's main worktree. Git lists it first in
# `worktree list`, and every worktree of a clone reports the same answer, so
# this doubles as the clone's identity.
main_worktree_of() {
  git -C "$1" worktree list --porcelain 2>/dev/null \
    | awk '/^worktree /{ print substr($0, 10); exit }'
}

SRC_MAIN="$(main_worktree_of "$SRC_ROOT")"
[[ -n "$SRC_MAIN" ]] || { echo "not a git checkout: $SRC_ROOT" >&2; exit 1; }
PROD_ROOT="${BUZZ_PROD_WORKTREE:-${SRC_MAIN}-prod}"
TARGET=aarch64-apple-darwin

REV=""
NO_INSTALL=false
for arg in "$@"; do
  case "$arg" in
    --no-install) NO_INSTALL=true ;;
    -*) echo "unknown flag: $arg" >&2; exit 1 ;;
    *) REV="$arg" ;;
  esac
done

git -C "$SRC_ROOT" fetch --tags --quiet origin || true
if [[ -z "$REV" ]]; then
  # Prefer a build tag when one exists — it is the rollback pin. A fresh clone
  # has none, so fall back to the tracked upstream and finally to HEAD rather
  # than refusing to build.
  REV="$(git -C "$SRC_ROOT" tag -l 'build/*' | sort -V | tail -1)"
  [[ -n "$REV" ]] || REV="$(git -C "$SRC_ROOT" rev-parse -q --verify '@{upstream}' 2>/dev/null || true)"
  [[ -n "$REV" ]] || REV=HEAD
fi
SHA="$(git -C "$SRC_ROOT" rev-parse -q --verify "${REV}^{commit}" 2>/dev/null || true)"
[[ -n "$SHA" ]] || { echo "not a commit: $REV" >&2; exit 1; }
echo "==> building Beekeeper.app from $REV ($(git -C "$SRC_ROOT" rev-parse --short "$SHA"))"

# ── worktree, detached at the requested commit ───────────────────────────────
# Objects live in *this* clone. A prod tree belonging to a different clone
# cannot see them, and the checkout below then fails as git's thoroughly
# unhelpful "--detach does not take a path argument" — it parsed the unknown
# rev as a pathspec. Say what is actually wrong instead.
if [[ -e "$PROD_ROOT" ]]; then
  PROD_MAIN="$(main_worktree_of "$PROD_ROOT")"
  if [[ "$PROD_MAIN" != "$SRC_MAIN" ]]; then
    echo "prod worktree belongs to another clone (or is not a git worktree):" >&2
    echo "  prod tree: $PROD_ROOT -> ${PROD_MAIN:-<not a git worktree>}" >&2
    echo "  this clone: $SRC_MAIN" >&2
    echo "Unset BUZZ_PROD_WORKTREE, or point it at a worktree of this clone." >&2
    exit 1
  fi
fi
if [[ ! -d "$PROD_ROOT" ]]; then
  git -C "$SRC_ROOT" worktree add --detach "$PROD_ROOT" "$SHA"
else
  git -C "$PROD_ROOT" checkout --detach --quiet "$SHA"
fi

cd "$PROD_ROOT"
export PATH="$PROD_ROOT/bin:$PATH"

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
  --config "$PROD_ROOT/desktop/src-tauri/tauri.local-prod.conf.json")

APP="$PROD_ROOT/desktop/src-tauri/target/$TARGET/release/bundle/macos/Beekeeper.app"

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
# Match the bundle's MacOS directory, not a binary name. Tauri leaves
# CFBundleExecutable as the Cargo binary, so a productName-based pattern never
# matches and the guard silently passes over a running app — the pre-rename
# version had exactly that bug, looking for `MacOS/Buzz` against a process
# whose real path was `MacOS/buzz-desktop`.
#
# Naming the *current* binary reintroduces it across any rename, in the one
# direction that matters: the installed app is by definition the OLD build, so
# right after `buzz-desktop` became `beekeeper-desktop` the guard stopped
# recognising every app it was meant to catch, and the install would `rm -rf` a
# running bundle. The directory is what is actually invariant here.
# Name what is actually holding the bundle. The match is the directory, so a
# *sidecar* that outlived the app (a buzz-shell-host serving a terminal, which
# is not reaped when the app quits) trips it too — and "Beekeeper.app is
# running" then sends the reader to quit an app that is already quit.
if HOLDERS="$(pgrep -lf "/Applications/Beekeeper.app/Contents/MacOS/")"; then
  echo "something is still running out of /Applications/Beekeeper.app — removing" \
       "the bundle under it would break it, so nothing was installed:" >&2
  echo "$HOLDERS" | sed 's/^/  /' >&2
  echo "Quit the app if it is running. A sidecar left over from an app that" \
       "already quit (buzz-shell-host, buzz-session-provider) can be ended with" \
       "\`kill <pid>\`; a shell host takes its terminal session's live process" \
       "with it." >&2
  exit 1
fi
# One-time sweep of the pre-rename bundle: quit anything still running out of
# the old "Bee Keeper.app" path and remove it, so /Applications does not end
# up carrying both spellings side by side.
if pgrep -f "/Applications/Bee Keeper.app/Contents/MacOS/" >/dev/null; then
  echo "==> quitting the old Bee Keeper.app"
  pkill -f "/Applications/Bee Keeper.app/Contents/MacOS/" || true
fi
rm -rf "/Applications/Bee Keeper.app"
rm -rf "/Applications/Beekeeper.app"
ditto "$APP" "/Applications/Beekeeper.app"
echo "==> installed $REV -> /Applications/Beekeeper.app"
echo "    First launch will ask for the login keychain once — expected after every update."
