#!/usr/bin/env bash
# app-from.sh — build and install the founder's daily-driver Beekeeper Dev.app
# from a *named landed commit*, so "what am I running" has one answer.
#
# Why this exists (PLAN-2026-09-05 §1): the daily driver has been `tauri dev`
# in the hot checkout plus eight sidecars refreshed by hand. That differs from
# the bundle Andy runs in ways that already produced findings — packs resolve
# under `target/debug` in dev and under the bundle when installed (finding 84),
# a stale `bee` on PATH, and the rule that forbids rebasing the main checkout
# while the app runs. A bundle built from a named sha removes all three, and it
# is signed once instead of once per rebuild, so the login-keychain prompt stops
# parking an unattended refresh with no provider.
#
# Debug profile, deliberately. `tauri build --debug` changes almost nothing that
# matters here — measured 2026-09-05: the keyring service name, the updater
# plugin, and profiling timers. Keeping the debug profile keeps the keyring
# service `beekeeper-desktop-dev` (`desktop/src-tauri/src/app_state_keyring.rs`),
# which is where the founder's existing dev identity already lives, so the
# bundle inherits that identity with no migration. Paired with the dev
# identifier/productName below (`io.agiterra.beekeeper.app.dev` / "Beekeeper
# Dev", the same pair `scripts/instance-env.sh` hands `tauri dev` in the main
# checkout) the bundle also inherits the same Application Support directory.
# A release-profile bundle would be a *different* app with a different identity;
# `scripts/local-prod-build.sh` is that build, and stays separate.
#
# Refuses any commit that is not an ancestor of `origin/main`: the point is to
# run something that landed, not something that might.
#
# Usage: scripts/app-from.sh <commit-ish> [--no-install] [--fresh]
#   <commit-ish>  required; must be an ancestor of origin/main after a fetch
#   --no-install  build and verify only — skip the install and the relaunch
#   --fresh       delete the build worktree first (a cold build, ~40 min here)
#
# The build worktree is reused between runs (`<main worktree>-app-from`, or
# BEEKEEPER_APP_FROM_WORKTREE) so a second build is incremental rather than another
# cold hour. It is detached and never the invoking checkout: this script must
# never touch a tree with an app running out of it.
set -euo pipefail

SIDECAR_PACKAGES=(
  beekeeper-acp beekeeper-agent beekeeper-backend-kubernetes beekeeper-dev-mcp
  beekeeper-cli git-credential-nostr beekeeper-shell-host beekeeper-host beekeeper-session-provider
)
# The binaries those packages produce. Every `beekeeper-*` package builds the
# binary of the same name, except `beekeeper-cli`, which builds `bee`. This is the
# set `desktop/scripts/check-sidecar-parity.mjs` compares against
# `tauri.conf.json`'s `externalBin` plus the provider; a name added in one
# place and not the other fails that check rather than a morning.
SIDECAR_BINARIES=(
  beekeeper-acp beekeeper-agent beekeeper-backend-kubernetes beekeeper-dev-mcp
  bee git-credential-nostr beekeeper-shell-host beekeeper-host beekeeper-session-provider
)

APP_NAME="Beekeeper Dev"
APP_IDENTIFIER="io.agiterra.beekeeper.app.dev"
INSTALL_DIR="$HOME/Applications"

SRC_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Absolute path of a checkout's main worktree; git lists it first.
main_worktree_of() {
  git -C "$1" worktree list --porcelain 2>/dev/null |
    awk '/^worktree /{ print substr($0, 10); exit }'
}

REV=""
NO_INSTALL=false
FRESH=false
for arg in "$@"; do
  case "$arg" in
    --no-install) NO_INSTALL=true ;;
    --fresh) FRESH=true ;;
    -*)
      echo "unknown flag: $arg" >&2
      exit 1
      ;;
    *)
      [[ -z "$REV" ]] || {
        echo "one commit-ish only (got '$REV' and '$arg')" >&2
        exit 1
      }
      REV="$arg"
      ;;
  esac
