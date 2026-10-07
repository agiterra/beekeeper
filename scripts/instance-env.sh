#!/usr/bin/env bash
# Computes the full multi-instance desktop dev environment.
# Source this file from desktop dev commands; it exports:
#   BEEKEEPER_VITE_PORT, BEEKEEPER_HMR_PORT, VITE_PORT, VITE_HMR_PORT
#   BEEKEEPER_RELAY_PORT, BEEKEEPER_RELAY_URL
#   BEEKEEPER_INSTANCE_SLUG, BEEKEEPER_WORKTREE_LABEL, VITE_DEV_BRANCH (worktrees only)
#   BEEKEEPER_TAURI_CONFIG
#   BEEKEEPER_PRIVATE_KEY (worktrees only, when BEEKEEPER_SHARE_IDENTITY=1)

WORKTREE_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

# `just` loads .env, and an older one still spells these BUZZ_*. Adopt those
# first, so a BUZZ_RELAY_URL there is honoured below rather than defaulted over.
# shellcheck source=lib/env-compat.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/env-compat.sh"
beekeeper_adopt_legacy_env instance-env

# Derive a stable base port from the worktree root so the same worktree always
# gets the same ports. This keeps the Tauri dev config stable between runs and
# preserves Cargo's build cache.
BASE_PORT=$(python3 -c "import hashlib,sys; h=int(hashlib.sha256(sys.argv[1].encode()).hexdigest(), 16); print(10000 + h % 55000)" "$WORKTREE_ROOT")
export BEEKEEPER_VITE_PORT=$BASE_PORT
export BEEKEEPER_HMR_PORT=$((BASE_PORT + 1))
export BEEKEEPER_RELAY_PORT=3000
export VITE_PORT="$BEEKEEPER_VITE_PORT"
export VITE_HMR_PORT="$BEEKEEPER_HMR_PORT"
export BEEKEEPER_RELAY_URL="${BEEKEEPER_RELAY_URL:-ws://localhost:3000}"

DEV_URL="http://localhost:${BEEKEEPER_VITE_PORT}"
if [[ "${BEEKEEPER_RESET_WEBVIEW_STATE:-0}" == "1" ]]; then
    DEV_URL="${DEV_URL}?resetDevState=1"
fi

BEEKEEPER_TAURI_CONFIG="{\"build\":{\"devUrl\":\"${DEV_URL}\",\"beforeDevCommand\":\"exec ./node_modules/.bin/vite --port ${BEEKEEPER_VITE_PORT} --strictPort\"},\"identifier\":\"io.agiterra.beekeeper.app.dev\",\"productName\":\"Beekeeper Dev\"}"
unset VITE_DEV_BRANCH

# Generate a badged variant of the app icon labelled $1 into
# target/dev-icons/icon.icns (gitignored). Sets DEV_ICON on success; returns
# non-zero (leaving the stock config in place) when the generator is
# unavailable or fails (e.g. Linux — no swift).
generate_badged_icon() {
    ICON_DIR="$WORKTREE_ROOT/desktop/src-tauri/target/dev-icons"
    mkdir -p "$ICON_DIR"
    DEV_ICON="$ICON_DIR/icon.icns"
    swift "$WORKTREE_ROOT/scripts/generate-dev-icon.swift" \
        "$WORKTREE_ROOT/desktop/src-tauri/icons/icon.icns" "$DEV_ICON" "$1"
}

