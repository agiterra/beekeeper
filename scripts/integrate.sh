#!/usr/bin/env bash
# integrate.sh — sync upstream, rebase the feature stack, rebuild `integrated`.
#
# Branch model (see docs/INTEGRATION.md):
#   main              ff-only mirror of block/buzz main
#   feature/<name>    upstreamable topic branches, rebased per sync
#   integration/glue  cross-feature patches + this tooling + CI pipelines
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
for entry in "${FEATURES[@]}"; do
  branch="${entry%%:*}"
  base="${entry#*:}"; [[ "$base" == "$entry" ]] && base="main"
  old_base=$(git merge-base "$branch" "$base")
  git checkout "$branch"
  git rebase --onto "$base" "$old_base" "$branch"
done
git checkout "$GLUE"
git rebase --onto main "$(git merge-base "$GLUE" main)" "$GLUE"

# ── 3. rebuild integrated ────────────────────────────────────────────────────
git checkout -B integrated main
for entry in "${FEATURES[@]}"; do
  git merge --no-ff --no-edit "${entry%%:*}"
done
git merge --no-ff --no-edit "$GLUE"

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
  git push --force-with-lease origin "$GLUE" integrated
  git push origin "$tag"
fi

echo "OK: integrated rebuilt at $(git rev-parse --short integrated), tagged $tag"