done
[[ -n "$REV" ]] || {
  echo "usage: scripts/app-from.sh <commit-ish> [--no-install] [--fresh]" >&2
  exit 1
}

SRC_MAIN="$(main_worktree_of "$SRC_ROOT")"
[[ -n "$SRC_MAIN" ]] || {
  echo "not a git checkout: $SRC_ROOT" >&2
  exit 1
}
BUILD_ROOT="${BEEKEEPER_APP_FROM_WORKTREE:-${SRC_MAIN}-app-from}"

# ── the commit must have landed ──────────────────────────────────────────────
# Fetch first: an ancestry test against a stale remote-tracking ref answers
# about yesterday's main. `origin` is named through the configured remote for
# the current branch where possible, because the remote names moved once
# already (2026-08-24) and hard-coding one is how two pre-push guards broke.
REMOTE="$(git -C "$SRC_ROOT" config --get branch."$(git -C "$SRC_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)".remote || true)"
REMOTE="${REMOTE:-origin}"
echo "==> fetching $REMOTE"
# Never block on a credential prompt: `origin` is the relay, which authenticates
# git with NIP-98 through `git-credential-nostr`. Without the helper installed
# a plain fetch waits forever on a username that cannot be typed here.
export GIT_TERMINAL_PROMPT=0
git -C "$SRC_ROOT" fetch --quiet "$REMOTE" || {
  echo "fetch from '$REMOTE' failed — refusing to test ancestry against a stale ref." >&2
  echo "If this is the relay remote, run 'just install-git-credentials' first." >&2
  exit 1
}

SHA="$(git -C "$SRC_ROOT" rev-parse -q --verify "${REV}^{commit}" 2>/dev/null || true)"
[[ -n "$SHA" ]] || {
  echo "not a commit: $REV" >&2
  exit 1
}
MAIN_REF="refs/remotes/${REMOTE}/main"
git -C "$SRC_ROOT" rev-parse -q --verify "$MAIN_REF" >/dev/null || {
  echo "no $MAIN_REF after fetching '$REMOTE' — cannot say what has landed." >&2
  exit 1
}
if ! git -C "$SRC_ROOT" merge-base --is-ancestor "$SHA" "$MAIN_REF"; then
  echo "refusing: $(git -C "$SRC_ROOT" rev-parse --short "$SHA") is not an ancestor of $MAIN_REF." >&2
  echo "This recipe installs builds of commits that have landed. Land it first," >&2
  echo "or use 'just desktop-standalone' for a branch you are still iterating on." >&2
  exit 1
fi
SHORT="$(git -C "$SRC_ROOT" rev-parse --short=9 "$SHA")"
COMMIT_COUNT="$(git -C "$SRC_ROOT" rev-list --count "$SHA")"
echo "==> building $APP_NAME from $REV ($SHORT), ancestor of $MAIN_REF"

# ── build worktree, detached at that commit ──────────────────────────────────
if $FRESH && [[ -e "$BUILD_ROOT" ]]; then
  echo "==> --fresh: removing $BUILD_ROOT"
  git -C "$SRC_ROOT" worktree remove --force "$BUILD_ROOT" 2>/dev/null || rm -rf "$BUILD_ROOT"
  git -C "$SRC_ROOT" worktree prune
fi
if [[ -e "$BUILD_ROOT" ]]; then
  BUILD_MAIN="$(main_worktree_of "$BUILD_ROOT")"
  if [[ "$BUILD_MAIN" != "$SRC_MAIN" ]]; then
    echo "build worktree belongs to another clone (or is not a git worktree):" >&2
    echo "  build tree: $BUILD_ROOT -> ${BUILD_MAIN:-<not a git worktree>}" >&2
    echo "  this clone: $SRC_MAIN" >&2
    exit 1
  fi
