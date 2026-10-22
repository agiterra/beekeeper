#!/usr/bin/env bash
# =============================================================================
# foreign-client-check.sh — the relay, seen only from outside
# =============================================================================
#
# Brings up the real `beekeeper-relay` binary the way `just test` does (a
# scratch Postgres database in the dev Postgres container — `buzz-postgres`
# unless scripts/lib/dev-services.sh is told otherwise — dropped on
# exit; Redis logical DB 14, never DB 0), seeds one channel with the built
# `bee`, and then drives the relay with nothing but the foreign clients in
# `conformance/foreign-client/` — Node plus `nostr-tools`, no Beekeeper SDK,
# no host, no service files — asserting the outcomes `docs/INTEGRATING.md`
# documents:
#
#   1. NIP-11 is public and says what the relay speaks (`supported_kinds`);
#      an unsigned bridge read is refused.
#   2. A channel member's NIP-98 read is answered, with every signature valid.
#   3. A member's signed `session.stop` intent is admitted and stored; with no
#      provider running, no receipt follows (and nothing pretends one did).
#   4. A non-member's `session.stop` is refused by name (a `restricted:`
#      membership refusal), and a non-member's read of the private channel
#      shows nothing. (On an open channel the write is refused the same way
#      but the read is answered: open means readable.)
#   5. A host-step claim for a request that does not exist is refused
#      (`unknown host step`); correlation over an empty channel reports the
#      exchange incomplete rather than inventing one.
#   6. `collect-conversation` reads back exactly the signed intent from 3.
#   7. The offline host-step verifier (standard-library Python) accepts its
#      signed fixture and rejects the tampered ones.
#
# What this does NOT prove (named, not faked): no session provider runs, so
# no 44223/44224/44225 is produced and no owner-authorized control is
# answered; no workflow is published, so no relay-signed 46013 exists and the
# live claim → result → echo correlation of a real host step is exercised
# only by the offline fixture (step 7) — the live path was observed in the
# 2026-10-11 acceptance run and is reproduced by `just test-handover`'s
# neighbours, not here. No real model, one machine.
#
# Usage: ./scripts/foreign-client-check.sh
# Requires: hermit activated, Postgres+Redis+S3 up (`just _ensure-services` plus
#   `docker compose up -d rustfs rustfs-init`; the relay's git object-store probe needs S3),
#   BEE_BIN and RELAY_BIN built (the `foreign-client-check` just recipe builds
#   them), `node` and `python3` on PATH, `nostr-tools` resolvable (the
#   desktop install, or FOREIGN_CLIENT_NOSTR_TOOLS).
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
BEE_BIN="${BEE_BIN:-${TARGET_DIR}/debug/bee}"
RELAY_BIN="${RELAY_BIN:-${TARGET_DIR}/debug/beekeeper-relay}"
CLIENT_DIR="${REPO_ROOT}/conformance/foreign-client"

for bin in "${BEE_BIN}" "${RELAY_BIN}"; do
  if [[ ! -x "${bin}" ]]; then
    echo "missing built binary: ${bin} (run \`just foreign-client-check\`, which builds it first)" >&2
    exit 1
  fi
done
for tool in node python3 docker curl openssl; do
  command -v "${tool}" >/dev/null || { echo "missing tool on PATH: ${tool}" >&2; exit 1; }
done
# shellcheck source=lib/dev-services.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/dev-services.sh"
pg_health="$(dev_service_health "${BEEKEEPER_DEV_POSTGRES_CONTAINER}")"
if [[ "${pg_health}" != "healthy" ]]; then
  echo "${BEEKEEPER_DEV_POSTGRES_CONTAINER} is ${pg_health}; run \`just _ensure-services\` first" >&2
  exit 1
