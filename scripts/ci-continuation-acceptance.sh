#!/usr/bin/env bash
# =============================================================================
# ci-continuation-acceptance.sh — real composition proof for CI-managed turn
# continuation (docs/CI_MANAGED_CONTINUATION_IMPL.md, docs/CI_MANAGED_CONTINUATION_SPEC.md)
# =============================================================================
#
# Composes, as independent OS processes talking over a real network socket:
#   - the actual `beekeeper-relay` binary, against a scratch Postgres database
#     (dropped on exit) and Redis logical DB 14 — never the dev database or
#     Redis DB 0;
#   - the built `bee` CLI, driving every step a real operator would: repo,
#     project, channel, and workflow announcement, session creation, CI
#     continuation registration, and read-only verification queries;
#   - the real `beekeeper-session-provider` binary, with a minimal ACP-speaking
#     bash script standing in for the model adapter (`agentCommand: bash`),
#     the same technique `crates/beekeeper-session-provider/src/session.rs`'s own
#     `testing::fake_agent`/`GOOD_AGENT` uses inside that crate's unit suite —
#     a real subprocess boundary, no network model call.
#
# No step is hand-signed as the relay or faked: the CI result (kind 46008) is
# produced by POSTing to the workflow's real `/hooks/<id>` webhook, the exact
# production path `crates/beekeeper-relay/src/api/bridge_ci_result_tests.rs`
# exercises for the plain (non-continuation) CI-completion flow.
#
# What this proves, in order (each also printed as a numbered PASS line):
#   1. `bee sessions create --wait` returns a confirmed cs-target from the
#      live provider (a real ACP `session/new` round trip against the fake
#      agent).
#   2. `bee ci continue` registers a `thread.turn.continue_on_ci` 44220 and
#      exits 0, printing a `continuationRegistered`-stage ack backed by a
#      durable 44224 receipt.
#   3. The workflow's webhook produces a genuine relay-signed 46008 for the
#      exact identity — not a hand-signed event (a client-signed 46008 is
#      rejected by relay ingest; only the webhook producer path can create
#      one).
#   4. The provider's live WS listener observes the 46008, verifies its
#      signer and identity, and starts exactly one continuation turn: the
#      fake agent's raw ACP log grows by exactly one `session/prompt`, a
#      `turn_started` 44224 receipt names the registered commandId, and the
#      exact text delivered in that ACP request is byte-for-byte the 44225
#      transcript's `user_prompt`. The materialized JSON is then checked
#      against every identity and correlation field in the actual signed
#      registration and relay result events.
#   5. A duplicate 46008 (identical webhook replay) and a second registration
#      naming a different commandId for the same identity+target both leave
#      the raw ACP log at exactly one continuation prompt: the once-admission
#      fence (§0) holds across both duplicate shapes, and the second
#      registration is durably refused (DUPLICATE_OPERATION).
#   6. Scenario A (docs/CI_CONTINUATION_RECOVERY_SPEC.md §5): a registration
#      left `waiting`, the real provider process `kill -9`'d and restarted
#      over the same state dir, THEN the 46008 arrives. Exactly one
#      `turn_started` names the original commandId, the 44223 for the started
#      generation still names the ORIGINAL target (no generation bump), a
#      `session_restored_native` transcript row precedes the delivered
#      prompt, the stub's `methods.log` shows `session/load` after the
#      restart and never `session/new`, and the restarted provider's own log
#      never shows `NO_LIVE_EXECUTION`.
#   7. Scenario B: the 46008 arrives first (record reaches `ready` in
#      `ci-continuations.json`), THEN the provider is killed before delivery
#      and restarted. Same assertions as scenario A. The race between
#      observing `ready` and the kill landing before admission is tight; a
#      run is accepted only when the store is still Ready and neither start
#      ledger has a claim. A lost race is retried up to 3 times and reported.
#   8. Scenario C: the same waiting→kill→restart→result shape, but the ACP
#      stub for this run advertises `session/load` and then REJECTS it. The
#      result is a durable `turn_refused/NATIVE_RESTORE_REJECTED` naming the
#      original commandId, with no `session/new` anywhere in the stub's
#      method log after the restart and no `turn_started` ever.
#
# What this does NOT prove (named, not faked):
#   - The private-project read path (§3f). This registers against a
#     `--access public` project, so admission never exercises
#     CI_RESULT_UNAVAILABLE_OR_HIDDEN or a provider-admitted private read.
#     That path is exercised by the provider's own unit suite
#     (crates/beekeeper-session-provider/src/tests/ci_continuation_tests.rs).
#   - A real model. The ACP adapter is a bash stub; no network call to any
#     LLM provider happens anywhere in this script.
#   - Seated (agent-actor) restore. Every session this script creates is
#     operator-created (unseated), so `native_restore`'s actor-seat lookup is
#     never exercised here; that path is proven by the provider's own unit
#     suite (`crates/beekeeper-session-provider/src/tests/ci_continuation_restore_tests.rs`).
#
# Set CI_CONTINUATION_SCENARIOS=basic to skip steps 6-8 (the restart
# scenarios) and run only the original five steps; default is `all`.
#
# Usage: ./scripts/ci-continuation-acceptance.sh
# Requires: hermit activated, Postgres+Redis+MinIO up (`just _ensure-services`),
#   and BEE_BIN/RELAY_BIN/PROVIDER_BIN built (the `test-ci-continuation` just
#   recipe builds them; set the env vars below to reuse existing binaries).
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
BEE_BIN="${BEE_BIN:-${TARGET_DIR}/debug/bee}"
RELAY_BIN="${RELAY_BIN:-${TARGET_DIR}/debug/beekeeper-relay}"
PROVIDER_BIN="${PROVIDER_BIN:-${TARGET_DIR}/debug/beekeeper-session-provider}"

for bin in "${BEE_BIN}" "${RELAY_BIN}" "${PROVIDER_BIN}"; do
  if [[ ! -x "${bin}" ]]; then
    echo "missing built binary: ${bin} (run \`just test-ci-continuation\`, which builds it first)" >&2
    exit 1
  fi
done

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${BLUE}[ci-continuation]${NC} $*"; }
ok()   { echo -e "${GREEN}[ci-continuation]${NC} $*"; }
err()  { echo -e "${RED}[ci-continuation]${NC} $*" >&2; }
pass() { echo -e "${GREEN}[PASS $1]${NC} $2"; }

WORKDIR="$(mktemp -d /tmp/ci-continuation-acceptance.XXXXXX)"
DB_NAME="buzz_ci_continuation_$$_$(date +%s)"
RELAY_PID=""
PROVIDER_PID=""
# Every provider generation this script has spawned (initial start, every
# restart in scenarios A/B, and scenario C's separate provider identity) —
# cleanup kills all of them, not just the most recent.
PROVIDER_PIDS=()

cleanup() {
  local status=$?
  local pid
  for pid in "${PROVIDER_PIDS[@]:-}"; do
    [[ -n "${pid}" ]] && kill -9 "${pid}" 2>/dev/null || true
  done
  [[ -n "${RELAY_PID}" ]] && kill "${RELAY_PID}" 2>/dev/null || true
  sleep 1
  docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
    -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
  if [[ ${status} -ne 0 ]]; then
    err "FAILED — logs and state preserved at ${WORKDIR}"
    err "relay log:    ${WORKDIR}/relay.log"
    err "provider log: ${WORKDIR}/provider.log"
  else
    rm -rf "${WORKDIR}"
  fi
  exit "${status}"
}
trap cleanup EXIT

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}

# Shared python helpers (target-key encoding, JSON file loading) used by both
# the original steps and the restart scenarios, written once so the encoding
# rule (`crates/beekeeper-sdk/src/builders.rs::coding_session_target_key`) lives in
# exactly one place in this script.
cat > "${WORKDIR}/helpers.py" <<'PY'
import json


def target_to_key(target):
    fields = [target["driver"], target["instanceId"], target["sessionId"], str(target["generation"])]
    return "coding-session/v1|" + "".join(f"{len(field.encode('utf-8'))}:{field}" for field in fields)


def load_json_file(path):
    with open(path, encoding="utf-8") as source:
        return json.load(source)
PY

# ── Scratch database (never the dev DB) ──────────────────────────────────────
log "Creating scratch database ${DB_NAME}..."
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

# ── Real relay binary, scratch DB, Redis logical DB 14 ───────────────────────
RELAY_PORT="$(free_port)"
RELAY_URL="ws://127.0.0.1:${RELAY_PORT}"
RELAY_HTTP="http://127.0.0.1:${RELAY_PORT}"
RELAY_KEY="$(openssl rand -hex 32)"

log "Starting beekeeper-relay on ${RELAY_HTTP} (scratch DB, Redis DB 14)..."
DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/${DB_NAME}" \
  REDIS_URL="redis://localhost:6379/14" \
  RELAY_URL="${RELAY_URL}" \
  BEEKEEPER_BIND_ADDR="127.0.0.1:${RELAY_PORT}" \
  BEEKEEPER_RELAY_PRIVATE_KEY="${RELAY_KEY}" \
  BEEKEEPER_REQUIRE_AUTH_TOKEN=false \
  BEEKEEPER_RECONCILE_CHANNELS=true \
  BEEKEEPER_AUTO_MIGRATE=true \
  "${RELAY_BIN}" > "${WORKDIR}/relay.log" 2>&1 &
RELAY_PID=$!

