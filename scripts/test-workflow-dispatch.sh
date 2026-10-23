#!/usr/bin/env bash
# The Postgres-backed workflow dispatch and vendor ingress tests: the durable
# queue (claim, lease reclaim and fencing, freeze, outcomes, exhaustion), B1
# delivery recording, the SQL candidate prefilter, and the relay worker,
# including the end-to-end seal -> B1 -> run_once test against the built
# test-webhook component. One list, run by both `.woodpecker/gate.yml`
# (against its service containers) and `just test-workflow-dispatch` (against
# a scratch database it creates and drops).
#
# Needs, already set: DATABASE_URL and BEEKEEPER_TEST_DATABASE_URL naming a
# migrated database this run owns, REDIS_URL, and BEEKEEPER_PLUGIN_ARTIFACTS
# naming a directory where scripts/build-plugins.sh wrote test-webhook.wasm.
# The seal test panics, never skips, when the fixture is absent.
#
# Libtest ORs the filters. Extend this list when you mark another
# Postgres-gated dispatch test; an ignored test no filter names runs nowhere.
set -euo pipefail
for var in DATABASE_URL BEEKEEPER_TEST_DATABASE_URL REDIS_URL BEEKEEPER_PLUGIN_ARTIFACTS; do
  if [[ -z "${!var:-}" ]]; then
    echo "test-workflow-dispatch: ${var} is not set; name this run's own service" >&2
    exit 2
  fi
done
if [[ ! -f "${BEEKEEPER_PLUGIN_ARTIFACTS}/test-webhook.wasm" ]]; then
  echo "test-workflow-dispatch: no test-webhook.wasm in ${BEEKEEPER_PLUGIN_ARTIFACTS}; run scripts/build-plugins.sh first" >&2
  exit 2
fi
# Each filter must name at least one ignored test. A filter that matches
# nothing passes silently: `workflow_dispatch::tests` once ran zero tests here
# because the module's Postgres tests live in `db_tests`.
run() {
  local crate="$1"; shift
  local filter n
  for filter in "$@"; do
    n=$(cargo test -q -p "$crate" --lib -- --ignored --list "$filter" | grep -c ': test$' || true)
    if [[ "$n" -eq 0 ]]; then
      echo "test-workflow-dispatch: filter '${filter}' names no ignored test in ${crate}" >&2
      exit 1
    fi
  done
  cargo test -p "$crate" --lib -- --ignored --test-threads=1 "$@"
}
run beekeeper-db \
  workflow_dispatch:: \
  vendor_ingress::tests \
  workflow::tests::dispatch_channel_candidates_prefilter_and_refuse_to_truncate
run beekeeper-relay \
  workflow_dispatch_runtime::tests