fi
# The relay refuses to start without its git object store: its startup
# conformance probe talks to the S3 endpoint the environment names (the
# `rustfs` service on the dev compose, port 9000). `_ensure-services` only
# checks Postgres and Redis, so say so here rather than dying three seconds
# in with an S3 error.
s3_health="$(dev_service_health "${BEEKEEPER_DEV_S3_CONTAINER}")"
if [[ "${s3_health}" != "healthy" ]]; then
  echo "${BEEKEEPER_DEV_S3_CONTAINER} is ${s3_health}; the relay's git object-store probe needs it — run \`docker compose up -d rustfs rustfs-init\`" >&2
  exit 1
fi

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
log() { echo -e "${BLUE}[foreign]${NC} $*"; }
ok()  { echo -e "${GREEN}[foreign]${NC} $*"; }
err() { echo -e "${RED}[foreign]${NC} $*" >&2; }

START_EPOCH="$(date +%s)"
WORKDIR="$(mktemp -d /tmp/foreign-client-check.XXXXXX)"
DB_NAME="buzz_foreign_client_$$_$(date +%s)"
RELAY_PID=""
RESULT_FILE="${WORKDIR}/results"
: > "${RESULT_FILE}"
FAIL_FILE="${WORKDIR}/failures"

cleanup() {
  local status=$?
  # Only the relay this script started; never another process.
  if [[ -n "${RELAY_PID}" ]]; then kill "${RELAY_PID}" 2>/dev/null || true; fi
  sleep 1
  dev_pg -d postgres \
    -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
  local passes fails
  passes="$(grep -c '^PASS' "${RESULT_FILE}" 2>/dev/null || true)"; passes="${passes:-0}"
  fails="$(grep -c '^FAIL' "${RESULT_FILE}" 2>/dev/null || true)"; fails="${fails:-0}"
  echo
  echo "──────────────────────────────────────────────────────────────────────"
  echo " foreign-client check — ${passes} passed, ${fails} failed, $(( $(date +%s) - START_EPOCH ))s wall"
  echo "──────────────────────────────────────────────────────────────────────"
  sed 's/^/  /' "${RESULT_FILE}" 2>/dev/null || true
  if [[ ${status} -ne 0 || -s "${FAIL_FILE}" ]]; then
    err "run did not pass cleanly (exit ${status}, ${fails} failed step(s)); logs and records preserved at ${WORKDIR}"
    exit 1
  fi
  ok "ALL STEPS PASSED"
  rm -rf "${WORKDIR}"
  exit 0
}
trap cleanup EXIT

pass() { printf 'PASS %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"; echo -e "${GREEN}[PASS $1]${NC} $2"; }
fail() { printf 'FAIL %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"; echo -e "${RED}[FAIL $1]${NC} $2" >&2; printf 'step %s\n' "$1" >> "${FAIL_FILE}"; }

# Each step is a subshell under its own `set -e`; the verdict is read from
# `$?` with errexit off in the parent only for the call — never as an `if`
# condition, which would swallow a failed assertion (see
# scripts/handover-acceptance.sh for the false passes that shape fixed).
run_step() {
  local num="$1" desc="$2" fn="$3" status
  log "── step ${num}: ${desc}"
  set +e
  ( set -euo pipefail; "${fn}" )
  status=$?
  set -e
  [[ "${status}" -eq 0 ]] || fail "${num}" "${desc} — exit ${status}; see ${WORKDIR}"
}

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}
jq_py() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }

# ── Relay ───────────────────────────────────────────────────────────────────
log "creating scratch database ${DB_NAME}..."
dev_pg -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

RELAY_PORT="$(free_port)"
RELAY_HTTP="http://127.0.0.1:${RELAY_PORT}"
RELAY_KEY="$(openssl rand -hex 32)"
log "starting beekeeper-relay on ${RELAY_HTTP} (scratch DB, Redis DB 14)..."
DATABASE_URL="$(dev_pg_url "${DB_NAME}")" \
  REDIS_URL="$(dev_redis_url 14)" \
  RELAY_URL="ws://127.0.0.1:${RELAY_PORT}" \
  BEEKEEPER_BIND_ADDR="127.0.0.1:${RELAY_PORT}" \
  BEEKEEPER_RELAY_PRIVATE_KEY="${RELAY_KEY}" \
  BEEKEEPER_REQUIRE_AUTH_TOKEN=false \
  BEEKEEPER_RECONCILE_CHANNELS=true \
  BEEKEEPER_AUTO_MIGRATE=true \
  BEEKEEPER_GIT_REPO_PATH="${WORKDIR}/relay-git" \
  "${RELAY_BIN}" > "${WORKDIR}/relay.log" 2>&1 &