for _ in $(seq 1 60); do
  if ! kill -0 "${RELAY_PID}" 2>/dev/null; then
    err "relay process died during startup"
    cat "${WORKDIR}/relay.log" >&2
    exit 1
  fi
  code="$(curl -s -o /dev/null -w '%{http_code}' "${RELAY_HTTP}/_readiness" || true)"
  [[ "${code}" == "200" ]] && break
  sleep 1
done
[[ "${code}" == "200" ]] || { err "relay did not become ready"; cat "${WORKDIR}/relay.log" >&2; exit 1; }
ok "relay ready at ${RELAY_HTTP}"

bee() { "${BEE_BIN}" "$@"; }

# ── Real fixtures: repo, project, channel, workflow — all via bee ───────────
export BEEKEEPER_RELAY_URL="${RELAY_HTTP}"

OWNER_KEY="$(openssl rand -hex 32)"
PROVIDER_KEY="$(openssl rand -hex 32)"
REPO_ID="ci-cont-repo-$$"
PROJECT_SLUG="ci-cont-project-$$"

log "Announcing repository, project, channel, workflow as the owner identity..."
BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee repos create --id "${REPO_ID}" > "${WORKDIR}/repo.json"
OWNER_HEX="$(python3 -c "
import json,sys
d=json.load(open('${WORKDIR}/repo.json'))
from urllib.parse import urlparse, parse_qs
q=parse_qs(urlparse(d['link']).query)
print(q['owner'][0])
")"
PROJECT="30621:${OWNER_HEX}:${PROJECT_SLUG}"
REPO="30617:${OWNER_HEX}:${REPO_ID}"

BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee projects create "${PROJECT_SLUG}" --repo "${REPO_ID}" --access public >/dev/null
BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee repos bind --id "${REPO_ID}" --project "${PROJECT}" >/dev/null

CHANNEL="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee channels create --name "ci-cont-$$" --type stream --visibility open \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])')"

cat > "${WORKDIR}/workflow.yaml" <<EOF
name: Record build result
trigger:
  on: webhook
steps:
  - id: record
    action: record_ci_result
    project: "${PROJECT}"
    repository: "${REPO}"
    check: "main-validation"
    phase: build
    commit: "{{trigger.commit}}"
    run: "{{trigger.run}}"
    attempt: "{{trigger.attempt}}"
    conclusion: "{{trigger.conclusion}}"
    evidence_url: "{{trigger.evidence_url}}"
    summary: "{{trigger.summary}}"
EOF
WORKFLOW_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee workflows create --channel "${CHANNEL}" --yaml - < "${WORKDIR}/workflow.yaml")"
WORKFLOW_ID="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['workflow_id'])" "${WORKFLOW_JSON}")"
WEBHOOK_SECRET="$(python3 -c "
import json,sys
d=json.loads(sys.argv[1])
inner=json.loads(d['message'][len('response:'):])
print(inner['webhook_secret'])
" "${WORKFLOW_JSON}")"
ok "fixtures ready: project=${PROJECT} repository=${REPO} channel=${CHANNEL} workflow=${WORKFLOW_ID}"

# The provider must be a channel member (kind:39002, #p) before it starts —
# channel discovery (crates/beekeeper-acp/src/relay.rs discover_channels) runs
# once at startup, from membership, never from the projects file alone.
BEEKEEPER_PRIVATE_KEY="${PROVIDER_KEY}" bee channels join --channel "${CHANNEL}" >/dev/null

# ── Real provider binary, fake ACP adapter (no model call) ──────────────────
ACP_REQUEST_LOG="${WORKDIR}/acp-requests.jsonl"
: > "${ACP_REQUEST_LOG}"
FAKE_AGENT="${WORKDIR}/fake-agent.sh"
cat > "${FAKE_AGENT}" <<'AGENT'
#!/bin/bash
# Minimal ACP-speaking stub — the same technique
# crates/beekeeper-session-provider/src/session.rs's testing::GOOD_AGENT (and, for
# the restart shape, testing::restorable_agent) uses in that crate's own unit
# suite. Answers initialize/session.new/session.load/session.prompt over
# JSON-RPC on stdio; never calls a model. Logs each raw session/new and
# session/prompt request to FABLE_ACP_REQUEST_LOG so the caller can inspect
# the exact ACP payload, and every method name it receives to
# FABLE_METHODS_LOG so a restart scenario can prove session/load fired and
# session/new did not. Mints a distinct sessionId per new conversation, records
# issued cursors beside the request log, and rejects unknown load cursors.
# The assertions below also compare load/prompt identity to the exact cursor
# persisted before the kill; accepting another known cursor cannot pass.
# Advertises `loadSession` at initialize — this is the
# "accepting" stub scenarios A and B restore against; scripts/
# ci-continuation-acceptance.sh's scenario C spawns a second, rejecting stub
# instead (see REJECT_AGENT below).
LAST_PROMPT=""
SESSION_ID=""
while IFS= read -r line; do
  printf '%s\n' "$line" | sed -n 's/.*"method":"\([a-zA-Z_/]*\)".*/\1/p' >> "${FABLE_METHODS_LOG}"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":true}}}\n' "$id" ;;
    *'"method":"session/load"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      requested=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      if [[ -n "$requested" ]] && grep -Fxq -- "$requested" "${FABLE_ACP_REQUEST_LOG}.cursors"; then
        SESSION_ID="$requested"
        printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"unknown cursor"}}\n' "$id"
      fi ;;
    *'"method":"session/new"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      SESSION_ID="acp-$(openssl rand -hex 16)"
      printf '%s\n' "$SESSION_ID" >> "${FABLE_ACP_REQUEST_LOG}.cursors"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s"}}\n' "$id" "$SESSION_ID" ;;
    *'"method":"session/prompt"'*)
      LAST_PROMPT="$id"
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      requested=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      if [[ -z "$SESSION_ID" || "$requested" != "$SESSION_ID" ]]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"prompt cursor mismatch"}}\n' "$id"
        continue
      fi
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}\n' "$SESSION_ID"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$LAST_PROMPT" ;;
  esac
done
AGENT
chmod +x "${FAKE_AGENT}"

# FAKE_AGENT's rejecting twin: advertises `loadSession` (so the provider's strict
# restore genuinely tries) and then rejects every session/load with a
# JSON-RPC error — the exact shape that, before strict native restore
# existed, silently fell through to a fresh session/new. It still answers
# session/new (for the scenario's ORIGINAL open, before any kill) with a
# visibly different sessionId, so a restore that fell through despite strict
# mode would be caught by the cursor as well as by the method log. Used only
# by scenario C, on its own separate provider process.
REJECT_AGENT="${WORKDIR}/reject-agent.sh"
cat > "${REJECT_AGENT}" <<'AGENT'
#!/bin/bash
while IFS= read -r line; do
  printf '%s\n' "$line" | sed -n 's/.*"method":"\([a-zA-Z_/]*\)".*/\1/p' >> "${FABLE_METHODS_LOG}"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":true}}}\n' "$id" ;;
    *'"method":"session/load"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"load rejected"}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"a-brand-new-conversation"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
AGENT
chmod +x "${REJECT_AGENT}"

STATE_DIR="${WORKDIR}/provider-state"
mkdir -p "${STATE_DIR}" "${WORKDIR}/checkout"
cat > "${WORKDIR}/projects.json" <<EOF
{"version":1,"channels":{"${CHANNEL}":"${WORKDIR}/checkout"}}
EOF
RUNTIMES_JSON="$(python3 -c "
import json
print(json.dumps([{
  'instanceRef': 'claude-primary',
  'driver': 'claude-agent-acp',
  'runtime': 'claude',
  'agentCommand': 'bash',
  'agentArgs': ['${FAKE_AGENT}'],
  'defaultModel': 'default',
  'allowedModels': ['default'],
}]))
")"

METHODS_LOG_1="${WORKDIR}/methods-1.log"
: > "${METHODS_LOG_1}"
PROJECTS_FILE="${WORKDIR}/projects.json"
PROVIDER_LOG_1="${WORKDIR}/provider.log"

log "Starting beekeeper-session-provider (fake ACP adapter, no model)..."
BEEKEEPER_PRIVATE_KEY="${PROVIDER_KEY}" \
  BEEKEEPER_RELAY_URL="${RELAY_URL}" \
  BEEKEEPER_CSP_STATE_DIR="${STATE_DIR}" \
  BEEKEEPER_CSP_PROJECTS_FILE="${PROJECTS_FILE}" \
  BEEKEEPER_CSP_RUNTIMES="${RUNTIMES_JSON}" \
  FABLE_ACP_REQUEST_LOG="${ACP_REQUEST_LOG}" \
  FABLE_METHODS_LOG="${METHODS_LOG_1}" \
  RUST_LOG=info \
  "${PROVIDER_BIN}" > "${PROVIDER_LOG_1}" 2>&1 &
PROVIDER_PID=$!
PROVIDER_PIDS+=("${PROVIDER_PID}")

for _ in $(seq 1 30); do
  if ! kill -0 "${PROVIDER_PID}" 2>/dev/null; then
    err "provider process died during startup"
    cat "${WORKDIR}/provider.log" >&2
    exit 1
  fi
  # tracing's ANSI styling puts an escape code *between* "pubkey" and "="
  # (`\x1b[3mpubkey\x1b[0m\x1b[2m=\x1b[0m...`), so a raw grep for the
  # contiguous string never matches; strip codes first, same as the
  # extraction below.
  sed 's/\x1b\[[0-9;]*m//g' "${WORKDIR}/provider.log" 2>/dev/null | grep -q 'pubkey=' && break
  sleep 1
