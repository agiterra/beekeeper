#!/usr/bin/env bash
# integrate.sh — sync upstream, rebase the feature stack, rebuild `integrated`.
#
# Branch model (see docs/INTEGRATION.md):
#   main              ff-only mirror of block/buzz main
#   feature/<name>    upstreamable topic branches, rebased per sync
#   integration/glue  cross-feature patches + this tooling + CI pipelines;
#                     a patch series REBASED ONTO the feature assembly each
#                     build (force-pushed; base recorded in integration/glue-base)
#   integrated        REBUILT product branch (force-pushed; pin via build/* tags)
#
# Usage: scripts/integrate.sh [--no-push] [--skip-gate] [--no-sync]
set -euo pipefail

# Feature stack in merge order. Stacked branches list their base after ':'
# (a branch rebases onto its base; base defaults to main).
FEATURES=(
  "fix/git-sign-oa-pubkey-validation"
  "feature/project-containers"
  "feature/project-access:feature/project-containers"
  "feature/coding-sessions"
)
GLUE="integration/glue"

NO_PUSH=false SKIP_GATE=false NO_SYNC=false
for arg in "$@"; do
  case "$arg" in
    --no-push) NO_PUSH=true ;;
    --skip-gate) SKIP_GATE=true ;;
    --no-sync) NO_SYNC=true ;;
    *) echo "unknown flag: $arg" >&2; exit 1 ;;
  esac
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
export PATH="$ROOT/bin:$PATH"

[[ -z "$(git status --porcelain)" ]] || { echo "working tree not clean" >&2; exit 1; }
git config rerere.enabled true
git config rerere.autoUpdate true

# ── 1. sync main (ff-only mirror of upstream) ────────────────────────────────
if ! $NO_SYNC; then
  git fetch upstream
  git checkout main
  git merge --ff-only upstream/main
  $NO_PUSH || git push origin main
fi

# ── 2. rebase the feature stack ──────────────────────────────────────────────
# rerere replays previously-resolved conflicts; a genuinely new conflict stops
# the script for human resolution (rerun after `git rebase --continue`).
# A branch already based on its target is skipped outright; a branch checked
# out in another linked worktree cannot be checked out here, so its rebase
# runs inside that worktree (requires it to be clean).
for entry in "${FEATURES[@]}"; do
  branch="${entry%%:*}"
  base="${entry#*:}"; [[ "$base" == "$entry" ]] && base="main"
  old_base=$(git merge-base "$branch" "$base")
  if [[ "$old_base" == "$(git rev-parse "$base")" ]]; then
    echo "== $branch already based on $base — skipping rebase"
    continue
  fi
  holder="$(git worktree list --porcelain \
    | awk -v b="refs/heads/$branch" '$1=="worktree"{w=$2} $1=="branch"&&$2==b{print w}')"
  if [[ -n "$holder" && "$holder" != "$ROOT" ]]; then
    [[ -z "$(git -C "$holder" status --porcelain)" ]] \
      || { echo "worktree holding $branch is dirty: $holder" >&2; exit 1; }
    git -C "$holder" rebase --onto "$base" "$old_base" "$branch"
  else
    git checkout "$branch"
    git rebase --onto "$base" "$old_base" "$branch"
  fi
done

# ── 3. rebuild integrated ────────────────────────────────────────────────────
# The assembly is main + feature merges (rerere replays the recorded
# cross-feature union resolutions). Glue is a PATCH SERIES REBASED ONTO THE
# ASSEMBLY — not merged — so glue commits may edit files that only exist on
# feature branches (cross-feature adaptation). `integration/glue-base` records
# the assembly commit the series is currently parented on.
OLD_GLUE_BASE="$(git rev-parse integration/glue-base)"
git checkout -B integrated-build main
for entry in "${FEATURES[@]}"; do
  git merge --no-ff --no-edit "${entry%%:*}"
done
ASSEMBLY="$(git rev-parse HEAD)"
git rebase --onto "$ASSEMBLY" "$OLD_GLUE_BASE" "$GLUE"
git update-ref refs/heads/integration/glue-base "$ASSEMBLY"

# ── 3b. stamp the CI base ref on the glue branch ─────────────────────────────
# The gate's file-size ratchet diffs against this commit (the CI clone has no
# origin/main ref and no credentials to fetch it).
mkdir -p .ci
if [[ "$(cat .ci/base-ref 2>/dev/null)" != "$(git rev-parse main)" ]]; then
  git rev-parse main > .ci/base-ref
  git add .ci/base-ref
  git commit -s -m "chore(integration): stamp CI base ref $(git rev-parse --short main)"
fi

# integrated = the rebased glue tip (linear on top of the assembly).
git checkout -B integrated-build "$GLUE" --

# ── 4. gate ──────────────────────────────────────────────────────────────────
if ! $SKIP_GATE; then
  cargo test --workspace
  just desktop-check
  just desktop-test
  (cd desktop && pnpm typecheck)
fi

# ── 5. tag + push ────────────────────────────────────────────────────────────
tag="build/$(date +%Y-%m-%d)"
n=1; while git rev-parse -q --verify "refs/tags/$tag" >/dev/null; do
  tag="build/$(date +%Y-%m-%d).$((++n))"
done
git tag -a "$tag" -m "integrated build: main=$(git rev-parse --short main) + ${#FEATURES[@]} features + glue"

if ! $NO_PUSH; then
  for entry in "${FEATURES[@]}"; do
    git push --force-with-lease origin "${entry%%:*}"
  done
  git push --force-with-lease origin "$GLUE" integration/glue-base integrated-build:integrated
  git push origin "$tag"
fi

echo "OK: integrated rebuilt at $(git rev-parse --short integrated-build), tagged $tag"
