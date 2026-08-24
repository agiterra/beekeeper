#!/usr/bin/env bash
# Pre-push guard: CI checks the PR merged with main, so local runs on a
# skewed branch can pass while CI fails. Block the push only when
# main's tracking ref has changed files this branch also touches.
#
# Two things here are load-bearing, both learned on 2026-08-24 when `origin`
# was repointed at the relay's own git hosting and GitHub became `upstream`:
#
#   - The remote is resolved from what `main` tracks, not hard-coded to
#     `origin`. A remote name is a local nickname; what main tracks is the
#     fact this guard actually needs.
#   - Every git command that can reach the network runs with
#     GIT_TERMINAL_PROMPT=0. The relay's git needs Nostr credentials, so a
#     plain `git fetch origin` sat waiting for a username that could never
#     arrive — and because this runs in a pre-push hook, it hung the push for
#     17 minutes with no output at all. `|| true` did not help: it catches a
#     fetch that *fails*, not one that never returns. A guard that can block
#     forever is worse than one that is occasionally skipped.
set -euo pipefail

# Never prompt. A hook that waits on a terminal has no terminal to wait on.
export GIT_TERMINAL_PROMPT=0
export GIT_ASKPASS=/usr/bin/true
export SSH_ASKPASS=/usr/bin/true

branch=$(git rev-parse --abbrev-ref HEAD)
if [ "$branch" = "main" ] || [ "$branch" = "HEAD" ]; then
  exit 0
fi

# Candidates in preference order: what main tracks, then the conventional
# names. Matches scripts/check-file-sizes-core.mjs, which resolves the
# file-size ratchet's base the same way and for the same reason.
main_ref=""
for cand in \
  "$(git rev-parse --abbrev-ref main@{upstream} 2>/dev/null || true)" \
  origin/main \
  upstream/main
do
  [ -n "$cand" ] || continue
  remote=${cand%%/*}
  git remote get-url "$remote" >/dev/null 2>&1 || continue
  # Refresh best-effort; an unreachable or auth-gated remote must not block.
  git fetch --quiet "$remote" main >/dev/null 2>&1 || true
  if git rev-parse --verify --quiet "$cand" >/dev/null; then
    main_ref=$cand
    break
  fi
done

# No usable ref: skip rather than block. This guard is an early warning, not
# an authority — CI still re-checks the merge.
[ -n "$main_ref" ] || exit 0

base=$(git merge-base HEAD "$main_ref")
if [ "$base" = "$(git rev-parse "$main_ref")" ]; then
  exit 0
fi

overlap=$(comm -12 \
  <(git diff --name-only "$base" "$main_ref" -- | sort) \
  <(git diff --name-only "$base" HEAD -- | sort))

if [ -z "$overlap" ]; then
  exit 0
fi

{
  echo "Branch is behind $main_ref, and it changed files this branch also touches:"
  echo "$overlap" | sed 's/^/  /'
  echo "Local checks ran on a tree CI will never test. Run 'git merge $main_ref',"
  echo "resolve, re-run checks, then push."
} >&2
exit 1
