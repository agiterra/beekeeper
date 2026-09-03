#!/bin/sh
# Velocity, from the harness's own measurement rather than anybody's claim.
set -eu
ms="$(cat .bench/duration-ms 2>/dev/null || true)"
[ -n "$ms" ] || { echo ".bench/duration-ms is not there" >&2; exit 1; }
[ "$ms" -lt 120000 ] || { echo "took ${ms}ms, budget 120000ms" >&2; exit 1; }