# In worktrees, extract a label from the branch name and derive a unique app
# identity and icon so multiple local desktop instances can run side by side.
# The main checkout keeps the plain dev identity but still gets a "dev"-badged
# icon so it is distinguishable from an installed production Beekeeper.app.
#
# Worktree detection: compare --git-dir to --git-common-dir. In the main
# working tree these are identical; in any worktree (whether under .worktrees/,
# .claude/worktrees/, or elsewhere on disk) they differ.
#
# Both MUST be resolved to physical absolute paths before comparing. Git
# reports them relative to the current directory, and not in the same form:
# from the repo root both read `.git`, but from a subdirectory --git-dir comes
# back absolute while --git-common-dir stays relative (`../.git`). Every
# desktop recipe sources this file after `cd desktop`, so a raw string compare
# called the main checkout a worktree and handed it a branch-derived identity —
# which is where the stray `io.agiterra.beekeeper.app.dev.main` app-data
# directory came from.
if git rev-parse --is-inside-work-tree &>/dev/null; then
    GIT_DIR=$(git rev-parse --absolute-git-dir 2>/dev/null)
    GIT_DIR=$(cd "$GIT_DIR" 2>/dev/null && pwd -P || true)
    GIT_COMMON_DIR=$(git rev-parse --git-common-dir 2>/dev/null)
    GIT_COMMON_DIR=$(cd "$GIT_COMMON_DIR" 2>/dev/null && pwd -P || true)
    if [[ -n "$GIT_DIR" && -n "$GIT_COMMON_DIR" && "$GIT_DIR" != "$GIT_COMMON_DIR" ]]; then
        BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD)
        export BEEKEEPER_WORKTREE_LABEL="${BRANCH_NAME##*/}"
        export BEEKEEPER_INSTANCE_SLUG=$(echo "$BRANCH_NAME" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/--*/-/g' | sed 's/^-//' | sed 's/-$//')

        # BEEKEEPER_SHARE_IDENTITY=1: reuse the main dev checkout's Nostr key so
        # worktrees skip onboarding and share the same identity. The per-worktree
        # identifier is kept so concurrent instances don't collide on
        # tauri-plugin-single-instance or the app data directory.
        if [[ "${BEEKEEPER_SHARE_IDENTITY:-0}" == "1" ]]; then
            KEYRING_SERVICE="beekeeper-desktop-dev"
            KEYRING_BLOB=""
            case "$(uname -s)" in
                Darwin)
                    if command -v security &>/dev/null; then
                        KEYRING_BLOB="$(security find-generic-password -s "$KEYRING_SERVICE" -a secrets -w 2>/dev/null || true)"
                    fi
                    ;;
                Linux)
                    if command -v secret-tool &>/dev/null; then
                        KEYRING_BLOB="$(secret-tool lookup service "$KEYRING_SERVICE" username secrets target default 2>/dev/null || true)"
                    fi
                    ;;
            esac

            KEYRING_IDENTITY="$(printf '%s' "$KEYRING_BLOB" | python3 -c 'import json, sys; value = json.load(sys.stdin).get("identity", ""); print(value if isinstance(value, str) else "")' 2>/dev/null || true)"
            CANONICAL_KEY="$HOME/Library/Application Support/io.agiterra.beekeeper.app.dev/identity.key"
            LEGACY_CANONICAL_KEY="$HOME/Library/Application Support/xyz.block.sprout.app.dev/identity.key"

            SHARED_IDENTITY="$KEYRING_IDENTITY"
            if [[ -z "$SHARED_IDENTITY" && -f "$CANONICAL_KEY" ]]; then
                SHARED_IDENTITY="$(cat "$CANONICAL_KEY")"
            elif [[ -z "$SHARED_IDENTITY" && -f "$LEGACY_CANONICAL_KEY" ]]; then
                SHARED_IDENTITY="$(cat "$LEGACY_CANONICAL_KEY")"
            fi

            if [[ -n "$SHARED_IDENTITY" ]]; then
                export BEEKEEPER_PRIVATE_KEY="$SHARED_IDENTITY"
            else
                echo "⚠ BEEKEEPER_SHARE_IDENTITY=1 but no identity found in keyring service $KEYRING_SERVICE, at $CANONICAL_KEY, or at $LEGACY_CANONICAL_KEY — run Beekeeper from repo root first" >&2
            fi
        fi

        if generate_badged_icon "$BEEKEEPER_WORKTREE_LABEL"; then
            echo "🌳 Worktree: ${BEEKEEPER_WORKTREE_LABEL}"
            export VITE_DEV_BRANCH="$BEEKEEPER_WORKTREE_LABEL"
            BEEKEEPER_TAURI_CONFIG="{\"build\":{\"devUrl\":\"${DEV_URL}\",\"beforeDevCommand\":\"exec ./node_modules/.bin/vite --port ${BEEKEEPER_VITE_PORT} --strictPort\"},\"identifier\":\"io.agiterra.beekeeper.app.dev.${BEEKEEPER_INSTANCE_SLUG}\",\"productName\":\"Beekeeper (${BEEKEEPER_WORKTREE_LABEL})\",\"bundle\":{\"icon\":[\"$DEV_ICON\"]}}"
        fi
    elif generate_badged_icon "dev"; then
        BEEKEEPER_TAURI_CONFIG="{\"build\":{\"devUrl\":\"${DEV_URL}\",\"beforeDevCommand\":\"exec ./node_modules/.bin/vite --port ${BEEKEEPER_VITE_PORT} --strictPort\"},\"identifier\":\"io.agiterra.beekeeper.app.dev\",\"productName\":\"Beekeeper Dev\",\"bundle\":{\"icon\":[\"$DEV_ICON\"]}}"
    fi
fi

export BEEKEEPER_TAURI_CONFIG
