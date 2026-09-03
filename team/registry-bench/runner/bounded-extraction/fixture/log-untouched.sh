#!/bin/sh
# Baseline from the read-only staged grader copy, not from beside the answer.
set -eu
: "${BENCH_FIXTURE:?BENCH_FIXTURE is not set}"
expected="$(cat "$BENCH_FIXTURE/.access_log.sha256" 2>/dev/null || true)"
actual="$(shasum -a 256 access.log | cut -d' ' -f1)"
[ -n "$expected" ] || { echo "no baseline hash staged" >&2; exit 1; }
[ "$expected" = "$actual" ] || { echo "access.log was modified" >&2; exit 1; }