RELAY_PID=$!

READY_CODE=""
for _ in $(seq 1 90); do
  if ! kill -0 "${RELAY_PID}" 2>/dev/null; then
    err "relay process died during startup"; cat "${WORKDIR}/relay.log" >&2; exit 1
  fi
  READY_CODE="$(curl -s -o /dev/null -w '%{http_code}' "${RELAY_HTTP}/_readiness" || true)"
  [[ "${READY_CODE}" == "200" ]] && break
  sleep 1
done
[[ "${READY_CODE}" == "200" ]] || { err "relay did not become ready"; cat "${WORKDIR}/relay.log" >&2; exit 1; }
RELAY_SELF="$(curl -s -H 'Accept: application/nostr+json' "${RELAY_HTTP}/" | jq_py 'd["self"]')"
ok "relay ready at ${RELAY_HTTP} (self ${RELAY_SELF:0:16}…)"

# ── Identities and one channel ──────────────────────────────────────────────
# Disposable keys, in memory and in 0600 files under WORKDIR only.
umask 077
MEMBER_KEY="$(openssl rand -hex 32)"
OUTSIDER_KEY="$(openssl rand -hex 32)"
printf 'BEEKEEPER_PRIVATE_KEY=%s\n' "${MEMBER_KEY}" > "${WORKDIR}/member.env"
printf 'BEEKEEPER_PRIVATE_KEY=%s\n' "${OUTSIDER_KEY}" > "${WORKDIR}/outsider.env"

export RELAY_URL="${RELAY_HTTP}"
export FOREIGN_CLIENT_OUT="${WORKDIR}/records"
export FOREIGN_CLIENT_RELAY_PUBKEY="${RELAY_SELF}"
client() { node "${CLIENT_DIR}/$1" "${@:2}"; }
as_member()   { FOREIGN_CLIENT_KEY_FILE="${WORKDIR}/member.env"   FOREIGN_CLIENT_LABEL=member   "$@"; }
as_outsider() { FOREIGN_CLIENT_KEY_FILE="${WORKDIR}/outsider.env" FOREIGN_CLIENT_LABEL=outsider "$@"; }

MEMBER_PUBKEY="$(as_member client foreign-identity.mjs | jq_py 'd["pubkey"]')"
OUTSIDER_PUBKEY="$(as_outsider client foreign-identity.mjs | jq_py 'd["pubkey"]')"

# The one place `bee` is used: to create the channel the member owns. Every
# assertion below is made by a foreign client, never by `bee`. The channel is
# **private**: an open channel is readable by any authenticated key (its
# writes are still membership-gated — the first run of this script proved
# that, and only that, on an open channel), and step 4's "an outsider sees
# nothing" is a claim about a private one.
CHANNEL="$(BEEKEEPER_RELAY_URL="${RELAY_HTTP}" BEEKEEPER_PRIVATE_KEY="${MEMBER_KEY}" "${BEE_BIN}" \
  channels create --name "foreign-client-$$" --type stream --visibility private \
  | jq_py 'd["channel_id"]')"
export FOREIGN_CLIENT_CHANNEL="${CHANNEL}"
ok "private channel ${CHANNEL} created by member ${MEMBER_PUBKEY:0:16}…; outsider ${OUTSIDER_PUBKEY:0:16}… never joins"

# A fictitious exact target: the relay validates the command's shape and the
# signer's membership, nothing about whether a provider exists.
TARGET='{"driver":"codex-acp","instanceId":"0000000000000000","sessionId":"11111111-1111-4111-8111-111111111111","generation":1}'
AUTHORITY="$(printf '%064d' 0 | tr 0 a)"

