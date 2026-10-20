#!/usr/bin/env bash
# planning-plugin-check.sh — the Planning plugin against a real relay.
#
# Composes the real beekeeper-relay binary with the test plugin registry
# (target/plugins/registry.test.toml: planning, planning-fixture,
# test-faults) on a scratch Postgres database in the dev `buzz-postgres`
# container (dropped on exit) and Redis logical DB 13 (never 0 or the dev
# database), with TWO host→community mappings on one relay:
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
# Usage: scripts/planning-plugin-check.sh [--out <dir>] [--desktop] [--keep]
#   --out      where logs go (default: a fresh temp dir, kept on failure)
#   --desktop  also run desktop/tests/e2e/project-planning.live.spec.ts
#   --keep     leave the relay running at the end (prints how to stop it)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT=""
DESKTOP=false
KEEP=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --desktop) DESKTOP=true; shift ;;
    --keep) KEEP=true; shift ;;
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

RELAY_PID=""
cleanup() {
  local status=$?
  if [[ "${KEEP}" == true ]]; then
    log "relay left running (pid ${RELAY_PID}); stop it with: kill ${RELAY_PID}; then drop ${DB_NAME}"
    return
  fi
  [[ -n "${RELAY_PID}" ]] && kill "${RELAY_PID}" 2>/dev/null && wait "${RELAY_PID}" 2>/dev/null || true
  docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
    -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
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

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}
PORT="$(free_port)"
log "scratch database ${DB_NAME}, relay on :${PORT}"
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/${DB_NAME}" \
  REDIS_URL="redis://localhost:6379/13" \
  RELAY_URL="ws://localhost:${PORT}" \
  BEEKEEPER_BIND_ADDR="127.0.0.1:${PORT}" \
  BEEKEEPER_HEALTH_PORT="$(free_port)" \
  BEEKEEPER_METRICS_PORT="$(free_port)" \
  BEEKEEPER_RELAY_PRIVATE_KEY="$(openssl rand -hex 32)" \
  BEEKEEPER_REQUIRE_AUTH_TOKEN=false \
  BEEKEEPER_AUTO_MIGRATE=true \
  BEEKEEPER_GIT_REPO_PATH="${WORKDIR}/relay-git" \
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
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d "${DB_NAME}" -c "
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
    BEEKEEPER_E2E_RELAY_URL='ws://localhost:${PORT}' \
    pnpm exec playwright test --project=integration tests/e2e/project-planning.live.spec.ts"
fi

exit ${status}
