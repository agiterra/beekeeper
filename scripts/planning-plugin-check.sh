#!/usr/bin/env bash
# planning-plugin-check.sh — the Planning plugin against a real relay.
#
# Composes the real beekeeper-relay binary with the test plugin registry
# (target/plugins/registry.test.toml: planning, planning-fixture,
# test-faults) on its own throwaway Postgres and Redis containers, started on
# free loopback ports and removed on exit, so it shares no database or Redis
# with the dev stack or anything else running here. (`--shared-dev` instead
# uses a scratch database in the dev Postgres, dropped on exit, and Redis
# logical DB 13 — `buzz-postgres` / `buzz-redis` unless
# scripts/lib/dev-services.sh names others.) TWO host→community mappings on one
# relay:
#
#   localhost:<port>  → community A   (RELAY_URL)
#   127.0.0.1:<port>  → community B   (RELAY_URL_B)
#
# so the cross-community tests bind to a genuinely different tenant. Then it
# runs, in order:
#
#   1. e2e_plugins          — the plugin route end to end
#   2. e2e_project_todos    — NIP-TD itself, unchanged by the plugin move
#   3. CLI                  — `bee planning` through the plugin, signing as the
#                             member, read back by `bee todos`
#   4. (with --desktop) the Playwright live spec, UI → CLI → UI
#
# Usage: scripts/planning-plugin-check.sh [--out <dir>] [--desktop] [--keep] [--shared-dev]
#   --out      where logs go (default: a fresh temp dir, kept on failure)
#   --desktop  also run desktop/tests/e2e/project-planning.live.spec.ts
#   --keep     leave the relay running at the end (prints how to stop it)
#   --shared-dev  use the dev buzz-postgres / buzz-redis (DB 13) instead of
#              dedicated containers
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT=""
DESKTOP=false
KEEP=false
DEDICATED=true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --desktop) DESKTOP=true; shift ;;
    --keep) KEEP=true; shift ;;
    --shared-dev) DEDICATED=false; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
WORKDIR="$(mktemp -d -t planning-plugin-check)"
OUT="${OUT:-${WORKDIR}}"
mkdir -p "${OUT}"
DB_NAME="buzz_planning_plugin_$$"
RESULTS="${OUT}/results.txt"
: >"${RESULTS}"

log() { echo "[planning-check] $*"; }
record() { echo "$*" | tee -a "${RESULTS}"; }

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}
if [[ "${DEDICATED}" == true ]]; then
  PG_CONTAINER="planning-check-$$-postgres"
  REDIS_CONTAINER="planning-check-$$-redis"
  PG_PORT="$(free_port)"
  REDIS_PORT="$(free_port)"
  REDIS_URL_VALUE="redis://127.0.0.1:${REDIS_PORT}/0"
else
  # shellcheck source=lib/dev-services.sh
  source "${ROOT}/scripts/lib/dev-services.sh"
  PG_CONTAINER="${BEEKEEPER_DEV_POSTGRES_CONTAINER}"
  REDIS_CONTAINER=""
  PG_PORT="${BEEKEEPER_DEV_POSTGRES_PORT}"
  REDIS_URL_VALUE="$(dev_redis_url 13)"
fi
# Over TCP, never the unix socket. On first boot the postgres image runs a
# temporary init server that listens only on the socket, answers queries,
# then shuts down and restarts as the real one — so a socket probe reports
# ready too early and the CREATE DATABASE after it can land on a server that
# is going away. The init server never listens on TCP.
pg() { docker exec -e PGPASSWORD=buzz_dev "${PG_CONTAINER}" psql -h 127.0.0.1 -U buzz -q "$@"; }
# The published port, from this side, as the relay will reach it.
host_port_open() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

RELAY_PID=""
cleanup() {
  local status=$?
  if [[ "${KEEP}" == true ]]; then
    log "relay left running (pid ${RELAY_PID}); stop it with: kill ${RELAY_PID}; then drop ${DB_NAME}"
    return
  fi
  [[ -n "${RELAY_PID}" ]] && kill "${RELAY_PID}" 2>/dev/null && wait "${RELAY_PID}" 2>/dev/null || true
  if [[ "${DEDICATED}" == true ]]; then
    # Only the two containers this run started, by the names it gave them.
    docker rm -f "${PG_CONTAINER}" "${REDIS_CONTAINER}" >/dev/null 2>&1 || true
  else
    pg -d postgres -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
  fi
  echo
  echo "── planning plugin check ─────────────────────────────────────────────"
  cat "${RESULTS}"
  echo "logs: ${OUT}"
  [[ "${OUT}" != "${WORKDIR}" ]] && rm -rf "${WORKDIR}"
  exit ${status}
}
trap cleanup EXIT

log "building plugins, relay, bee and the e2e tests..."
./scripts/build-plugins.sh >"${OUT}/build-plugins.log" 2>&1
cargo build -p beekeeper-relay -p beekeeper-cli >"${OUT}/build.log" 2>&1
cargo test -p beekeeper-test-client --test e2e_plugins --test e2e_project_todos --no-run >>"${OUT}/build.log" 2>&1
grep '^plugin ' "${OUT}/build-plugins.log" | tee -a "${RESULTS}"