done
PROVIDER_PUBKEY="$(sed 's/\x1b\[[0-9;]*m//g' "${WORKDIR}/provider.log" | grep -o 'pubkey=[0-9a-f]\{64\}' | head -1 | cut -d= -f2)"
[[ -n "${PROVIDER_PUBKEY}" ]] || { err "could not read provider pubkey from its own startup log"; exit 1; }

# Wait for the provider's live catalog (44222) for this channel — proves it
# joined its subscribe/discover_channels pass, not just process liveness.
for _ in $(seq 1 30); do
  count="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44222 --channel "${CHANNEL}" \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')"
  [[ "${count}" -gt 0 ]] && break
  sleep 1
done
[[ "${count}" -gt 0 ]] || { err "provider never advertised a catalog for the channel"; cat "${WORKDIR}/provider.log" >&2; exit 1; }
ok "provider live: pubkey=${PROVIDER_PUBKEY}"

# ── Shared helpers for scenarios 6-8 (kill/restart composition) ─────────────
# Used only by the restart scenarios below; steps 1-5 above are unchanged.

# Read the exact persisted target's cursor before killing its provider.
saved_cursor() {
  python3 - "${WORKDIR}" "$1" "$2" "$3" <<'PY'
import pathlib
import sys
sys.path.insert(0, sys.argv[1])
import helpers
state = helpers.load_json_file(pathlib.Path(sys.argv[2]) / "state.json")
matches = [record for record in state["sessions"].values()
           if helpers.target_to_key({**record, "instanceId": sys.argv[4][:16]}) == sys.argv[3]]
assert len(matches) == 1, matches
assert matches[0]["resumeCursor"], matches[0]
print(matches[0]["resumeCursor"])
PY
}

# Ready alone does not establish an unclaimed crash window: claims are
# appended before the ready-to-claimed store write. Check both ledgers too.
has_start_claim() {
  python3 - "$1" "$2" <<'PY'
import json
import pathlib
import sys
for name in ("commands.jsonl", "operations.jsonl"):
    path = pathlib.Path(sys.argv[1]) / name
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                sys.exit(0)  # uncertainty must not pass as an unclaimed crash window
            if record.get("commandId") == sys.argv[2]:
                sys.exit(0)
sys.exit(1)
PY
}

# Number of kind:44222 catalog events stored for the channel right now — a
# monotonically increasing count, since 44222 is a regular (non-ephemeral,
# non-replaceable) kind, so a fresh publish after a restart always grows it.
channel_catalog_count() {
  BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44222 --channel "${CHANNEL}" \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'
}

# Wait (up to 60s) for a `kill -9`'d pid to actually be gone, so the restart
# that follows binds a genuinely new process rather than racing the old one's
# teardown.
wait_pid_exit() {
  local pid="$1"
  for _ in $(seq 1 120); do
    kill -0 "${pid}" 2>/dev/null || return 0
    sleep 0.5
  done
  err "pid ${pid} did not exit within 60s of kill -9"
  exit 1
}

# Spawn a beekeeper-session-provider generation with the given identity/state/ACP
# stub, append its pid to PROVIDER_PIDS (so cleanup always finds it), and set
# LAST_PID. Mirrors the initial provider1 spawn above, parameterized so
# scenarios A/B (same identity, new log file each restart) and scenario C (a
# second identity, its own stub, state dir and logs) can all use it.
spawn_provider() {
  local key="$1" state_dir="$2" projects_file="$3" runtimes_json="$4" \
        log_file="$5" methods_log="$6" acp_log="$7"
  BEEKEEPER_PRIVATE_KEY="${key}" \
    BEEKEEPER_RELAY_URL="${RELAY_URL}" \
    BEEKEEPER_CSP_STATE_DIR="${state_dir}" \
    BEEKEEPER_CSP_PROJECTS_FILE="${projects_file}" \
    BEEKEEPER_CSP_RUNTIMES="${runtimes_json}" \
    FABLE_ACP_REQUEST_LOG="${acp_log}" \
    FABLE_METHODS_LOG="${methods_log}" \
    RUST_LOG=info \
    "${PROVIDER_BIN}" > "${log_file}" 2>&1 &
  LAST_PID=$!
  PROVIDER_PIDS+=("${LAST_PID}")
}

# Wait for a just-spawned provider's own startup log to print its pubkey
# (proof of process liveness past `initialize`), and echo that pubkey.
wait_provider_pubkey() {
  local pid="$1" log="$2"
  local i
  for i in $(seq 1 30); do
    if ! kill -0 "${pid}" 2>/dev/null; then
      err "provider process died during startup; log: ${log}"
      cat "${log}" >&2
      exit 1
    fi
    # See the identical comment on the initial provider1 wait above: strip
    # ANSI codes before matching, since tracing's styling splits "pubkey="
    # with an escape sequence.
    sed 's/\x1b\[[0-9;]*m//g' "${log}" 2>/dev/null | grep -q 'pubkey=' && break
    sleep 1
  done
  local pubkey
  pubkey="$(sed 's/\x1b\[[0-9;]*m//g' "${log}" | grep -o 'pubkey=[0-9a-f]\{64\}' | head -1 | cut -d= -f2)"
  [[ -n "${pubkey}" ]] || { err "could not read provider pubkey from ${log}"; exit 1; }
  echo "${pubkey}"
}

# Wait for the channel-wide 44222 catalog count to exceed `min_count` — proof
# that a just-started provider completed its own discover/subscribe pass, not
# just that its process is alive. Echoes the new count. Only valid for a
# provider identity's FIRST-EVER spawn: `csp::catalog` skips re-advertising
# an unchanged catalog, so this never fires after a restart (see
# `wait_provider_relay_connected` below, which is what restarts wait on).
wait_catalog_above() {
  local min_count="$1"
  local count=0
  local i
  for i in $(seq 1 30); do
    count="$(channel_catalog_count)"
    [[ "${count}" -gt "${min_count}" ]] && { echo "${count}"; return 0; }
    sleep 1
  done
  err "channel catalog count did not exceed ${min_count} within 30s (stayed at ${count})"
  exit 1
}

# Wait for a (re)started provider's own log to show it reached the network —
# proof it is ready to observe the 46008 that scenarios A/B/C post right
# after this returns. Used for every RESTART (unlike `wait_catalog_above`,
# which only ever fires on a provider identity's first-ever spawn: a restart
# publishes no new 44222, because `csp::catalog` correctly skips
# re-advertising a catalog that has not changed — confirmed by direct
# comparison of a restarted provider's log against its first spawn's, which
# has exactly one `csp::catalog: advertised provider catalog` line, never
# repeated after a kill -9/respawn). "witnessed relay identity" is the last
# startup milestone that is plain text end to end (grep-safe without ANSI
# stripping) and it precedes this provider observing or acting on any event.
wait_provider_relay_connected() {
  local pid="$1" log="$2"
  local i
  for i in $(seq 1 30); do
    if ! kill -0 "${pid}" 2>/dev/null; then
      err "provider process died during startup; log: ${log}"
      cat "${log}" >&2
      exit 1
    fi
    grep -q 'witnessed relay identity' "${log}" 2>/dev/null && return 0
    sleep 1
  done
  err "${log} never logged 'witnessed relay identity' within 30s"
  cat "${log}" >&2
  exit 1
}

# Everything docs/CI_CONTINUATION_RECOVERY_SPEC.md §5 items 2-3 require of a
# restart scenario's outcome, once the 46008 has been posted and a
# turn_started is expected: exactly one turn_started receipt for
# `command_id`; the 44223 published after the restart still names the
# ORIGINAL `target_key` (no generation bump); a session_restored_native
# transcript row precedes the delivered user_prompt in the transcript's own
# sequence; the stub's method log shows session/load and never session/new
# after `methods_mark` (the line count captured right before the kill); the
# ACP load and prompt name the original persisted cursor, prompt bytes equal
# the exact target's transcript materialization; and the restarted
# provider's own log never shows NO_LIVE_EXECUTION. Prints the turn_started
# receipt event id on success (for the caller's PASS line).
verify_native_restore_delivery() {
  local label="$1" command_id="$2" target_key="$3" \
        methods_log="$4" methods_mark="$5" acp_log="$6" baseline_prompts="$7" \
        provider_log="$8" restart_epoch="$9" expected_cursor="${10}" provider_pubkey="${11}"

  if grep -q 'NO_LIVE_EXECUTION' "${provider_log}"; then
    err "${label}: ${provider_log} shows NO_LIVE_EXECUTION — the restore did not reopen the \
generation before delivery was attempted"
    exit 1
  fi

  local post_restart_methods
  post_restart_methods="$(tail -n +"$((methods_mark + 1))" "${methods_log}")"
  if ! grep -q '^session/load$' <<<"${post_restart_methods}"; then
    err "${label}: ${methods_log} shows no session/load after the restart (mark line ${methods_mark})"
    exit 1
  fi
  if grep -q '^session/new$' <<<"${post_restart_methods}"; then
    err "${label}: ${methods_log} shows session/new after the restart — the restore fell through \
to a fresh conversation. Methods observed since restart:"
    err "${post_restart_methods}"
    exit 1
  fi

  BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}" \
    > "${WORKDIR}/${label}-receipts.json"
  BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44223 --channel "${CHANNEL}" \
    > "${WORKDIR}/${label}-metadata.json"
  BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44225 --channel "${CHANNEL}" \
    > "${WORKDIR}/${label}-transcript.json"

  python3 - "${WORKDIR}" "${label}" "${command_id}" "${target_key}" "${restart_epoch}" \
    "${acp_log}" "${baseline_prompts}" "${expected_cursor}" "${provider_pubkey}" <<'PY'
