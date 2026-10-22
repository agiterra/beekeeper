# shellcheck shell=bash
# Sourced by the infra-free unit gates (`just test-unit`,
# `scripts/run-tests.sh unit`). .env usually names the dev database and dev
# Redis, and a test that reads DATABASE_URL or REDIS_URL with a default would
# write there; two audit tests missing their `#[ignore]` did. So every URL a
# test could pick up from the environment points at a closed port, and such a
# test fails or skips instead.
#
# Postgres tests are `#[ignore = "requires Postgres"]` and run in `just test`,
# each against a database that run creates. The Redis-gated tests (tunnel,
# mesh_demo, NIP-98 replay, presence fan-out) say SKIP on stderr without
# Redis; to run them here, name a Redis this run may write to:
#
#   BEEKEEPER_TEST_REDIS_URL=redis://localhost:56379 just test-unit
export DATABASE_URL="postgres://buzz:buzz_dev@127.0.0.1:1/unit_tests_have_no_database" # sadscan:disable np.postgres.1
export BEEKEEPER_TEST_DATABASE_URL="${DATABASE_URL}"
export TEST_DATABASE_URL="${DATABASE_URL}"
unset READ_DATABASE_URL
export REDIS_URL="${BEEKEEPER_TEST_REDIS_URL:-redis://127.0.0.1:1}"