fi
# Never build in a checkout someone is working in. The founder's app runs out
# of the main worktree and `tauri dev` rebuilds on file change, so a checkout
# there would rebuild their running app at another commit.
for guarded in "$SRC_MAIN" "$SRC_ROOT"; do
  if [[ "$(cd "$guarded" && pwd -P)" == "$(cd "$BUILD_ROOT" 2>/dev/null && pwd -P || echo /nonexistent)" ]]; then
    echo "refusing: the build worktree is $guarded, a live checkout." >&2
    exit 1
  fi
done

# Seed the build tree's heavy build state from the source checkout, as the
# project's own sandbox.yml declares it. Non-fatal by construction: an unseeded
# tree is cold, which is slow, not wrong — so a failure here reports and the
# build carries on from scratch.
seed_build_tree() {
  local tree="$1" source="$2"
  local bee="${BEEKEEPER_BEE:-$(command -v bee 2>/dev/null || true)}"
  for candidate in \
      "$source/target/release/bee" \
      "$source/target/debug/bee" \
      "/Applications/Beekeeper.app/Contents/MacOS/bee"; do
    [[ -n "$bee" && -x "$bee" ]] && break
    bee="$candidate"
  done
  if [[ -z "$bee" || ! -x "$bee" ]]; then
    echo "note: no 'bee' to seed $tree from $source; this build starts cold" >&2
    return 0
  fi
  if ! "$bee" sandbox seed --tree "$tree" --from "$source" --run-recipes --confirm; then
    echo "warning: seeding $tree did not finish; this build starts cold" >&2
  fi
}

if [[ ! -d "$BUILD_ROOT" ]]; then
  git -C "$SRC_ROOT" worktree add --detach "$BUILD_ROOT" "$SHA"
else
  git -C "$BUILD_ROOT" checkout --detach --quiet "$SHA"
fi

seed_build_tree "$BUILD_ROOT" "$SRC_MAIN"

cd "$BUILD_ROOT"
export PATH="$BUILD_ROOT/bin:$PATH"
# The commit is known here and the build scripts should not have to re-derive
# it. `BEEKEEPER_SOURCE_SHA`/`BEEKEEPER_SOURCE_COMMIT_COUNT` are the pair every build
# script in this repo already reads together or not at all
# (`crates/beekeeper-relay/build.rs`, `crates/beekeeper-core/build.rs`).
export BEEKEEPER_SOURCE_SHA="$SHA"
export BEEKEEPER_SOURCE_COMMIT_COUNT="$COMMIT_COUNT"

TARGET="$(rustc -vV | sed -n 's|host: ||p')"

# Debug profile, optimized. The debug profile is kept for its keyring service
# and identity (above), not for its codegen: unoptimized, every installed
# binary — desktop, provider, host — ran several times slower than it needs to,
# and the desktop's periodic relay re-queries showed up as multi-second
# CPU bursts (2026-10-02, ledger 310). `opt-level` leaves `debug_assertions`
# on, so `cfg(debug_assertions)` code paths, the keyring name among them, are
# unchanged. Exported once so both the sidecar build and `tauri build --debug`
# below see it; developers' own `cargo` builds are untouched.
export CARGO_PROFILE_DEV_OPT_LEVEL=2

# ── sidecars (debug profile, same eight the bundle declares) ─────────────────
echo "==> building sidecars"
CARGO_PACKAGE_ARGS=()
for pkg in "${SIDECAR_PACKAGES[@]}"; do
  CARGO_PACKAGE_ARGS+=(-p "$pkg")
done
cargo build "${CARGO_PACKAGE_ARGS[@]}"
mkdir -p desktop/src-tauri/binaries
for bin in "${SIDECAR_BINARIES[@]}"; do
  cp "target/debug/${bin}" "desktop/src-tauri/binaries/${bin}-${TARGET}"
  chmod 755 "desktop/src-tauri/binaries/${bin}-${TARGET}"
done

# ── bundle ──────────────────────────────────────────────────────────────────
# externalBin carries the provider, which the tracked delta config already
# spells out; identifier and productName pin the bundle to the dev identity.
BUNDLE_CONFIG="$(node -e '
  const fs = require("node:fs");
  const delta = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  process.stdout.write(JSON.stringify({
    bundle: { externalBin: delta.bundle.externalBin },
    identifier: process.argv[2],
    productName: process.argv[3],
  }));
