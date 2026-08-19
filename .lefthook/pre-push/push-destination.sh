#!/usr/bin/env bash
# Tripwire: this fork's docs/ carries our deploy state, open defects, test
# session names and relay URL. Today there is no block/buzz remote, so nothing
# can leak. The day someone adds one, this stops the first push to it.
#
# Ours: the lightyear relay and github.com/agiterra. Anything else is outward.
set -euo pipefail

url="${2:-}"
[ -n "$url" ] || url="$(git remote get-url "${1:-origin}" 2>/dev/null || true)"

case "$url" in
  *lightyear.agiterra.org*|*github.com/agiterra/*|*github.com:agiterra/*|"") exit 0 ;;
esac

[ "${BUZZ_ALLOW_OUTWARD_PUSH:-}" = "1" ] && exit 0

cat >&2 <<MSG

  Push destination is not one of this fork's remotes:
    $url

  docs/ on this line is fork-local: relay deploy state, open defects with
  transcript evidence, test session names, the branch ceremony. It is not
  Buzz documentation and should not travel upstream.

  If this is deliberate, cut a topic branch off main (the clean mirror) and
  push that, or re-run with BUZZ_ALLOW_OUTWARD_PUSH=1.

MSG
exit 1
