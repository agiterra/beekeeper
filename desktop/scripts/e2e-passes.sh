#!/usr/bin/env bash
# Run Playwright as two passes: the given projects in parallel, then the
# `smoke-serial` project alone with nothing else running. `smoke-serial` holds
# files that pass alone but fail beside other workers (see playwright.config.ts);
# Playwright skips a project's dependents when the dependency has any failure,
# so `dependencies` cannot order it, and running it in the same invocation
# would put it beside the parallel workers.
#
#   scripts/e2e-passes.sh <project>... [-- <playwright args / filters>]
#
# Extra arguments go to both passes. With filters, a pass that matches none of
# them is not a failure, but a run in which neither pass found a test is.
# Exits with the first pass's code if it failed, else the second's.
set -uo pipefail
cd "$(dirname "$0")/.."

projects=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
    projects+=("--project=$1")
    shift
done
[ "${1:-}" = "--" ] && shift
# pnpm forwards a literal `--` from `pnpm test:e2e:smoke -- x.spec.ts`.
[ "${1:-}" = "--" ] && shift

extra=()
if [ $# -gt 0 ]; then extra=(--pass-with-no-tests); fi

listed() {
    pnpm exec playwright test --list "$@" ${extra[@]+"${extra[@]}"} 2>/dev/null |
        grep -Eo '^Total: [0-9]+' | grep -Eo '[0-9]+' || echo 0
}
if [ $# -gt 0 ]; then
    found=$(( $(listed "${projects[@]}" "$@") + $(listed --project=smoke-serial "$@") ))
    if [ "$found" -eq 0 ]; then
        echo "e2e-passes: no test matches: $*" >&2
        exit 1
    fi
fi

pnpm exec playwright test "${projects[@]}" ${extra[@]+"${extra[@]}"} "$@"
first=$?
pnpm exec playwright test --project=smoke-serial ${extra[@]+"${extra[@]}"} "$@"
second=$?
if [ "$first" -ne 0 ]; then exit "$first"; fi
exit "$second"