# ── Steps ───────────────────────────────────────────────────────────────────
step_info() {
  local out
  out="$(client foreign-info.mjs 44221 44224 46013 46014 46022 46023 24224)"
  echo "${out}" > "${WORKDIR}/step1.json"
  local unauth count
  unauth="$(echo "${out}" | jq_py 'd["unauthenticated_query_status"]')"
  count="$(echo "${out}" | jq_py 'd["supported_kinds_count"]')"
  [[ "${unauth}" -ge 400 ]] || { err "unsigned /query answered ${unauth}"; return 1; }
  echo "${out}" | jq_py '[k["schemas"] for k in d["requested"] if k["kind"]==46022][0]' | grep -q 'buzz-host-step/v1' || { err "46022 lacks buzz-host-step/v1"; return 1; }
  echo "${out}" | jq_py '[k["schemas"] for k in d["requested"] if k["kind"]==44221][0]' | grep -q 'buzz-coding-session-lifecycle-command/v1' || { err "44221 lacks its schema"; return 1; }
  pass 1 "NIP-11 advertises ${count} kinds with schemas (46022 → buzz-host-step/v1); unsigned /query answered ${unauth}"
}

step_member_read() {
  local out status count
  out="$(as_member client foreign-session.mjs read)"
  status="$(echo "${out}" | jq_py 'd["status"]')"
  count="$(echo "${out}" | jq_py 'd["count"]')"
  [[ "${status}" == "200" && "${count}" == "0" ]] || { err "member read: ${out}"; return 1; }
  pass 2 "member's NIP-98 read of 44222–44225 answered 200 with ${count} events (fresh channel), signatures verified"
}

step_member_stop() {
  local out status accepted
  out="$(as_member client foreign-session.mjs stop "op-member-$$" "${TARGET}" "${AUTHORITY}")"
  status="$(echo "${out}" | jq_py 'd["status"]')"
  accepted="$(echo "${out}" | jq_py 'd["response"].get("accepted")')"
  [[ "${status}" == "200" && "${accepted}" == "True" ]] || { err "member stop not admitted: ${out}"; return 1; }
  # Stored: the member reads its own signed intent back, verified.
  local stored
  stored="$(as_member client foreign-native-read.mjs "{\"kinds\":[44221],\"#h\":[\"${CHANNEL}\"]}" member-44221.json | jq_py 'len(d)')"
  [[ "${stored}" == "1" ]] || { err "expected 1 stored 44221, got ${stored}"; return 1; }
  # No provider runs: no receipt may appear, and the client must not invent one.
  local receipts
  receipts="$(as_member client foreign-native-read.mjs "{\"kinds\":[44224],\"#h\":[\"${CHANNEL}\"]}" member-44224.json | jq_py 'len(d)')"
  [[ "${receipts}" == "0" ]] || { err "a 44224 appeared with no provider running"; return 1; }
  pass 3 "member's session.stop admitted and stored (1 × 44221 read back); 0 × 44224 — no provider, no invented receipt"
}