import json
import pathlib
import sys

sys.path.insert(0, sys.argv[1])
import helpers  # noqa: E402

workdir = pathlib.Path(sys.argv[1])
label, command_id, target_key, restart_epoch_raw, acp_log_path, baseline_raw, expected_cursor, provider = sys.argv[2:]
restart_epoch = int(restart_epoch_raw)
baseline_prompts = int(baseline_raw)

receipts = helpers.load_json_file(workdir / f"{label}-receipts.json")
started = [
    event
    for event in receipts
    if (content := json.loads(event["content"])).get("commandId") == command_id
    and content.get("status") == "turn_started"
]
assert len(started) == 1, f"expected exactly one turn_started receipt, found {len(started)}: {started!r}"
assert started[0]["pubkey"] == provider, started
assert helpers.target_to_key(json.loads(started[0]["content"])["session"]) == target_key, started

metadata = helpers.load_json_file(workdir / f"{label}-metadata.json")
matching_metadata = [
    event
    for event in metadata
    if event["pubkey"] == provider and ["cs-target", target_key] in event.get("tags", [])
    and helpers.target_to_key(json.loads(event["content"])["session"]) == target_key
    and event.get("created_at", 0) >= restart_epoch
]
assert matching_metadata, (
    f"no 44223 published at/after the restart (epoch {restart_epoch}) named cs-target {target_key}"
)

transcript = helpers.load_json_file(workdir / f"{label}-transcript.json")
restored_seq = None
prompt_seq = None
materialized = None
for event in transcript:
    try:
        envelope = json.loads(event["content"])
    except (TypeError, json.JSONDecodeError):
        continue
    if event["pubkey"] != provider or helpers.target_to_key(envelope["session"]) != target_key:
        continue
    if event.get("created_at", 0) < restart_epoch:
        continue
    item = envelope.get("item", {})
    if item.get("kind") == "status" and item.get("status") == "session_restored_native":
        if restored_seq is None or envelope["eventSeq"] > restored_seq:
            restored_seq = envelope["eventSeq"]
    if item.get("kind") == "user_prompt" and item.get("commandId") == command_id:
        prompt_seq = envelope["eventSeq"]
        materialized = item["content"]
assert restored_seq is not None, "no session_restored_native transcript item found"
assert prompt_seq is not None, f"no materialized user_prompt transcript item for {command_id}"
assert restored_seq < prompt_seq, (
    f"session_restored_native (seq {restored_seq}) did not precede the delivered prompt (seq {prompt_seq})"
)

with open(acp_log_path, encoding="utf-8") as source:
    requests = [json.loads(line) for line in source if line.strip()]
prompt_requests = [request for request in requests if request.get("method") == "session/prompt"]
assert len(prompt_requests) == baseline_prompts + 1, (
    f"expected exactly one new ACP session/prompt after the restart, saw "
    f"{len(prompt_requests) - baseline_prompts}"
)
delivered_request = prompt_requests[baseline_prompts]
assert delivered_request["params"]["sessionId"] == expected_cursor, delivered_request
loads = [request for request in requests if request.get("method") == "session/load"
         and request["params"].get("sessionId") == expected_cursor]
assert len(loads) == 1, f"expected one restore of the original cursor {expected_cursor}: {loads!r}"
assert delivered_request["params"]["prompt"] == [{"type": "text", "text": materialized}], delivered_request
delivered = delivered_request["params"]["prompt"][0]["text"]
assert delivered == materialized, "ACP prompt bytes differ from the persisted transcript materialization"

with open(workdir / f"{label}-started-id.txt", "w", encoding="utf-8") as sink:
    sink.write(started[0]["id"])
PY

  cat "${WORKDIR}/${label}-started-id.txt"
}

# ── Step 1: real session create ──────────────────────────────────────────────
log "bee sessions create --wait ..."
CREATE_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee sessions create --channel "${CHANNEL}" \
  --provider-instance claude-primary --provider-authority "${PROVIDER_PUBKEY}" \
  --project "${PROJECT}" \
  --brief - --wait --timeout-secs 30 <<< "start the ci continuation acceptance run")"
TARGET_KEY="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['target'])" "${CREATE_JSON}")"
[[ "${TARGET_KEY}" != "None" && -n "${TARGET_KEY}" ]] || { err "sessions create did not confirm a target: ${CREATE_JSON}"; exit 1; }
pass 1 "sessions create confirmed target ${TARGET_KEY}"

BASELINE_PROMPTS="$(python3 - "${ACP_REQUEST_LOG}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    requests = [json.loads(line) for line in source if line.strip()]
assert any(request.get("method") == "session/new" for request in requests), requests
print(sum(request.get("method") == "session/prompt" for request in requests))
PY
)"

# ── Step 2: register the CI continuation ─────────────────────────────────────
log "bee ci continue ..."
COMMIT="$(openssl rand -hex 20)"
CHECK_NAME="main-validation"
RUN_ID="136"
ATTEMPT="1"
PHASE="build"
CONTINUATION_TEXT="CI passed; open the PR."
EVIDENCE_URL="https://ci.example/runs/${RUN_ID}"
RESULT_SUMMARY="all required jobs passed"
CONTINUE_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continue \
  --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY}" --target "${TARGET_KEY}" \
  --project "${PROJECT}" --repository "${REPO}" \
  --commit "${COMMIT}" --check "${CHECK_NAME}" --run "${RUN_ID}" --attempt "${ATTEMPT}" \
  --workflow "${WORKFLOW_ID}" --phase "${PHASE}" \
  --continuation "${CONTINUATION_TEXT}" \
  --expires-in 86400 --ack-timeout 30)"
COMMAND_ID="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['commandId'])" "${CONTINUE_JSON}")"
printf '%s\n' "${CONTINUE_JSON}" > "${WORKDIR}/continue-output.json"
BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44220 --channel "${CHANNEL}" \
  > "${WORKDIR}/registration-events.json"
pass 2 "ci continue registered ${COMMAND_ID} (exit 0 = continuation_registered; a refusal or ack timeout would have been a nonzero exit under set -e)"

# ── Step 3: the real workflow webhook produces a genuine 46008 ─────────────
log "POSTing the workflow webhook to produce a relay-signed CI result..."
WEBHOOK_BODY="$(python3 -c "
import json
print(json.dumps({
  'commit': '${COMMIT}', 'run': '${RUN_ID}', 'attempt': ${ATTEMPT}, 'conclusion': 'success',
  'evidence_url': '${EVIDENCE_URL}', 'summary': '${RESULT_SUMMARY}',
}))
")"
HOOK_STATUS="$(curl -s -o "${WORKDIR}/hook-response.json" -w '%{http_code}' \
  -X POST "${RELAY_HTTP}/hooks/${WORKFLOW_ID}" \
  -H "x-webhook-secret: ${WEBHOOK_SECRET}" -H 'Content-Type: application/json' \
  -d "${WEBHOOK_BODY}")"
[[ "${HOOK_STATUS}" == "202" ]] || { err "webhook POST did not accept (status ${HOOK_STATUS}): $(cat "${WORKDIR}/hook-response.json")"; exit 1; }

for _ in $(seq 1 20); do
  # 46008 carries no h tag (workflow_ci_result.rs) — query without --channel.
  RESULT_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 46008)"
  RESULT_COUNT="$(python3 -c "
import json,sys
events=json.loads(sys.argv[1])
matching=[e for e in events if '${COMMIT}' in e.get('content','')]
print(len(matching))
" "${RESULT_JSON}")"
  [[ "${RESULT_COUNT}" -gt 0 ]] && break
  sleep 1
done
[[ "${RESULT_COUNT}" -gt 0 ]] || { err "no relay-signed 46008 was stored for commit ${COMMIT}"; exit 1; }
printf '%s\n' "${RESULT_JSON}" > "${WORKDIR}/result-events.json"
RELAY_SELF="$(curl -s -H 'Accept: application/nostr+json' "${RELAY_HTTP}/" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["self"])')"
RESULT_SIGNER="$(python3 -c "
import json,sys
events=json.loads(sys.argv[1])
matching=[e for e in events if '${COMMIT}' in e.get('content','')]
print(matching[0]['pubkey'])
" "${RESULT_JSON}")"
[[ "${RESULT_SIGNER}" == "${RELAY_SELF}" ]] || {
  err "46008 signer ${RESULT_SIGNER} did not match NIP-11 relay self ${RELAY_SELF}"
  exit 1
}
pass 3 "webhook produced a real 46008 signed by ${RESULT_SIGNER} (the relay's own identity, not a hand-signed event)"

# ── Step 4: the provider starts exactly one continuation turn ───────────────
log "waiting for the provider to admit and start the continuation turn..."
for _ in $(seq 1 30); do
  RECEIPTS_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}")"
  STARTED="$(python3 -c "
import json,sys
events=json.loads(sys.argv[1])
for e in events:
    c=json.loads(e['content'])
    if c.get('commandId')=='${COMMAND_ID}' and c.get('status')=='turn_started':
        print(e['id']); break
else:
    print('')
" "${RECEIPTS_JSON}")"
  [[ -n "${STARTED}" ]] && break
  sleep 1
done
[[ -n "${STARTED}" ]] || { err "no turn_started receipt named commandId ${COMMAND_ID} within 30s"; cat "${WORKDIR}/provider.log" >&2; exit 1; }

