#!/usr/bin/env bash
# Stamp the assignment this checkout was configured for onto the commit — and
# invent nothing when there is no assignment.
#
# `$1` is the commit-message file. With no `buzz.assignmentId`, or one that is
# not a lowercase 64-hex event id, this exits 0 and writes no trailer: a commit
# nobody assigned must not carry a made-up assignment.
#
# `set -e` is deliberately absent, for the same reason as in
# `wip-post-commit.sh`: a hook that fails a commit is worse than no hook.
#
# These exact bytes are also compiled into
# `beekeeper_core::seat_git_hooks::WIP_PREPARE_COMMIT_MSG_HOOK`.
set -uo pipefail

msg_file="${1:-}"
[ -n "$msg_file" ] || exit 0
[ -f "$msg_file" ] || exit 0

assignment="$(git config --get buzz.assignmentId 2>/dev/null || true)"
printf '%s' "$assignment" | grep -Eq '^[0-9a-f]{64}$' || exit 0

# Already stamped — by a rebase, an amend, or the author's own hand.
if grep -Eq '^Assignment:[[:space:]]' "$msg_file"; then
  exit 0
fi

git interpret-trailers \
  --if-exists doNothing \
  --trailer "Assignment: $assignment" \
  --in-place "$msg_file" || true

exit 0
