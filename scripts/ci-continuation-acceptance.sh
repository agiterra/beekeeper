#!/usr/bin/env bash
# =============================================================================
# ci-continuation-acceptance.sh — real composition proof for CI-managed turn
# continuation (docs/CI_MANAGED_CONTINUATION_IMPL.md, docs/CI_MANAGED_CONTINUATION_SPEC.md)
# =============================================================================
#
# Composes, as independent OS processes talking over a real network socket:
#   - the actual `buzz-relay` binary, against a scratch Postgres database
#     (dropped on exit) and Redis logical DB 14 — never the dev database or
#     Redis DB 0;
#   - the built `bee` CLI, driving every step a real operator would: repo,
#     project, channel, and workflow announcement, session creation, CI
#     continuation registration, and read-only verification queries;
#   - the real `buzz-session-provider` binary, with a minimal ACP-speaking
#     bash script standing in for the model adapter (`agentCommand: bash`),
#     the same technique `crates/buzz-session-provider/src/session.rs`'s own
#     `testing::fake_agent`/`GOOD_AGENT` uses inside that crate's unit suite —
#     a real subprocess boundary, no network model call.
#
# No step is hand-signed as the relay or faked: the CI result (kind 46008) is
# produced by POSTing to the workflow's real `/hooks/<id>` webhook, the exact
# production path `crates/buzz-relay/src/api/bridge_ci_result_tests.rs`
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
#
# What this does NOT prove (named, not faked):
#   - The private-project read path (§3f). This registers against a
#     `--access public` project, so admission never exercises
#     CI_RESULT_UNAVAILABLE_OR_HIDDEN or a provider-admitted private read.
#     That path is exercised by the provider's own unit suite
#     (crates/buzz-session-provider/src/tests/ci_continuation_tests.rs).
#   - A real model. The ACP adapter is a bash stub; no network call to any
#     LLM provider happens anywhere in this script.
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
RELAY_BIN="${RELAY_BIN:-${TARGET_DIR}/debug/buzz-relay}"
PROVIDER_BIN="${PROVIDER_BIN:-${TARGET_DIR}/debug/buzz-session-provider}"

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

cleanup() {
  local status=$?
  [[ -n "${PROVIDER_PID}" ]] && kill "${PROVIDER_PID}" 2>/dev/null || true
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

# ── Scratch database (never the dev DB) ──────────────────────────────────────
log "Creating scratch database ${DB_NAME}..."
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

# ── Real relay binary, scratch DB, Redis logical DB 14 ───────────────────────
RELAY_PORT="$(free_port)"
RELAY_URL="ws://127.0.0.1:${RELAY_PORT}"
RELAY_HTTP="http://127.0.0.1:${RELAY_PORT}"
RELAY_KEY="$(openssl rand -hex 32)"

log "Starting buzz-relay on ${RELAY_HTTP} (scratch DB, Redis DB 14)..."
DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/${DB_NAME}" \
  REDIS_URL="redis://localhost:6379/14" \
  RELAY_URL="${RELAY_URL}" \
  BUZZ_BIND_ADDR="127.0.0.1:${RELAY_PORT}" \
  BUZZ_RELAY_PRIVATE_KEY="${RELAY_KEY}" \
  BUZZ_REQUIRE_AUTH_TOKEN=false \
  BUZZ_RECONCILE_CHANNELS=true \
  BUZZ_AUTO_MIGRATE=true \
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
export BUZZ_RELAY_URL="${RELAY_HTTP}"

OWNER_KEY="$(openssl rand -hex 32)"
PROVIDER_KEY="$(openssl rand -hex 32)"
REPO_ID="ci-cont-repo-$$"
PROJECT_SLUG="ci-cont-project-$$"

log "Announcing repository, project, channel, workflow as the owner identity..."
BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee repos create --id "${REPO_ID}" > "${WORKDIR}/repo.json"
OWNER_HEX="$(python3 -c "
import json,sys
d=json.load(open('${WORKDIR}/repo.json'))
from urllib.parse import urlparse, parse_qs
q=parse_qs(urlparse(d['link']).query)
print(q['owner'][0])
")"
PROJECT="30621:${OWNER_HEX}:${PROJECT_SLUG}"
REPO="30617:${OWNER_HEX}:${REPO_ID}"

BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee projects create "${PROJECT_SLUG}" --repo "${REPO_ID}" --access public >/dev/null
BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee repos bind --id "${REPO_ID}" --project "${PROJECT}" >/dev/null

CHANNEL="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee channels create --name "ci-cont-$$" --type stream --visibility open \
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
WORKFLOW_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee workflows create --channel "${CHANNEL}" --yaml - < "${WORKDIR}/workflow.yaml")"
WORKFLOW_ID="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['workflow_id'])" "${WORKFLOW_JSON}")"
WEBHOOK_SECRET="$(python3 -c "
import json,sys
d=json.loads(sys.argv[1])
inner=json.loads(d['message'][len('response:'):])
print(inner['webhook_secret'])
" "${WORKFLOW_JSON}")"
ok "fixtures ready: project=${PROJECT} repository=${REPO} channel=${CHANNEL} workflow=${WORKFLOW_ID}"