AFTER_ONE="$(python3 - "${ACP_REQUEST_LOG}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    print(sum(json.loads(line).get("method") == "session/prompt" for line in source if line.strip()))
PY
)"
[[ "$((AFTER_ONE - BASELINE_PROMPTS))" -eq 1 ]] || {
  err "expected exactly one new ACP session/prompt after admission, saw $((AFTER_ONE - BASELINE_PROMPTS))"
  exit 1
}

BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44225 --channel "${CHANNEL}" \
  > "${WORKDIR}/transcript-events.json"

STATUS_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continuation status \
  --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY}" --target "${TARGET_KEY}" \
  --command-id "${COMMAND_ID}")"
printf '%s\n' "${STATUS_JSON}" > "${WORKDIR}/first-status.json"

python3 - "${WORKDIR}" "${CHANNEL}" "${COMMAND_ID}" "${TARGET_KEY}" \
  "${OWNER_HEX}" "${PROVIDER_PUBKEY}" "${RELAY_SELF}" "${PROJECT}" "${REPO}" \
  "${COMMIT}" "${CHECK_NAME}" "${RUN_ID}" "${ATTEMPT}" "${WORKFLOW_ID}" \
  "${PHASE}" "${CONTINUATION_TEXT}" "${EVIDENCE_URL}" "${RESULT_SUMMARY}" \
  "${BASELINE_PROMPTS}" <<'PY'
import hashlib
import json
import pathlib
import sys
import time

(
    workdir_raw,
    channel,
    command_id,
    target_key,
    owner,
    provider,
    relay_self,
    project,
    repository,
    commit,
    check,
    run,
    attempt_raw,
    workflow,
    phase,
    continuation,
    evidence_url,
    result_summary,
    baseline_prompts_raw,
) = sys.argv[1:]
workdir = pathlib.Path(workdir_raw)
attempt = int(attempt_raw)
baseline_prompts = int(baseline_prompts_raw)

def load(name):
    with (workdir / name).open(encoding="utf-8") as source:
        return json.load(source)

def exactly_one(items, label):
    assert len(items) == 1, f"expected exactly one {label}, found {len(items)}: {items!r}"
    return items[0]

def target_to_key(target):
    fields = [target["driver"], target["instanceId"], target["sessionId"], str(target["generation"])]
    return "coding-session/v1|" + "".join(f"{len(field.encode('utf-8'))}:{field}" for field in fields)

expected_identity = {
    "project": project,
    "repository": repository,
    "commit": commit,
    "check": check,
    "run": run,
    "attempt": attempt,
    "workflow": workflow,
    "phase": phase,
}
identity_bytes = json.dumps(expected_identity, separators=(",", ":")).encode()
operation_id = hashlib.sha256(identity_bytes).hexdigest()

ack = load("continue-output.json")
assert ack["commandId"] == command_id, ack
assert ack["target"] == target_key, ack
assert ack["provider"] == provider, ack
assert ack["operationId"] == operation_id, ack
assert isinstance(ack["expiresAt"], int) and ack["expiresAt"] > int(time.time()), ack

registration_events = load("registration-events.json")
registration_event = exactly_one(
    [event for event in registration_events if event.get("id") == ack["registeredEventId"]],
    "signed registration event matching registeredEventId",
)
assert registration_event["kind"] == 44220, registration_event
assert registration_event["pubkey"] == owner, registration_event
registration = json.loads(registration_event["content"])
assert set(registration) == {"schema", "commandId", "target", "action"}, registration
assert registration["schema"] == "buzz-coding-session-command/v1", registration
assert registration["commandId"] == command_id, registration
assert set(registration["target"]) == {"driver", "instanceId", "sessionId", "generation"}, registration
assert target_to_key(registration["target"]) == target_key, registration
assert registration["action"] == {
    "type": "thread.turn.continue_on_ci",
    "identity": expected_identity,
    "continuation": continuation,
    "expiresAt": ack["expiresAt"],
}, registration
assert registration_event["tags"] == [
    ["h", channel],
    ["cs-v", "csc1-1"],
    ["cs-target", target_key],
], registration_event

receipt_events = load("first-status.json")
assert receipt_events["commandId"] == command_id, receipt_events
assert receipt_events["target"] == target_key, receipt_events
assert receipt_events["provider"] == provider, receipt_events
assert receipt_events["stage"] == "started", receipt_events
assert ack["receiptEventId"] in receipt_events["receiptEventIds"], receipt_events

result_events = load("result-events.json")
decoded_results = []
for event in result_events:
    try:
        content = json.loads(event["content"])
    except (KeyError, TypeError, json.JSONDecodeError):
        continue
    if content.get("identity") == expected_identity:
        decoded_results.append((event, content))
result_event, result = exactly_one(decoded_results, "relay result for the complete eight-field identity")
assert result_event["kind"] == 46008, result_event
assert result_event["pubkey"] == relay_self, result_event
assert result == {
    "schema": "buzz-ci-result/v1",
    "identity": expected_identity,
    "conclusion": "success",
    "evidence_url": evidence_url,
    "summary": result_summary,
}, result
assert sorted(result_event["tags"]) == sorted([
    ["d", operation_id],
    ["a", repository],
    ["project", project],
    ["workflow", workflow],
    ["schema", "buzz-ci-result/v1"],
]), result_event

transcript_events = load("transcript-events.json")
transcript_prompts = []
for event in transcript_events:
    try:
        item = json.loads(event["content"])["item"]
    except (KeyError, TypeError, json.JSONDecodeError):
        continue
    if item.get("commandId") == command_id and item.get("kind") == "user_prompt":
        transcript_prompts.append(item["content"])
materialized = exactly_one(transcript_prompts, "materialized user_prompt transcript item")

with (workdir / "acp-requests.jsonl").open(encoding="utf-8") as source:
    requests = [json.loads(line) for line in source if line.strip()]
prompt_requests = [request for request in requests if request.get("method") == "session/prompt"]
assert len(prompt_requests) == baseline_prompts + 1, prompt_requests
delivered_request = prompt_requests[baseline_prompts]
assert delivered_request["method"] == "session/prompt", delivered_request
state = load("provider-state/state.json")
original = exactly_one([record for record in state["sessions"].values()
    if target_to_key({**record, "instanceId": provider[:16]}) == target_key], "original target")
assert delivered_request["params"]["sessionId"] == original["resumeCursor"], delivered_request
delivered_blocks = delivered_request["params"]["prompt"]
assert delivered_blocks == [{"type": "text", "text": materialized}], delivered_blocks
delivered = delivered_blocks[0]["text"]
assert delivered == materialized, "ACP prompt bytes differ from the persisted transcript materialization"

materialized_json = json.loads(materialized)
assert list(materialized_json) == ["type", "operationId", "registration", "result", "continuation"], materialized_json
assert materialized_json["type"] == "ci_result", materialized_json
assert materialized_json["operationId"] == operation_id, materialized_json
assert materialized_json["registration"] == {
    "commandId": command_id,
    "signer": registration_event["pubkey"],
    "registeredAt": materialized_json["registration"]["registeredAt"],
    "expiresAt": registration["action"]["expiresAt"],
}, materialized_json
assert isinstance(materialized_json["registration"]["registeredAt"], int), materialized_json
assert registration_event["created_at"] <= materialized_json["registration"]["registeredAt"] <= int(time.time()), materialized_json
assert materialized_json["result"] == {
    "eventId": result_event["id"],
    "signer": result_event["pubkey"],
    "observedAt": materialized_json["result"]["observedAt"],
    "identity": result["identity"],
    "conclusion": result["conclusion"],
    "evidenceUrl": result["evidence_url"],
    "summary": result["summary"],
}, materialized_json
assert isinstance(materialized_json["result"]["observedAt"], int), materialized_json
assert result_event["created_at"] <= materialized_json["result"]["observedAt"] <= int(time.time()), materialized_json
assert materialized_json["continuation"] == continuation, materialized_json
print("raw ACP prompt, transcript bytes, signed registration, relay result, and started status verified")
PY
pass 4 "provider started exactly one turn (turnId receipt ${STARTED:0:16}...); raw ACP text equals the transcript and every §1c identity/correlation field matches the signed events"

# ── Step 5: duplicate 46008 and a second commandId both spend zero turns ───
log "POSTing an identical duplicate webhook..."
DUPLICATE_HOOK_STATUS="$(curl -s -o "${WORKDIR}/duplicate-hook-response.json" -w '%{http_code}' \
  -X POST "${RELAY_HTTP}/hooks/${WORKFLOW_ID}" \
  -H "x-webhook-secret: ${WEBHOOK_SECRET}" -H 'Content-Type: application/json' \
  -d "${WEBHOOK_BODY}")"
[[ "${DUPLICATE_HOOK_STATUS}" == "202" ]] || {
  err "duplicate webhook POST did not accept (status ${DUPLICATE_HOOK_STATUS}): $(cat "${WORKDIR}/duplicate-hook-response.json")"
  exit 1
}
sleep 2

log "registering a second commandId for the same identity+target..."
set +e
SECOND_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continue \
  --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY}" --target "${TARGET_KEY}" \
  --project "${PROJECT}" --repository "${REPO}" \
  --commit "${COMMIT}" --check "${CHECK_NAME}" --run "${RUN_ID}" --attempt "${ATTEMPT}" \
  --workflow "${WORKFLOW_ID}" --phase "${PHASE}" \
  --continuation "a different continuation text, same identity and target" \
  --expires-in 86400 --ack-timeout 30 2> "${WORKDIR}/second-continue.stderr")"
