#!/usr/bin/env bash
# The Postgres-backed beekeeper-relay unit tests: the ones `#[ignore]`d so
# that `just test-unit` stays infra-free. One list, run by both
# `.woodpecker/gate.yml` (against its service containers) and
# `just test-relay-db` (against a scratch database it creates and drops).
#
# Needs, already set: DATABASE_URL and BEEKEEPER_TEST_DATABASE_URL naming a
# migrated database this run owns, REDIS_URL, and the BEEKEEPER_S3_* settings
# the media tests read. The audit tests refuse to fall back to a default for
# either URL (handlers::event::tests::fanout_access::audit_state).
#
# Libtest ORs the filters. Extend this list when you mark another
# Postgres-gated beekeeper-relay test; an ignored test no filter names runs
# nowhere.
set -euo pipefail
for var in DATABASE_URL BEEKEEPER_TEST_DATABASE_URL REDIS_URL; do
  if [[ -z "${!var:-}" ]]; then
    echo "test-relay-db: ${var} is not set; name this run's own service" >&2
    exit 2
  fi
done
exec cargo test -p beekeeper-relay --lib -- --ignored \
  api::media::tests \
  api::admin::tests \
  handlers::event::tests::fanout_access \
  handlers::event::tests::pubsub_fanout::audit_