' desktop/src-tauri/tauri.local-prod.conf.json "$APP_IDENTIFIER" "$APP_NAME")"

pnpm install
# Staged before bundling: `bundle.macOS.files` copies it from
# `desktop/src-tauri/loginitems/` during the bundle, so it lands inside the
# app (and, in the release pipeline, inside the DMG) rather than being copied
# in afterwards where a signature would not cover it.
"$BUILD_ROOT/scripts/stage-menubar.sh" --debug

(cd desktop && pnpm tauri build --debug --bundles app --config "$BUNDLE_CONFIG")

APP="$BUILD_ROOT/desktop/src-tauri/target/debug/bundle/macos/${APP_NAME}.app"
[[ -d "$APP" ]] || {
  echo "bundle missing: $APP" >&2
  exit 1
}
for bin in "${SIDECAR_BINARIES[@]}"; do
  [[ -x "$APP/Contents/MacOS/${bin}" ]] || {
    echo "sidecar missing from bundle: $bin" >&2
    exit 1
  }
done
[[ -d "$APP/Contents/Resources/personas/roles" ]] || {
  echo "role packs missing from bundle (resources)" >&2
  exit 1
}
# An empty LoginItems/ passes every signature check there is: `--deep` walks
# nested code but does not notice its absence. The nesting itself is declared
# in the bundle config (`bundle.macOS.files`) and happens during bundling; this
# asserts it landed.
[[ -d "$APP/Contents/Library/LoginItems/Beekeeper Menu Bar.app" ]] || {
  echo "the menu bar app is missing from LoginItems" >&2
  exit 1
}
# Tauri leaves the bundle unsigned (per-binary linker ad-hoc signatures, no
# CodeResources seal). Sign the whole bundle once so it verifies as a unit.
# With the local signing identity (`just dev-signing-identity`), every build
# keeps the same designated requirement, so the Keychain's "Always Allow"
# survives the install. Ad-hoc changes with every build and the Keychain asks
# again (ledger 368). So the script says which one it used.
SIGN_IDENTITY="${BEEKEEPER_DEV_SIGNING_IDENTITY:-Beekeeper Dev Local Signing}"
if security find-identity -p codesigning | grep -qF "\"$SIGN_IDENTITY\""; then
  echo "==> signing with \"$SIGN_IDENTITY\""
  codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
else
  echo "==> no \"$SIGN_IDENTITY\" identity: signing ad-hoc, so the Keychain will ask for the password again after install (create it with: just dev-signing-identity)" >&2
  codesign --force --deep --sign - "$APP"
fi
codesign --verify --deep --strict "$APP" || {
  echo "bundle signature verification failed" >&2
  exit 1
}
# The nested bundle on its own, because `--deep` above reports the outer app's
# verdict and a nested signature broken by the copy is the interesting case.
codesign --verify --strict "$APP/Contents/Library/LoginItems/Beekeeper Menu Bar.app" || {
  echo "the nested menu bar app's signature does not verify" >&2
  exit 1
}
echo "==> bundle OK: $APP"

# What was built, asked of the artifact rather than of this script wherever the
# artifact can answer. The clock line is when this run *finished*, which is not
# the same fact as the binary's own build time — `bee --version` carries that.
stamp() {
  echo "    commit    $SHA"
  echo "    count     $COMMIT_COUNT"
  echo "    bee       $("$APP/Contents/MacOS/bee" --version 2>/dev/null | tr '\n' ' ' || echo 'unavailable')"
  echo "    finished  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
}

if $NO_INSTALL; then
  echo "==> --no-install: skipping the install and the relaunch"
  stamp
  exit 0
fi