# The provider must be a channel member (kind:39002, #p) before it starts —
# channel discovery (crates/buzz-acp/src/relay.rs discover_channels) runs
# once at startup, from membership, never from the projects file alone.
BUZZ_PRIVATE_KEY="${PROVIDER_KEY}" bee channels join --channel "${CHANNEL}" >/dev/null

# ── Real provider binary, fake ACP adapter (no model call) ──────────────────
ACP_REQUEST_LOG="${WORKDIR}/acp-requests.jsonl"
: > "${ACP_REQUEST_LOG}"
FAKE_AGENT="${WORKDIR}/fake-agent.sh"
cat > "${FAKE_AGENT}" <<'AGENT'
#!/bin/bash
# Minimal ACP-speaking stub — the same technique
# crates/buzz-session-provider/src/session.rs's testing::GOOD_AGENT uses in
# that crate's own unit suite. Answers initialize/session.new/session.prompt
# over JSON-RPC on stdio; never calls a model. Logs each raw session/new and
# session/prompt request so the caller can inspect the exact ACP payload.
LAST_PROMPT=""
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      LAST_PROMPT="$id"
      printf '%s\n' "$line" >> "${FABLE_ACP_REQUEST_LOG}"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$LAST_PROMPT" ;;
  esac
done
AGENT
chmod +x "${FAKE_AGENT}"

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

log "Starting buzz-session-provider (fake ACP adapter, no model)..."
BUZZ_PRIVATE_KEY="${PROVIDER_KEY}" \
  BUZZ_RELAY_URL="${RELAY_URL}" \
  BUZZ_CSP_STATE_DIR="${STATE_DIR}" \
  BUZZ_CSP_PROJECTS_FILE="${WORKDIR}/projects.json" \
  BUZZ_CSP_RUNTIMES="${RUNTIMES_JSON}" \
  FABLE_ACP_REQUEST_LOG="${ACP_REQUEST_LOG}" \
  RUST_LOG=info \
  "${PROVIDER_BIN}" > "${WORKDIR}/provider.log" 2>&1 &
PROVIDER_PID=$!

for _ in $(seq 1 30); do
  if ! kill -0 "${PROVIDER_PID}" 2>/dev/null; then
    err "provider process died during startup"
    cat "${WORKDIR}/provider.log" >&2
    exit 1
  fi
  grep -q 'pubkey=' "${WORKDIR}/provider.log" 2>/dev/null && break
  sleep 1
done
PROVIDER_PUBKEY="$(sed 's/\x1b\[[0-9;]*m//g' "${WORKDIR}/provider.log" | grep -o 'pubkey=[0-9a-f]\{64\}' | head -1 | cut -d= -f2)"
[[ -n "${PROVIDER_PUBKEY}" ]] || { err "could not read provider pubkey from its own startup log"; exit 1; }

# Wait for the provider's live catalog (44222) for this channel — proves it
# joined its subscribe/discover_channels pass, not just process liveness.
for _ in $(seq 1 30); do
  count="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44222 --channel "${CHANNEL}" \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')"
  [[ "${count}" -gt 0 ]] && break
  sleep 1
done
[[ "${count}" -gt 0 ]] || { err "provider never advertised a catalog for the channel"; cat "${WORKDIR}/provider.log" >&2; exit 1; }
ok "provider live: pubkey=${PROVIDER_PUBKEY}"

# ── Step 1: real session create ──────────────────────────────────────────────
log "bee sessions create --wait ..."
CREATE_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee sessions create --channel "${CHANNEL}" \
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
CONTINUE_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee ci continue \
  --channel "${CHANNEL}" --provider "${PROVIDER_PUBKEY}" --target "${TARGET_KEY}" \
  --project "${PROJECT}" --repository "${REPO}" \
  --commit "${COMMIT}" --check "${CHECK_NAME}" --run "${RUN_ID}" --attempt "${ATTEMPT}" \
  --workflow "${WORKFLOW_ID}" --phase "${PHASE}" \
  --continuation "${CONTINUATION_TEXT}" \
  --expires-in 86400 --ack-timeout 30)"
COMMAND_ID="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['commandId'])" "${CONTINUE_JSON}")"
printf '%s\n' "${CONTINUE_JSON}" > "${WORKDIR}/continue-output.json"
BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44220 --channel "${CHANNEL}" \
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
  RESULT_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 46008)"
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
  RECEIPTS_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}")"
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

BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44225 --channel "${CHANNEL}" \
  > "${WORKDIR}/transcript-events.json"

STATUS_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee ci continuation status \
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
assert delivered_request["params"]["sessionId"] == "acp-session-1", delivered_request
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
SECOND_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee ci continue \
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
  SECOND_STATUS_JSON="$(BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee ci continuation status \
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
BUZZ_PRIVATE_KEY="${OWNER_KEY}" bee events query --kinds 44224 --channel "${CHANNEL}" \
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

ok "ALL STEPS PASSED"