step_outsider() {
  local out status error
  out="$(as_outsider client foreign-session.mjs stop "op-outsider-$$" "${TARGET}" "${AUTHORITY}")"
  status="$(echo "${out}" | jq_py 'd["status"]')"
  error="$(echo "${out}" | jq_py 'd["response"].get("error") or d["response"].get("message") or ""')"
  [[ "${status}" -ge 400 ]] || { err "outsider stop was admitted: ${out}"; return 1; }
  # Refused by name: a `restricted:` refusal that says membership is the
  # reason. The sentence depends on the channel ("not a channel member" on a
  # private one, "coding-session events require channel membership" on an
  # open one, observed 2026-10-14), so the assertion is on the class and the
  # word, not the prose.
  echo "${error}" | grep -q '^restricted:' || { err "outsider refusal is not restricted: ${error}"; return 1; }
  echo "${error}" | grep -qi 'member' || { err "outsider refusal does not name membership: ${error}"; return 1; }
  local read count
  read="$(as_outsider client foreign-session.mjs read)"
  count="$(echo "${read}" | jq_py 'd.get("count")')"
  [[ "${count}" == "0" || "${count}" == "None" ]] || { err "outsider read saw ${count} events"; return 1; }
  local hidden
  hidden="$(as_outsider client foreign-native-read.mjs "{\"kinds\":[44221],\"#h\":[\"${CHANNEL}\"]}" outsider-44221.json | jq_py 'len(d)')"
  [[ "${hidden}" == "0" ]] || { err "outsider can read the member's 44221"; return 1; }
  pass 4 "outsider's session.stop refused ${status} \"${error}\"; outsider's reads of the private channel show 0 events"
}

step_hook() {
  local run="22222222-2222-4222-8222-222222222222"
  local fake_request
  fake_request="$(printf '%064d' 0 | tr 0 e)"
  local out admitted msg
  out="$(as_member client foreign-hook.mjs claim "${run}" "summarize" "${fake_request}")"
  admitted="$(echo "${out}" | jq_py 'd["claimAdmitted"]')"
  msg="$(echo "${out}" | jq_py '(d["claimResponse"] or {}).get("error") or (d["claimResponse"] or {}).get("message") or ""')"
  [[ "${admitted}" == "False" ]] || { err "claim of a nonexistent request was admitted: ${out}"; return 1; }
  echo "${msg}" | grep -qi 'unknown host step' || { err "claim refusal does not say unknown host step: ${msg}"; return 1; }
  # Correlation over a channel with no request must report incomplete (exit 3).
  set +e
  local corr; corr="$(as_member client foreign-hook.mjs correlate "${run}")"; local code=$?
  set -e
  [[ "${code}" -eq 3 ]] || { err "correlate exited ${code}, expected 3: ${corr}"; return 1; }
  echo "${corr}" | jq_py 'd["complete"]' | grep -q False || { err "correlate claimed completeness"; return 1; }
  pass 5 "claim of an unknown request refused \"${msg}\"; correlation over an empty channel reports incomplete (exit 3), nothing invented"
}

step_collect() {
  local out count kinds
  out="$(as_member client collect-conversation.mjs check)"
  count="$(echo "${out}" | jq_py 'd["verifiedEventCount"]')"
  kinds="$(echo "${out}" | jq_py '",".join(map(str,d["kinds"]))')"
  [[ "${count}" == "1" && "${kinds}" == "44221" ]] || { err "collect: ${out}"; return 1; }
  pass 6 "collect-conversation read back exactly the member's signed 44221 (${count} event, kinds ${kinds})"
}

step_offline_verifier() {
  ( cd conformance/headless-host-step && python3 test_client.py >"${WORKDIR}/offline-tests.log" 2>&1 ) || { cat "${WORKDIR}/offline-tests.log" >&2; return 1; }
  local out
  out="$(python3 conformance/headless-host-step/client.py conformance/headless-host-step/exchange.json)"
  echo "${out}" | jq_py 'd["disposition"]' | grep -q exited || { err "offline verifier: ${out}"; return 1; }
  pass 7 "offline host-step verifier: unit fixtures pass; exchange.json verifies (disposition exited, unsupported capabilities disclosed)"
}

run_step 1 "NIP-11 says what the relay speaks; unsigned reads are refused" step_info
run_step 2 "a member reads the channel over NIP-98" step_member_read
run_step 3 "a member's signed stop is admitted and stored; no receipt is invented" step_member_stop
run_step 4 "an outsider's stop is refused by name; an outsider sees nothing" step_outsider
run_step 5 "host-step claim of an unknown request is refused; correlation stays honest" step_hook
run_step 6 "collect-conversation reads back the signed intent" step_collect
run_step 7 "offline host-step verifier (stdlib Python)" step_offline_verifier