SECOND_EXIT=$?
set -e
[[ "${SECOND_EXIT}" -eq 0 || "${SECOND_EXIT}" -eq 1 ]] || {
  err "second registration exited ${SECOND_EXIT}; expected registration or authenticated refusal"
  cat "${WORKDIR}/second-continue.stderr" >&2
  exit 1
}
printf '%s\n' "${SECOND_JSON}" > "${WORKDIR}/second-continue-output.json"
SECOND_COMMAND_ID="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['commandId'])" "${SECOND_JSON}")"
[[ "${SECOND_COMMAND_ID}" != "${COMMAND_ID}" ]] || { err "second registration reused the first commandId — test setup bug"; exit 1; }
if [[ "${SECOND_EXIT}" -eq 1 ]]; then
  python3 - "${WORKDIR}/second-continue-output.json" "${SECOND_COMMAND_ID}" \
    "${TARGET_KEY}" "${PROVIDER_PUBKEY}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    output = json.load(source)
assert output["commandId"] == sys.argv[2], output
assert output["target"] == sys.argv[3], output
assert output["provider"] == sys.argv[4], output
assert output["code"] == "DUPLICATE_OPERATION", output
PY
fi

SECOND_STATUS_JSON=""
for _ in $(seq 1 30); do
  SECOND_STATUS_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continuation status \
    --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY}" --target "${TARGET_KEY}" \
    --command-id "${SECOND_COMMAND_ID}")"
  SECOND_STATUS="$(python3 -c "import json,sys; d=json.loads(sys.argv[1]); print(f\"{d['stage']}:{d.get('refusalCode') or ''}\")" "${SECOND_STATUS_JSON}")"
  [[ "${SECOND_STATUS}" == "refused:DUPLICATE_OPERATION" ]] && break
  sleep 1
done
[[ "${SECOND_STATUS}" == "refused:DUPLICATE_OPERATION" ]] || {
  err "second registration did not reach authenticated DUPLICATE_OPERATION refusal: ${SECOND_STATUS_JSON}"
  exit 1
}
printf '%s\n' "${SECOND_STATUS_JSON}" > "${WORKDIR}/second-status.json"
BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}" \
  > "${WORKDIR}/second-receipt-events.json"

AFTER_DUPLICATES="$(python3 - "${ACP_REQUEST_LOG}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    print(sum(json.loads(line).get("method") == "session/prompt" for line in source if line.strip()))
PY
)"
[[ "${AFTER_DUPLICATES}" -eq "${AFTER_ONE}" ]] || {
  err "duplicate result and/or second commandId spent an extra turn: ACP prompt count went from ${AFTER_ONE} to ${AFTER_DUPLICATES}"
  exit 1
}
python3 - "${WORKDIR}/second-status.json" "${WORKDIR}/second-receipt-events.json" \
  "${SECOND_COMMAND_ID}" "${TARGET_KEY}" "${PROVIDER_PUBKEY}" <<'PY'
import json
import sys

status_path, receipts_path, command_id, target_key, provider = sys.argv[1:]
with open(status_path, encoding="utf-8") as source:
    status = json.load(source)
with open(receipts_path, encoding="utf-8") as source:
    events = json.load(source)

assert status["commandId"] == command_id, status
assert status["target"] == target_key, status
assert status["provider"] == provider, status
assert status["stage"] == "refused", status
assert status["refusalCode"] == "DUPLICATE_OPERATION", status

def target_to_key(target):
    fields = [target["driver"], target["instanceId"], target["sessionId"], str(target["generation"])]
    return "coding-session/v1|" + "".join(f"{len(field.encode('utf-8'))}:{field}" for field in fields)

matching = []
for event in events:
    try:
        receipt = json.loads(event["content"])
    except (KeyError, TypeError, json.JSONDecodeError):
        continue
    if receipt.get("commandId") != command_id or receipt.get("status") != "turn_refused":
        continue
    if receipt.get("error", {}).get("code") != "DUPLICATE_OPERATION":
        continue
    assert event["pubkey"] == provider, event
    assert target_to_key(receipt["session"]) == target_key, receipt
    matching.append(event)
assert matching, f"no provider-signed DUPLICATE_OPERATION receipt for {command_id}"
assert all(event["id"] in status["receiptEventIds"] for event in matching), (matching, status)
PY
pass 5 "duplicate 46008 and second commandId ${SECOND_COMMAND_ID} produced an authenticated DUPLICATE_OPERATION refusal and no second ACP prompt"

if [[ "${CI_CONTINUATION_SCENARIOS:-all}" != "basic" ]]; then

# ── Shared helpers for scenarios 6-8 ─────────────────────────────────────────

# Portable substitute for bash 4's `mapfile`/`readarray`: macOS ships bash
# 3.2 as /bin/bash (and this script's `#!/usr/bin/env bash` resolves to it
# even with hermit active), and 3.2 has neither builtin. Splits `text`
# (already captured via command substitution, one field per line) into the
# indexed array named by `array_name`. `read -d ''` reads to EOF rather than
# a NUL byte (none of this script's values contain one) and returns nonzero
# for hitting EOF instead of the delimiter — expected, not a failure, hence
# `|| true`.
split_lines_into() {
  local array_name="$1" text="$2"
  IFS=$'\n' read -r -d '' -a "${array_name}" <<< "${text}"$'\n' || true
}

# Create a fresh session (its own target/generation, isolated from steps
# 1-5) against `provider_pubkey` and register a CI continuation on it, using
# the same project/repository/workflow fixtures steps 1-5 already
# established. Prints four lines: target key, commandId, the random commit
# this continuation is waiting on, and the channel-wide ACP session/prompt
# count at registration time (the baseline for "exactly one new prompt").
register_fresh_continuation() {
  local provider_pubkey="$1" acp_log="$2" brief="$3"
  local create_json target_key baseline commit continue_json command_id
  create_json="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee sessions create --channel "${CHANNEL}" \
    --provider-instance claude-primary --provider-authority "${provider_pubkey}" \
    --project "${PROJECT}" \
    --brief - --wait --timeout-secs 30 <<< "${brief}")"
  target_key="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['target'])" "${create_json}")"
  [[ "${target_key}" != "None" && -n "${target_key}" ]] \
    || { err "register_fresh_continuation: sessions create did not confirm a target: ${create_json}"; exit 1; }

  baseline="$(python3 - "${acp_log}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    print(sum(json.loads(line).get("method") == "session/prompt" for line in source if line.strip()))
PY
)"

  commit="$(openssl rand -hex 20)"
  continue_json="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continue \
    --channel "${CHANNEL}" --provider "${provider_pubkey}" --target "${target_key}" \
    --project "${PROJECT}" --repository "${REPO}" \
    --commit "${commit}" --check "${CHECK_NAME}" --run "${RUN_ID}" --attempt "${ATTEMPT}" \
    --workflow "${WORKFLOW_ID}" --phase "${PHASE}" \
    --continuation "${CONTINUATION_TEXT}" \
    --expires-in 86400 --ack-timeout 30)"
  command_id="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['commandId'])" "${continue_json}")"

  printf '%s\n%s\n%s\n%s\n' "${target_key}" "${command_id}" "${commit}" "${baseline}"
}

# The record's lifecycle-state `type` in the durable CI-continuation store
# (`ci-continuations.json`) for `command_id`, or "missing" if it has no
# record — read directly off disk so it works whether or not the provider
# whose state dir this is happens to be alive right now.
ci_continuation_store_state() {
  local state_dir="$1" command_id="$2"
  python3 -c "
import json
with open('${state_dir}/ci-continuations.json', encoding='utf-8') as source:
    data = json.load(source)
record = next((r for r in data.get('registrations', []) if r['commandId'] == '${command_id}'), None)
print(record['state']['type'] if record else 'missing')
"
}

# Poll `ci_continuation_store_state` for `command_id` to become "ready",
# entirely inside ONE python process (rather than one `python3 -c` spawn per
# bash-loop iteration, which costs ~20ms of interpreter startup alone —
# measured, and enough on its own to blow past the ready→claimed window this
# check exists to catch: with a fake ACP adapter and no network model call,
# scenario B's admission-after-ready transition can complete before a
# once-per-100ms, subprocess-per-check bash loop ever samples it). Prints the
# LAST state observed before either seeing "ready" or the deadline — "ready"
# on success, "missing"/"waiting"/"claimed" otherwise (the retry-vs-proceed
# decision the caller makes on it).
wait_for_ready_tight() {
  local state_dir="$1" command_id="$2" deadline_secs="$3"
  python3 -c "
import json
import time

path = '${state_dir}/ci-continuations.json'
command_id = '${command_id}'
deadline = time.time() + ${deadline_secs}
last = 'missing'
while time.time() < deadline:
    try:
        with open(path, encoding='utf-8') as source:
            data = json.load(source)
    except (FileNotFoundError, json.JSONDecodeError):
        continue
    record = next((r for r in data.get('registrations', []) if r['commandId'] == command_id), None)
    last = record['state']['type'] if record else 'missing'
    if last == 'ready':
        break
print(last)
"
}