if [[ "${DEDICATED}" == true ]]; then
  log "dedicated postgres ${PG_CONTAINER} on :${PG_PORT}, redis ${REDIS_CONTAINER} on :${REDIS_PORT}"
  docker run -d --rm --name "${PG_CONTAINER}" -p "127.0.0.1:${PG_PORT}:5432" \
    -e POSTGRES_USER=buzz -e POSTGRES_PASSWORD=buzz_dev -e POSTGRES_DB=postgres \
    postgres:17-alpine >/dev/null
  docker run -d --rm --name "${REDIS_CONTAINER}" -p "127.0.0.1:${REDIS_PORT}:6379" \
    redis:7-alpine >/dev/null
  ready=false
  for _ in $(seq 1 60); do
    if pg -d postgres -c "SELECT 1" >/dev/null 2>&1 && host_port_open "${PG_PORT}" \
      && docker exec "${REDIS_CONTAINER}" redis-cli ping >/dev/null 2>&1 \
      && host_port_open "${REDIS_PORT}"; then
      ready=true
      break
    fi
    sleep 1
  done
  if [[ "${ready}" != true ]]; then
    log "dedicated postgres/redis not ready over TCP within 60s"
    docker logs "${PG_CONTAINER}" 2>&1 | tail -20
    exit 1
  fi
fi
PORT="$(free_port)"
log "scratch database ${DB_NAME}, relay on :${PORT}"
pg -d postgres -c "CREATE DATABASE ${DB_NAME};" >/dev/null

DATABASE_URL="postgres://buzz:buzz_dev@127.0.0.1:${PG_PORT}/${DB_NAME}" \
  REDIS_URL="${REDIS_URL_VALUE}" \
  RELAY_URL="ws://localhost:${PORT}" \
  BEEKEEPER_BIND_ADDR="127.0.0.1:${PORT}" \
  BEEKEEPER_HEALTH_PORT="$(free_port)" \
  BEEKEEPER_METRICS_PORT="$(free_port)" \
  BEEKEEPER_RELAY_PRIVATE_KEY="$(openssl rand -hex 32)" \
  BEEKEEPER_REQUIRE_AUTH_TOKEN=false \
  BEEKEEPER_AUTO_MIGRATE=true \
  BEEKEEPER_GIT_REPO_PATH="${WORKDIR}/relay-git" \
  BEEKEEPER_BUILTIN_PLUGINS_FILE= \
  BEEKEEPER_PLUGINS_FILE="${ROOT}/target/plugins/registry.test.toml" \
  ./target/debug/beekeeper-relay >"${OUT}/relay.log" 2>&1 &
RELAY_PID=$!

ready=""
for _ in $(seq 1 120); do
  kill -0 "${RELAY_PID}" 2>/dev/null || { log "relay died"; tail -40 "${OUT}/relay.log"; exit 1; }
  ready="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PORT}/_readiness" || true)"
  [[ "${ready}" == 200 ]] && break
  sleep 1
done
[[ "${ready}" == 200 ]] || { log "relay never became ready"; tail -40 "${OUT}/relay.log"; exit 1; }
pg -d "${DB_NAME}" -c "
  INSERT INTO communities (id, host) VALUES
    ('00000000-0000-4000-8000-00000000000a', 'localhost:${PORT}'),
    ('00000000-0000-4000-8000-00000000000b', '127.0.0.1:${PORT}')
  ON CONFLICT (lower(host)) DO NOTHING;" >/dev/null
grep -o '"message":"plugin loaded"[^}]*' "${OUT}/relay.log" | tee -a "${RESULTS}" || true

export RELAY_URL="ws://localhost:${PORT}"
export RELAY_URL_B="ws://127.0.0.1:${PORT}"
status=0

run() {
  local name="$1"; shift
  set +e
  "$@" >"${OUT}/${name}.log" 2>&1
  local code=$?
  set -e
  local summary
  summary="$(grep -E '^test result|CHECK PASSED|passed|failed' "${OUT}/${name}.log" | tail -1 || true)"
  record "${name}: exit ${code} ${summary}"
  [[ ${code} -eq 0 ]] || status=1
}

run e2e_plugins cargo test -p beekeeper-test-client --test e2e_plugins -- --ignored --test-threads=4
run e2e_project_todos cargo test -p beekeeper-test-client --test e2e_project_todos -- --ignored --test-threads=4

if [[ -x "${ROOT}/scripts/planning-plugin-cli-check.sh" ]]; then
  run cli "${ROOT}/scripts/planning-plugin-cli-check.sh" "ws://localhost:${PORT}"
fi

if [[ "${DESKTOP}" == true ]]; then
  run desktop-live bash -c "cd desktop && pnpm build:e2e >/dev/null && \
    BEEKEEPER_E2E_PROJECT_PLANNING=1 BEEKEEPER_E2E_CLI_BIN='${ROOT}/target/debug/bee' \
    BEEKEEPER_E2E_RELAY_URL='http://localhost:${PORT}' \
    pnpm exec playwright test --project=integration tests/e2e/project-planning.live.spec.ts"
  mkdir -p "${OUT}/screenshots"
  cp desktop/test-results/planning/*.png "${OUT}/screenshots/" 2>/dev/null || true
fi

exit ${status}
