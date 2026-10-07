#!/usr/bin/env bash
# Offline test for scripts/lib/env-compat.sh.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fail() { echo "FAIL: $*" >&2; exit 1; }

out="$(env -i PATH="$PATH" BUZZ_RELAY_URL=ws://legacy:1 BUZZ_VITE_PORT=4000 \
  BEEKEEPER_VITE_PORT=5000 BUZZ_SECRET_THING=s3cr3t bash -c '
    source "$1/lib/env-compat.sh"
    beekeeper_adopt_legacy_env test 2>"$2"
    printf "%s|%s|%s|%s\n" "$BEEKEEPER_RELAY_URL" "$BEEKEEPER_VITE_PORT" "$BUZZ_RELAY_URL" "$BEEKEEPER_SECRET_THING"
    bash -c "printf %s \"\$BEEKEEPER_RELAY_URL\""
  ' _ "$SCRIPT_DIR" "${TMPDIR:-/tmp}/env-compat-stderr.$$")"
stderr="$(cat "${TMPDIR:-/tmp}/env-compat-stderr.$$")"; rm -f "${TMPDIR:-/tmp}/env-compat-stderr.$$"

[[ "$(sed -n 1p <<<"$out")" == "ws://legacy:1|5000|ws://legacy:1|s3cr3t" ]] ||
  fail "legacy name not adopted, new name did not win, or legacy removed: $out"
[[ "$(sed -n 2p <<<"$out")" == "ws://legacy:1" ]] || fail "adopted name was not exported to children"
grep -q 'BUZZ_RELAY_URL' <<<"$stderr" || fail "stderr should name the adopted variable: $stderr"
grep -q 'BUZZ_VITE_PORT' <<<"$stderr" && fail "a name whose new twin was set should not be reported"
grep -q 's3cr3t\|legacy:1' <<<"$stderr" && fail "stderr printed a value"

quiet="$(env -i PATH="$PATH" bash -c 'source "$1/lib/env-compat.sh"; beekeeper_adopt_legacy_env test 2>&1' _ "$SCRIPT_DIR")"
[[ -z "$quiet" ]] || fail "no legacy names should print nothing: $quiet"

echo "PASS: env-compat adopts BUZZ_* into unset BEEKEEPER_*, new name wins, names only"