# POST the workflow's real webhook for `commit`, producing a genuine
# relay-signed 46008 — the same production path step 3 exercises, reused here
# for every scenario's result.
post_ci_result_webhook() {
  local commit="$1"
  local body status response_file
  response_file="${WORKDIR}/hook-response-${commit}.json"
  body="$(python3 -c "
import json
print(json.dumps({
  'commit': '${commit}', 'run': '${RUN_ID}', 'attempt': ${ATTEMPT}, 'conclusion': 'success',
  'evidence_url': '${EVIDENCE_URL}', 'summary': '${RESULT_SUMMARY}',
}))
")"
  status="$(curl -s -o "${response_file}" -w '%{http_code}' \
    -X POST "${RELAY_HTTP}/hooks/${WORKFLOW_ID}" \
    -H "x-webhook-secret: ${WEBHOOK_SECRET}" -H 'Content-Type: application/json' \
    -d "${body}")"
  [[ "${status}" == "202" ]] \
    || { err "webhook POST for commit ${commit} did not accept (status ${status}): $(cat "${response_file}")"; exit 1; }
}

# ── Step 6: Scenario A — waiting → kill -9 → restart → result ───────────────
log "Scenario A: register a continuation, kill -9 the provider while it is still 'waiting', restart it, then deliver the result..."
split_lines_into SCEN_A "$(register_fresh_continuation "${PROVIDER_PUBKEY}" "${ACP_REQUEST_LOG}" "scenario A: waiting-kill-restart")"
TARGET_KEY_A="${SCEN_A[0]}"; COMMAND_ID_A="${SCEN_A[1]}"; COMMIT_A="${SCEN_A[2]}"; BASELINE_A="${SCEN_A[3]}"

WAITING_STATE_A="$(ci_continuation_store_state "${STATE_DIR}" "${COMMAND_ID_A}")"
[[ "${WAITING_STATE_A}" == "waiting" ]] \
  || { err "scenario A: expected ${COMMAND_ID_A} to be 'waiting' before the kill, found '${WAITING_STATE_A}'"; exit 1; }

CURSOR_A="$(saved_cursor "${STATE_DIR}" "${TARGET_KEY_A}" "${PROVIDER_PUBKEY}")"
METHODS_MARK_A="$(wc -l < "${METHODS_LOG_1}" | tr -d ' ')"
log "Scenario A: kill -9 provider pid ${PROVIDER_PID}..."
kill -9 "${PROVIDER_PID}"
wait_pid_exit "${PROVIDER_PID}"

PROVIDER_LOG_A="${WORKDIR}/provider-restart-a.log"
RESTART_EPOCH_A="$(date +%s)"
spawn_provider "${PROVIDER_KEY}" "${STATE_DIR}" "${PROJECTS_FILE}" "${RUNTIMES_JSON}" \
  "${PROVIDER_LOG_A}" "${METHODS_LOG_1}" "${ACP_REQUEST_LOG}"
PROVIDER_PID="${LAST_PID}"
RESTARTED_PUBKEY_A="$(wait_provider_pubkey "${PROVIDER_PID}" "${PROVIDER_LOG_A}")"
[[ "${RESTARTED_PUBKEY_A}" == "${PROVIDER_PUBKEY}" ]] \
  || { err "scenario A: restarted provider pubkey ${RESTARTED_PUBKEY_A} != original ${PROVIDER_PUBKEY}"; exit 1; }
wait_provider_relay_connected "${PROVIDER_PID}" "${PROVIDER_LOG_A}"
ok "Scenario A: provider restarted (pid ${PROVIDER_PID}) over the same state dir ${STATE_DIR}"

log "Scenario A: posting the 46008 for commit ${COMMIT_A}..."
post_ci_result_webhook "${COMMIT_A}"

STARTED_A=""
for _ in $(seq 1 30); do
  STARTED_A="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}" \
    | python3 -c "
import json, sys
events = json.load(sys.stdin)
for e in events:
    c = json.loads(e['content'])
    if c.get('commandId') == '${COMMAND_ID_A}' and c.get('status') == 'turn_started':
        print(e['id']); break
else:
    print('')
")"
  [[ -n "${STARTED_A}" ]] && break
  sleep 1
done
[[ -n "${STARTED_A}" ]] \
  || { err "scenario A: no turn_started receipt for ${COMMAND_ID_A} within 30s"; cat "${PROVIDER_LOG_A}" >&2; exit 1; }

STARTED_A="$(verify_native_restore_delivery "scenario-a" "${COMMAND_ID_A}" "${TARGET_KEY_A}" \
  "${METHODS_LOG_1}" "${METHODS_MARK_A}" "${ACP_REQUEST_LOG}" "${BASELINE_A}" \
  "${PROVIDER_LOG_A}" "${RESTART_EPOCH_A}" "${CURSOR_A}" "${PROVIDER_PUBKEY}")"
pass 6 "scenario A: waiting → kill -9 → restart → 46008 delivered exactly one turn_started (${STARTED_A:0:16}...) to the original generation ${TARGET_KEY_A} via native restore (session/load observed, no session/new)"

# ── Step 7: Scenario B — result ready → kill -9 before delivery → restart ──
log "Scenario B: post the 46008 first, catch the record at 'ready', then kill -9 before admission and restart..."
SCENARIO_B_OK=0
for ATTEMPT_B in 1 2 3; do
  split_lines_into SCEN_B "$(register_fresh_continuation "${PROVIDER_PUBKEY}" "${ACP_REQUEST_LOG}" "scenario B attempt ${ATTEMPT_B}: ready-kill-restart")"
  TARGET_KEY_B="${SCEN_B[0]}"; COMMAND_ID_B="${SCEN_B[1]}"; COMMIT_B="${SCEN_B[2]}"; BASELINE_B="${SCEN_B[3]}"

  CURSOR_B="$(saved_cursor "${STATE_DIR}" "${TARGET_KEY_B}" "${PROVIDER_PUBKEY}")"
  post_ci_result_webhook "${COMMIT_B}"

  # Single-process tight poll (see wait_for_ready_tight's comment): with a
  # fake ACP adapter and no network model call, admission-after-ready can
  # complete in well under a bash loop's subprocess-spawn overhead, so this
  # sometimes never observes "ready" at all — that is the same lost race as
  # the post-kill check below catching "claimed", just resolved before this
  # host ever got a durable "ready" snapshot on disk. Either way, retry with
  # a fresh registration rather than treating it as a hard failure.
  READY_STATE_B="$(wait_for_ready_tight "${STATE_DIR}" "${COMMAND_ID_B}" 10)"
  if [[ "${READY_STATE_B}" != "ready" ]]; then
    log "scenario B attempt ${ATTEMPT_B}: ${COMMAND_ID_B} never observably reached 'ready' (last saw '${READY_STATE_B}'); admission likely completed before this host's first read — retrying"
    continue
  fi

  # Kill first, measure second: every extra command between observing
  # "ready" and the kill signal is more time for admission to win the race.
  kill -9 "${PROVIDER_PID}" 2>/dev/null || true
  METHODS_MARK_B="$(wc -l < "${METHODS_LOG_1}" | tr -d ' ')"
  wait_pid_exit "${PROVIDER_PID}"

  # Both stores matter: operation/command claims precede mark_claimed, whose
  # write may fail. Retry unless Ready survives and neither ledger has a claim.
  POST_KILL_STATE_B="$(ci_continuation_store_state "${STATE_DIR}" "${COMMAND_ID_B}")"
  if [[ "${POST_KILL_STATE_B}" != "ready" ]] || has_start_claim "${STATE_DIR}" "${COMMAND_ID_B}"; then
    log "scenario B attempt ${ATTEMPT_B}: admission won the pre-kill race (store shows '${POST_KILL_STATE_B}' for ${COMMAND_ID_B}); retrying"
    RETRY_LOG_B="${WORKDIR}/provider-restart-b-retry-${ATTEMPT_B}.log"
    spawn_provider "${PROVIDER_KEY}" "${STATE_DIR}" "${PROJECTS_FILE}" "${RUNTIMES_JSON}" \
      "${RETRY_LOG_B}" "${METHODS_LOG_1}" "${ACP_REQUEST_LOG}"
    PROVIDER_PID="${LAST_PID}"
    wait_provider_pubkey "${PROVIDER_PID}" "${RETRY_LOG_B}" >/dev/null
    wait_provider_relay_connected "${PROVIDER_PID}" "${RETRY_LOG_B}"
    continue
  fi

  PROVIDER_LOG_B="${WORKDIR}/provider-restart-b.log"
  RESTART_EPOCH_B="$(date +%s)"
  spawn_provider "${PROVIDER_KEY}" "${STATE_DIR}" "${PROJECTS_FILE}" "${RUNTIMES_JSON}" \
    "${PROVIDER_LOG_B}" "${METHODS_LOG_1}" "${ACP_REQUEST_LOG}"
  PROVIDER_PID="${LAST_PID}"
  RESTARTED_PUBKEY_B="$(wait_provider_pubkey "${PROVIDER_PID}" "${PROVIDER_LOG_B}")"
  [[ "${RESTARTED_PUBKEY_B}" == "${PROVIDER_PUBKEY}" ]] \
    || { err "scenario B: restarted provider pubkey ${RESTARTED_PUBKEY_B} != original ${PROVIDER_PUBKEY}"; exit 1; }
  wait_provider_relay_connected "${PROVIDER_PID}" "${PROVIDER_LOG_B}"
  ok "Scenario B attempt ${ATTEMPT_B}: killed before admission (Ready and neither start ledger claimed), provider restarted (pid ${PROVIDER_PID})"

  STARTED_B=""
  for _ in $(seq 1 30); do
    STARTED_B="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}" \
      | python3 -c "
import json, sys
events = json.load(sys.stdin)
for e in events:
    c = json.loads(e['content'])
    if c.get('commandId') == '${COMMAND_ID_B}' and c.get('status') == 'turn_started':
        print(e['id']); break
