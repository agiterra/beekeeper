#!/usr/bin/env bash
# Run the pre-push floor BEFORE git opens the connection to the relay, then
# push. Fixes ledger 178(n): `git push` mints its NIP-98 credential at ref
# discovery — before any pre-push hook runs — and reuses that one credential
# for the rest of the push (confirmed empirically against a throwaway HTTP
# git server: the credential helper's `get` fires exactly once per `git
# push`, and the same Authorization value is replayed on the retried GET and
# the receive-pack POST). The relay's token window is +-900s. A crate-
# touching floor can run ~20 minutes, so by the time the hook finishes and
# git uploads the pack, the credential minted at the start is long expired —
# `HTTP 401` with every check green (landing 184, 2026-09-20, see
# plans/archive/2026-09-20-pre-push-floor-stamp.md).
#
# This script runs the exact same floor (`scripts/pre-push-floor.sh`) against
# the outgoing range for the branch being pushed, and on success records a
# short-lived pass stamp (scripts/pre-push-floor-stamp.mjs) naming the tip sha
# and the exact changed-file set. `git push` then runs as normal: its
# pre-push hook (`lefthook.yml`'s `floor` command) finds a fresh, matching
# stamp and returns immediately, so the credential git minted moments earlier
# is still seconds old when the pack uploads. A push without this wrapper —
# `git push` directly — still runs the floor in full inside the hook, exactly
# as before, and may still 401 on a long floor.
#
# Usage: scripts/push-with-floor.sh [git push args...]
#   scripts/push-with-floor.sh origin main
#   scripts/push-with-floor.sh origin work/my-branch:main
# With no args, defaults to `origin` and the current branch, matching plain
# `git push` on a branch with an upstream configured.
set -uo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root" || exit 1

echo "push-with-floor: running the floor before opening the connection..." >&2
BUZZ_PRE_PUSH_FLOOR_STAMP_WRITE=1 ./scripts/pre-push-floor.sh
floor_status=$?

if [ "$floor_status" != "0" ]; then
  echo "push-with-floor: floor failed — not pushing. Fix it, or push with --no-verify only on a SHA \`just ci\` already passed (docs/INTEGRATION.md § Landing a batch)." >&2
  exit "$floor_status"
fi

echo "push-with-floor: floor passed; pushing now while the stamp is fresh" >&2
exec git push "$@"