# ── install ─────────────────────────────────────────────────────────────────
# Match the bundle's own executable by path, read from its Info.plist: Tauri
# leaves CFBundleExecutable as the cargo binary name, so a productName-based
# pattern silently passes over a running app (the bug local-prod-build.sh
# documents). Not the whole MacOS directory: the agent host and the provider
# it supervises run from there too and are meant to outlive the app's quit and
# update, so matching them made every install refuse (ledger 302(e)). They are
# restarted onto the new bundle after it is in place, below.
DEST="$INSTALL_DIR/${APP_NAME}.app"
mkdir -p "$INSTALL_DIR"
APP_EXE="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${DEST}/Contents/Info.plist" 2>/dev/null || echo beekeeper-desktop)"
APP_PATTERN="${DEST}/Contents/MacOS/${APP_EXE}"
if pgrep -f "$APP_PATTERN" >/dev/null; then
  echo "==> quitting the running ${APP_NAME}"
  osascript -e "quit app \"${APP_NAME}\"" 2>/dev/null || pkill -f "$APP_PATTERN" || true
  for _ in $(seq 1 20); do
    pgrep -f "$APP_PATTERN" >/dev/null || break
    sleep 0.5
  done
  if pgrep -f "$APP_PATTERN" >/dev/null; then
    echo "${APP_NAME} did not quit — quit it, then re-run" >&2
    exit 1
  fi
fi
# Keep exactly one previous bundle, so a bad build is one `mv` from undone.
rm -rf "${DEST}.prev"
if [[ -d "$DEST" ]]; then
  mv "$DEST" "${DEST}.prev"
fi
ditto "$APP" "$DEST"
echo "==> installed $SHORT -> $DEST (previous kept as ${DEST}.prev)"
# `kickstart` returning 0 means launchd accepted the request, not that the
# process is running. A kickstart issued the moment `ditto` finishes can be
# killed by AMFI as a Launch Constraint Violation (OS_REASON_CODESIGNING) while
# the new bundle is still being assessed; launchd then gives up on the service
# and the host stays dead until someone kickstarts it by hand (2026-10-08: the
# host and menu bar were down from 02:47 until 07:19, with "restarted" printed).
# So wait for a pid that survives a few seconds, retry with a pause, and say so
# when it never comes up.
restart_login_item() {
  local label="$1" target="gui/$(id -u)/$1" attempt pid
  for attempt in 1 2 3 4; do
    if ! launchctl kickstart -k "$target" 2>/dev/null; then
      echo "==> ${label} is registered but not loaded; left as it is" >&2
      return 0
    fi
    sleep 3
    pid="$(launchctl print "$target" 2>/dev/null | awk '$1 == "pid" { print $3; exit }')"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      echo "==> restarted ${label} on the new bundle (pid ${pid})"
      return 0
    fi
    echo "==> ${label} did not stay up after restart attempt ${attempt}; retrying" >&2
    sleep $((attempt * 2))
  done
  echo "==> ${label} is NOT running: launchd reports $(launchctl print "$target" 2>/dev/null | awk -F' = ' '/last exit reason/ { print $2; exit }'). Run: launchctl kickstart -k ${target}" >&2
  return 0
}
# Restart this user's login items that run from the bundle just replaced —
# the agent host and the menu bar app — so they, and the provider the host
# supervises, run the new binaries rather than the moved-aside ones. The host
# restores its sessions on start. A system LaunchDaemon (`--system`) needs
# root and is left alone, said out loud.
for plist in "$HOME"/Library/LaunchAgents/io.agiterra.beekeeper.*.plist; do
  [[ -f "$plist" ]] || continue
  program="$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null || true)"
  [[ "$program" == "${DEST}/"* ]] || continue
  label="$(basename "$plist" .plist)"
  restart_login_item "$label"
done
for plist in /Library/LaunchDaemons/io.agiterra.beekeeper.*.plist; do
  [[ -f "$plist" ]] || continue
  program="$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null || true)"
  [[ "$program" == "${DEST}/"* ]] || continue
  echo "==> $(basename "$plist" .plist) runs from this bundle as a system daemon; restart it with sudo launchctl kickstart -k system/$(basename "$plist" .plist)" >&2
done
open -a "$DEST"
stamp