else:
    print('')
")"
    [[ -n "${STARTED_B}" ]] && break
    sleep 1
  done
  [[ -n "${STARTED_B}" ]] \
    || { err "scenario B attempt ${ATTEMPT_B}: no turn_started receipt for ${COMMAND_ID_B} within 30s"; cat "${PROVIDER_LOG_B}" >&2; exit 1; }

  STARTED_B="$(verify_native_restore_delivery "scenario-b" "${COMMAND_ID_B}" "${TARGET_KEY_B}" \
    "${METHODS_LOG_1}" "${METHODS_MARK_B}" "${ACP_REQUEST_LOG}" "${BASELINE_B}" \
    "${PROVIDER_LOG_B}" "${RESTART_EPOCH_B}" "${CURSOR_B}" "${PROVIDER_PUBKEY}")"
  SCENARIO_B_OK=1
  pass 7 "scenario B (attempt ${ATTEMPT_B}/3): ready → kill -9 before admission → restart delivered exactly one turn_started (${STARTED_B:0:16}...) to the original generation ${TARGET_KEY_B} via native restore"
  break
done
[[ "${SCENARIO_B_OK}" -eq 1 ]] \
  || { err "scenario B: admission won the pre-kill race on all 3 attempts; could not exercise ready→kill→restart"; exit 1; }

# ── Step 8: Scenario C — rejected load → durable NATIVE_RESTORE_REJECTED ────
log "Scenario C: a second provider, whose ACP stub advertises then REJECTS session/load, proves a refused restore ends in a durable refusal, never a fresh conversation..."
PROVIDER_KEY_C="$(openssl rand -hex 32)"
STATE_DIR_C="${WORKDIR}/provider-state-c"
mkdir -p "${STATE_DIR_C}" "${WORKDIR}/checkout-c"
PROJECTS_FILE_C="${WORKDIR}/projects-c.json"
cat > "${PROJECTS_FILE_C}" <<EOF
{"version":1,"channels":{"${CHANNEL}":"${WORKDIR}/checkout-c"}}
EOF
RUNTIMES_JSON_C="$(python3 -c "
import json
print(json.dumps([{
  'instanceRef': 'claude-primary',
  'driver': 'claude-agent-acp',
  'runtime': 'claude',
  'agentCommand': 'bash',
  'agentArgs': ['${REJECT_AGENT}'],
  'defaultModel': 'default',
  'allowedModels': ['default'],
}]))
")"
ACP_REQUEST_LOG_C="${WORKDIR}/acp-requests-c.jsonl"
: > "${ACP_REQUEST_LOG_C}"
METHODS_LOG_C="${WORKDIR}/methods-c.log"
: > "${METHODS_LOG_C}"

BEEKEEPER_PRIVATE_KEY="${PROVIDER_KEY_C}" bee channels join --channel "${CHANNEL}" >/dev/null

PROVIDER_LOG_C="${WORKDIR}/provider-c.log"
CATALOG_BEFORE_C_START="$(channel_catalog_count)"
spawn_provider "${PROVIDER_KEY_C}" "${STATE_DIR_C}" "${PROJECTS_FILE_C}" "${RUNTIMES_JSON_C}" \
  "${PROVIDER_LOG_C}" "${METHODS_LOG_C}" "${ACP_REQUEST_LOG_C}"
PROVIDER_PID_C="${LAST_PID}"
PROVIDER_PUBKEY_C="$(wait_provider_pubkey "${PROVIDER_PID_C}" "${PROVIDER_LOG_C}")"
wait_catalog_above "${CATALOG_BEFORE_C_START}" >/dev/null
ok "Scenario C: second provider live (pubkey=${PROVIDER_PUBKEY_C}, rejecting ACP stub)"

split_lines_into SCEN_C "$(register_fresh_continuation "${PROVIDER_PUBKEY_C}" "${ACP_REQUEST_LOG_C}" "scenario C: rejected-load")"
TARGET_KEY_C="${SCEN_C[0]}"; COMMAND_ID_C="${SCEN_C[1]}"; COMMIT_C="${SCEN_C[2]}"; BASELINE_C="${SCEN_C[3]}"
CURSOR_C="$(saved_cursor "${STATE_DIR_C}" "${TARGET_KEY_C}" "${PROVIDER_PUBKEY_C}")"

WAITING_STATE_C="$(ci_continuation_store_state "${STATE_DIR_C}" "${COMMAND_ID_C}")"
[[ "${WAITING_STATE_C}" == "waiting" ]] \
  || { err "scenario C: expected ${COMMAND_ID_C} to be 'waiting' before the kill, found '${WAITING_STATE_C}'"; exit 1; }

METHODS_MARK_C="$(wc -l < "${METHODS_LOG_C}" | tr -d ' ')"
kill -9 "${PROVIDER_PID_C}"
wait_pid_exit "${PROVIDER_PID_C}"

PROVIDER_LOG_C_RESTART="${WORKDIR}/provider-restart-c.log"
spawn_provider "${PROVIDER_KEY_C}" "${STATE_DIR_C}" "${PROJECTS_FILE_C}" "${RUNTIMES_JSON_C}" \
  "${PROVIDER_LOG_C_RESTART}" "${METHODS_LOG_C}" "${ACP_REQUEST_LOG_C}"
PROVIDER_PID_C="${LAST_PID}"
RESTARTED_PUBKEY_C="$(wait_provider_pubkey "${PROVIDER_PID_C}" "${PROVIDER_LOG_C_RESTART}")"
[[ "${RESTARTED_PUBKEY_C}" == "${PROVIDER_PUBKEY_C}" ]] \
  || { err "scenario C: restarted provider pubkey ${RESTARTED_PUBKEY_C} != original ${PROVIDER_PUBKEY_C}"; exit 1; }
wait_provider_relay_connected "${PROVIDER_PID_C}" "${PROVIDER_LOG_C_RESTART}"
ok "Scenario C: provider restarted (pid ${PROVIDER_PID_C}) with the rejecting ACP stub"

log "Scenario C: posting the 46008 for commit ${COMMIT_C}..."
post_ci_result_webhook "${COMMIT_C}"

STAGE_C=""
for _ in $(seq 1 30); do
  STATUS_JSON_C="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee ci continuation status \
    --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY_C}" --target "${TARGET_KEY_C}" \
    --command-id "${COMMAND_ID_C}")"
  STAGE_C="$(python3 -c "import json,sys; d=json.loads(sys.argv[1]); print(f\"{d['stage']}:{d.get('refusalCode') or ''}\")" "${STATUS_JSON_C}")"
  [[ "${STAGE_C}" == "refused:NATIVE_RESTORE_REJECTED" ]] && break
  sleep 1
done
[[ "${STAGE_C}" == "refused:NATIVE_RESTORE_REJECTED" ]] \
  || { err "scenario C: ${COMMAND_ID_C} did not reach refused/NATIVE_RESTORE_REJECTED within 30s (last: ${STAGE_C})"; cat "${PROVIDER_LOG_C_RESTART}" >&2; exit 1; }

RECEIPTS_C_JSON="$(BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}")"
python3 -c "
import json, sys
sys.path.insert(0, '${WORKDIR}')
import helpers
events = json.loads(sys.argv[1])
refusals = []
started = []
for e in events:
    c = json.loads(e['content'])
    if c.get('commandId') != '${COMMAND_ID_C}':
        continue
    assert e['pubkey'] == '${PROVIDER_PUBKEY_C}', e
    assert helpers.target_to_key(c['session']) == '${TARGET_KEY_C}', c
    if c.get('status') == 'turn_refused':
        refusals.append(c)
    if c.get('status') == 'turn_started':
        started.append(c)
assert not started, f'scenario C must never produce a turn_started, found {started!r}'
assert len(refusals) == 1, f'expected exactly one turn_refused, found {len(refusals)}: {refusals!r}'
assert refusals[0]['error']['code'] == 'NATIVE_RESTORE_REJECTED', refusals[0]
" "${RECEIPTS_C_JSON}"

POST_RESTART_METHODS_C="$(tail -n +"$((METHODS_MARK_C + 1))" "${METHODS_LOG_C}")"
if ! grep -q '^session/load$' <<<"${POST_RESTART_METHODS_C}"; then
  err "scenario C: ${METHODS_LOG_C} shows no session/load after the restart"
  exit 1
fi
if grep -q '^session/new$' <<<"${POST_RESTART_METHODS_C}"; then
  err "scenario C: ${METHODS_LOG_C} shows session/new after the restart — a rejected load must never fall through to a new conversation. Methods observed since restart:"
  err "${POST_RESTART_METHODS_C}"
  exit 1
fi

python3 - "${ACP_REQUEST_LOG_C}" "${BASELINE_C}" "${CURSOR_C}" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as source:
    requests = [json.loads(line) for line in source if line.strip()]
assert sum(row.get("method") == "session/prompt" for row in requests) == int(sys.argv[2]), requests
loads = [row for row in requests if row.get("method") == "session/load"]
assert len(loads) == 1 and loads[0]["params"]["sessionId"] == sys.argv[3], loads
PY

pass 8 "scenario C: rejecting stub → waiting → kill -9 → restart → 46008 produced a durable turn_refused/NATIVE_RESTORE_REJECTED for ${COMMAND_ID_C} (target ${TARGET_KEY_C}), no session/new after the restart, and no turn_started"

else
  log "CI_CONTINUATION_SCENARIOS=basic — skipping the kill/restart scenarios (steps 6-8)"
fi

ok "ALL STEPS PASSED"
