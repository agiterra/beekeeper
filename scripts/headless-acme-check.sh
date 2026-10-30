#!/usr/bin/env bash
# =============================================================================
# headless-acme-check.sh — a plugged runtime, end to end, out of process
# =============================================================================
#
# Round 1 of the pluggable slice proved the `acme` runtime descriptor through
# the provider's in-process harness
# (`crates/beekeeper-session-provider/src/runtime_descriptor_plug_tests.rs`).
# This script proves the same plug with the real binaries as separate OS
# processes, the way an operator would do it on a server
# (`docs/agent-host.md` § On a server, `docs/runtime-descriptor.md`
# § Commissioning):
#
#   1. The isolated relay `foreign-client-check` uses (scratch Postgres
#      database in the dev Postgres, `buzz-postgres` unless
#      scripts/lib/dev-services.sh names another, dropped on exit; Redis
#      logical DB 14).
#   2. `bee host commission --runtime acme --driver conformance/acme-acp/acme-acp
#      --credential-env ACME_TOKEN --credential-source acme-vault
#      --credential-file <0600 json>` into a fresh `BEEKEEPER_HOST_HOME`;
#      `host.json` carries the descriptor as written.
#   3. `beekeeper-host run` in the foreground, as this script's own child on
#      its own control socket; it spawns the real `beekeeper-session-provider`.
#   4. `bee agents create` — the canonical headless creation path — admits
#      one participant on the `acme-primary` runtime and waits for the
#      provider's signed `created` receipt.
#   5. One addressed channel message (`bee messages send --mention`).
#      Exactly one signed kind-9 reply from the stub arrives in the thread,
#      carrying `sha256(ACME_TOKEN)[:16]` — the credential reached the adapter
#      through the descriptor's `credentialEnv`, and the value itself appears
#      in no published event.
#   6. The provider's signed 44223 metadata for that session says
#      `credentialSource: "acme-vault"`, `runtime: "acme"`, driver `acme-acp`.
#   7. A live kind-24224 presence heartbeat from the provider is readable
#      through the HTTP bridge (`/query`), the relay's Redis register.
#   8. `bee agents stop` yields a signed 44224 `stopped` receipt; then
#      `bee host stop` ends the provider and the host still answers
#      (`notSupervised`).
#
# Every signed-event assertion (5, 6, 8) is read back through
# `conformance/foreign-client/foreign-native-read.mjs`, which verifies every
# signature with `nostr-tools` and nothing of Beekeeper's; `bee events query`
# strips signatures and is used only for counting and for the presence read.
#
# What this does NOT prove (named, not faked): no real model — the stub
# answers from a fixed string; one machine, one channel, one turn; no
# `session/load`, steering or restart, which the stub refuses by code; and
# nothing about the login-time registration (`bee host install`), which
# `just test-host` and the install unit tests cover.
#
# Usage: ./scripts/headless-acme-check.sh
# Requires: hermit activated; Postgres+Redis+S3 up (`just _ensure-services`
#   plus `docker compose up -d rustfs rustfs-init`); BEE_BIN, RELAY_BIN,
#   HOST_BIN and PROVIDER_BIN built (the `headless-acme-check` just recipe
#   builds them); `node`, `python3`, `docker`, `curl`, `openssl` on PATH;
#   `nostr-tools` resolvable the way `foreign-client-check` needs it.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
BEE_BIN="${BEE_BIN:-${TARGET_DIR}/debug/bee}"
RELAY_BIN="${RELAY_BIN:-${TARGET_DIR}/debug/beekeeper-relay}"
HOST_BIN="${HOST_BIN:-${TARGET_DIR}/debug/beekeeper-host}"
PROVIDER_BIN="${PROVIDER_BIN:-${TARGET_DIR}/debug/beekeeper-session-provider}"
CLIENT_DIR="${REPO_ROOT}/conformance/foreign-client"
STUB="${REPO_ROOT}/conformance/acme-acp/acme-acp"

for bin in "${BEE_BIN}" "${RELAY_BIN}" "${HOST_BIN}" "${PROVIDER_BIN}" "${STUB}"; do
  if [[ ! -x "${bin}" ]]; then
    echo "missing executable: ${bin} (run \`just headless-acme-check\`, which builds the binaries first)" >&2
    exit 1
  fi
done
for tool in node python3 docker curl openssl git; do
  command -v "${tool}" >/dev/null || { echo "missing tool on PATH: ${tool}" >&2; exit 1; }
done
# shellcheck source=lib/dev-services.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/dev-services.sh"
pg_health="$(dev_service_health "${BEEKEEPER_DEV_POSTGRES_CONTAINER}")"
if [[ "${pg_health}" != "healthy" ]]; then
  echo "${BEEKEEPER_DEV_POSTGRES_CONTAINER} is ${pg_health}; run \`just _ensure-services\` first" >&2
  exit 1
fi
s3_health="$(dev_service_health "${BEEKEEPER_DEV_S3_CONTAINER}")"
if [[ "${s3_health}" != "healthy" ]]; then
  echo "${BEEKEEPER_DEV_S3_CONTAINER} is ${s3_health}; the relay's git object-store probe needs it — run \`docker compose up -d rustfs rustfs-init\`" >&2
  exit 1
fi

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
log() { echo -e "${BLUE}[acme]${NC} $*"; }
ok()  { echo -e "${GREEN}[acme]${NC} $*"; }
err() { echo -e "${RED}[acme]${NC} $*" >&2; }

START_EPOCH="$(date +%s)"
# Short base on purpose: the host's control socket is an AF_UNIX path, capped
# at ~104 bytes, and a scratch directory under a long TMPDIR would exceed it.
WORKDIR="${HEADLESS_ACME_WORKDIR:-$(mktemp -d /tmp/acme-check.XXXXXX)}"
mkdir -p "${WORKDIR}"
chmod 700 "${WORKDIR}"
DB_NAME="buzz_acme_check_$$_$(date +%s)"
RELAY_PID=""
PROXY_PID=""
HOST_PID=""
RESULT_FILE="${WORKDIR}/results"
: > "${RESULT_FILE}"
FAIL_FILE="${WORKDIR}/failures"

cleanup() {
  local status=$?
  # Only the processes this script started, never another host or provider on
  # this machine: the host first (SIGTERM, it takes its provider with it), then
  # the relay.
  HOST_PID="$(cat "${WORKDIR}/host.pid" 2>/dev/null || true)"
  if [[ -n "${HOST_PID}" ]] && kill -0 "${HOST_PID}" 2>/dev/null; then
    kill -TERM "${HOST_PID}" 2>/dev/null || true
    for _ in $(seq 1 30); do kill -0 "${HOST_PID}" 2>/dev/null || break; sleep 0.5; done
    kill -9 "${HOST_PID}" 2>/dev/null || true
  fi
  if [[ -n "${RELAY_PID}" ]]; then kill "${RELAY_PID}" 2>/dev/null || true; fi
  if [[ -n "${PROXY_PID}" ]]; then kill "${PROXY_PID}" 2>/dev/null || true; fi
  source_manifest "${WORKDIR}/source-after.json"
  sleep 1
  dev_pg -d postgres \
    -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
  local passes fails
  passes="$(grep -c '^PASS' "${RESULT_FILE}" 2>/dev/null || true)"; passes="${passes:-0}"
  fails="$(grep -c '^FAIL' "${RESULT_FILE}" 2>/dev/null || true)"; fails="${fails:-0}"
  echo
  echo "──────────────────────────────────────────────────────────────────────"
  echo " headless-acme check — ${passes} passed, ${fails} failed, $(( $(date +%s) - START_EPOCH ))s wall"
  echo "──────────────────────────────────────────────────────────────────────"
  sed 's/^/  /' "${RESULT_FILE}" 2>/dev/null || true
  if [[ ${status} -ne 0 || -s "${FAIL_FILE}" ]]; then
    err "run did not pass cleanly (exit ${status}, ${fails} failed step(s)); logs and records preserved at ${WORKDIR}"
    exit 1
  fi
  ok "ALL STEPS PASSED"
  if [[ "${HEADLESS_ACME_KEEP:-0}" == 1 ]]; then
    ok "evidence preserved at ${WORKDIR}"
    exit 0
  fi
  # The provider resolves `node` through this repository's hermit stubs, and
  # hermit then unpacks a read-only package cache under the fake HOME; make it
  # writable before removing it, or the scratch directory outlives the run.
  chmod -R u+w "${WORKDIR}" 2>/dev/null || true
  rm -rf "${WORKDIR}"
  exit 0
}
trap cleanup EXIT

pass() { printf 'PASS %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"; echo -e "${GREEN}[PASS $1]${NC} $2"; }
fail() { printf 'FAIL %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"; echo -e "${RED}[FAIL $1]${NC} $2" >&2; printf 'step %s\n' "$1" >> "${FAIL_FILE}"; }

# Same shape as foreign-client-check: each step in a subshell under its own
# `set -e`, the verdict read from `$?` with errexit off only around the call,
# never as an `if` condition (which swallows a failed assertion).
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
strip_ansi() { sed -E $'s/\x1b\\[[0-9;]*[a-zA-Z]//g'; }

source_manifest() {
  python3 - "$1" <<'MANIFEST'
import hashlib,json,pathlib,subprocess,sys
paths=set(subprocess.check_output(['git','diff','--name-only','HEAD'],text=True).splitlines())
paths.update(subprocess.check_output(['git','ls-files','--others','--exclude-standard'],text=True).splitlines())
manifest={'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'files':{p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() if pathlib.Path(p).is_file() else None for p in sorted(paths)}}
pathlib.Path(sys.argv[1]).write_text(json.dumps(manifest,indent=2))
MANIFEST
}
source_manifest "${WORKDIR}/source-before.json"
python3 - "${WORKDIR}/binaries-before.json" "${BEE_BIN}" "${RELAY_BIN}" "${HOST_BIN}" "${PROVIDER_BIN}" "${STUB}" "${REPO_ROOT}/target/debug/git-credential-nostr" "${REPO_ROOT}/conformance/acme-acp/hire_report.py" "${REPO_ROOT}/conformance/acme-acp/hire_proxy.py" "${REPO_ROOT}/conformance/acme-acp/hire_git_fault.py" <<'HASHES'
import hashlib,json,pathlib,sys
pathlib.Path(sys.argv[1]).write_text(json.dumps({str(pathlib.Path(p).resolve()):hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() for p in sys.argv[2:]},indent=2))
HASHES

# ── Relay ───────────────────────────────────────────────────────────────────
log "creating scratch database ${DB_NAME}..."
dev_pg -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

RELAY_PORT="$(free_port)"
RELAY_BIND_PORT="${RELAY_PORT}"
if [[ "${HEADLESS_HIRE_CHECK:-0}" == 1 ]]; then
  dev_services_are_compose_defaults && { err "hire proof requires explicitly isolated service containers"; exit 1; }
  RELAY_BIND_PORT="$(free_port)"
  python3 "${REPO_ROOT}/conformance/acme-acp/hire_proxy.py" "${RELAY_PORT}" "${RELAY_BIND_PORT}" "${WORKDIR}" > "${WORKDIR}/proxy.log" 2>&1 &
  PROXY_PID=$!
fi
RELAY_HTTP="http://127.0.0.1:${RELAY_PORT}"
RELAY_WS="ws://127.0.0.1:${RELAY_PORT}"
RELAY_KEY="$(openssl rand -hex 32)"
log "starting beekeeper-relay on ${RELAY_HTTP} (scratch DB, Redis DB 14)..."
DATABASE_URL="$(dev_pg_url "${DB_NAME}")" \
  REDIS_URL="$(dev_redis_url 14)" \
  RELAY_URL="${RELAY_WS}" \
  BEEKEEPER_BIND_ADDR="127.0.0.1:${RELAY_BIND_PORT}" \
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
ok "relay ready at ${RELAY_HTTP}"

# ── Owner, channel, credential, host home ───────────────────────────────────
umask 077
OWNER_KEY="$(openssl rand -hex 32)"
printf 'BEEKEEPER_PRIVATE_KEY=%s\n' "${OWNER_KEY}" > "${WORKDIR}/owner.env"
ACME_SECRET="$(openssl rand -hex 24)"
ACME_WITNESS="$(printf '%s' "${ACME_SECRET}" | python3 -c 'import hashlib,sys; print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest()[:16])')"

HOST_HOME="${WORKDIR}/host-home"          # BEEKEEPER_HOST_HOME: service state
FAKE_HOME="${WORKDIR}/home"               # OS HOME for the host and its children
SOCK="${WORKDIR}/h.sock"                  # BEEKEEPER_HOST_SOCK, this host's own
CHECKOUT="${WORKDIR}/checkout"            # the participant's working tree
CREDENTIALS="${WORKDIR}/acme-credentials.json"
mkdir -p "${HOST_HOME}" "${FAKE_HOME}" "${CHECKOUT}"
git -C "${CHECKOUT}" init -q
printf '{"runtimes":{"acme-primary":{"ACME_TOKEN":"%s"}}}\n' "${ACME_SECRET}" > "${CREDENTIALS}"
chmod 600 "${CREDENTIALS}"

# Every `bee` call is the owner, against this relay, naming this host home and
# socket; nothing reads the machine's own host, home or instance.
bee() {
  BEEKEEPER_RELAY_URL="${RELAY_HTTP}" BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" \
  BEEKEEPER_HOST_HOME="${HOST_HOME}" BEEKEEPER_HOST_SOCK="${SOCK}" \
  BEEKEEPER_HOST_INSTANCE=production \
    "${BEE_BIN}" "$@"
}
# Signature-verified reads through the foreign client (nostr-tools only).
export RELAY_URL="${RELAY_HTTP}"
export FOREIGN_CLIENT_OUT="${WORKDIR}/records"
export FOREIGN_CLIENT_KEY_FILE="${WORKDIR}/owner.env"
export FOREIGN_CLIENT_LABEL=owner
verified_read() { node "${CLIENT_DIR}/foreign-native-read.mjs" "$1" "$2"; }

OWNER_PUBKEY="$(node "${CLIENT_DIR}/foreign-identity.mjs" | jq_py 'd["pubkey"]')"
CHANNEL="$(bee channels create --name "acme-check-$$" --type stream --visibility private | jq_py 'd["channel_id"]')"
export FOREIGN_CLIENT_CHANNEL="${CHANNEL}"
ok "owner ${OWNER_PUBKEY:0:16}… owns private channel ${CHANNEL}; ACME_TOKEN witness ${ACME_WITNESS}"

PROVIDER_PUBKEY=""
ACTOR_PUBKEY=""
# Steps run in subshells, so the facts they discover travel through files.
provider_log() {
  echo "${HOST_HOME}/.local/state/buzz/host/service/session-provider/logs/$(cat "${WORKDIR}/provider.pubkey").log"
}
provider_log_tail() { strip_ansi < "$(provider_log)" 2>/dev/null | tail -60 >&2 || true; }

# ── Steps ───────────────────────────────────────────────────────────────────
step_commission() {
  local out
  out="$(bee host commission --home "${HOST_HOME}" --runtime acme --driver "${STUB}" \
    --credential-env ACME_TOKEN --credential-source acme-vault \
    --credential-file "${CREDENTIALS}" --provider-command "${PROVIDER_BIN}" \
    --runtime-install-root "${REPO_ROOT}/conformance/acme-acp")"
  echo "${out}" > "${WORKDIR}/commission.json"
  [[ "$(echo "${out}" | jq_py 'd["commissioned"] and d["relay_admitted"]')" == "True" ]] \
    || { err "commission did not report admission: ${out}"; return 1; }
  PROVIDER_PUBKEY="$(echo "${out}" | jq_py 'd["provider_pubkey"]')"
  echo "${PROVIDER_PUBKEY}" > "${WORKDIR}/provider.pubkey"
  local host_json="${HOST_HOME}/.local/state/buzz/host/host.json"
  [[ -f "${host_json}" ]] || { err "no host.json at ${host_json}"; return 1; }
  local descriptor
  descriptor="$(jq_py 'json.dumps([d["runtimes"][0][k] for k in ("instanceRef","driver","runtime","agentCommand","credentialEnv","credentialSource")])' < "${host_json}")"
  [[ "${descriptor}" == "[\"acme-primary\", \"acme-acp\", \"acme\", \"${STUB}\", [\"ACME_TOKEN\"], \"acme-vault\"]" ]] \
    || { err "host.json descriptor is not the commissioned one: ${descriptor}"; return 1; }
  local pointer="${HOST_HOME}/.local/state/buzz/host/service/session-provider/${PROVIDER_PUBKEY}/runtime-credentials-source.json"
  [[ -f "${pointer}" ]] || { err "no credential pointer at ${pointer}"; return 1; }
  grep -q "${ACME_SECRET}" "${host_json}" && { err "host.json carries the secret"; return 1; }
  pass 1 "commissioned provider ${PROVIDER_PUBKEY:0:16}… (relay_admitted, runtime_account unconfirmed); host.json runtimes[0] = acme-primary/acme-acp/acme with credentialEnv [ACME_TOKEN], credentialSource acme-vault; secret only in the 0600 file"
}

host_status() { bee host status 2>/dev/null; }
provider_state() { host_status | jq_py 'd["provider"]["state"]' 2>/dev/null || echo unknown; }

# `csp=info` is appended whatever RUST_LOG says: `just` loads `.env`, whose
# RUST_LOG names relay targets only, and step 2 waits on the provider's own
# `csp::authority` "witnessed relay identity" line — without the target that
# wait times out while the provider is in fact live (round 2 finalizer run).
step_host_run() {
  PROVIDER_PUBKEY="$(cat "${WORKDIR}/provider.pubkey")"
  HOME="${FAKE_HOME}" BEEKEEPER_HOST_HOME="${HOST_HOME}" BEEKEEPER_HOST_SOCK="${SOCK}" \
    BEEKEEPER_HOST_INSTANCE=production RUST_LOG="${RUST_LOG:-info},csp=info" \
    env -u FOREIGN_CLIENT_KEY_FILE -u FOREIGN_CLIENT_KEY -u BEEKEEPER_PRIVATE_KEY -u NOSTR_PRIVATE_KEY "${HOST_BIN}" run > "${WORKDIR}/host.log" 2>&1 &
  HOST_PID=$!
  echo "${HOST_PID}" > "${WORKDIR}/host.pid"
  local state=""
  for _ in $(seq 1 60); do
    kill -0 "${HOST_PID}" 2>/dev/null || { err "host died"; cat "${WORKDIR}/host.log" >&2; return 1; }
    state="$(provider_state)"
    [[ "${state}" == "live" ]] && break
    sleep 1
  done
  [[ "${state}" == "live" ]] || { err "provider never went live (state ${state})"; cat "${WORKDIR}/host.log" >&2; return 1; }
  local pid relay_state
  pid="$(host_status | jq_py 'd["provider"].get("pid","")')"
  relay_state="$(host_status | jq_py 'd["relayConnection"]["state"]')"
  [[ "$(ps -o ppid= -p "${pid}" | tr -d ' ')" == "${HOST_PID}" ]] || { err "provider ${pid} is not the host's child"; return 1; }
  local PROVIDER_LOG; PROVIDER_LOG="$(provider_log)"
  local connected=""
  for _ in $(seq 1 60); do
    if [[ -f "${PROVIDER_LOG}" ]] && strip_ansi < "${PROVIDER_LOG}" | grep -q 'witnessed relay identity'; then connected=yes; break; fi
    sleep 1
  done
  [[ "${connected}" == "yes" ]] || { err "provider never witnessed the relay; log: ${PROVIDER_LOG}"; strip_ansi < "${PROVIDER_LOG}" | tail -40 >&2; return 1; }
  pass 2 "beekeeper-host ${HOST_PID} runs in the foreground on ${SOCK##*/}; its child provider ${pid} is live (host reports relay ${relay_state}, as it must — it holds no relay socket); the provider witnessed the relay identity"
}

step_create() {
  PROVIDER_PUBKEY="$(cat "${WORKDIR}/provider.pubkey")"
  local op out
  op="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  if ! out="$(bee agents create --operation-id "${op}" --channel "${CHANNEL}" \
      --display-name acme-bot --system-prompt "You are the acme conformance stub." \
      --home "${HOST_HOME}" --cwd "${CHECKOUT}" --provider-instance acme-primary \
      --wait-secs 120 2> "${WORKDIR}/create.err")"; then
    err "agents create failed:"; cat "${WORKDIR}/create.err" >&2
    provider_log_tail; return 1
  fi
  echo "${out}" > "${WORKDIR}/create.json"
  ACTOR_PUBKEY="$(echo "${out}" | jq_py 'd["agent_pubkey"]')"
  echo "${ACTOR_PUBKEY}" > "${WORKDIR}/actor.pubkey"
  local admitted receipt driver runtime
  admitted="$(echo "${out}" | jq_py 'd.get("host_admitted")')"
  receipt="$(echo "${out}" | jq_py 'd.get("receipt_event_id") or ""')"
  driver="$(echo "${out}" | jq_py 'd["target"]["driver"]')"
  [[ "${admitted}" == "True" && -n "${receipt}" ]] || { err "create was not admitted with a receipt: ${out}"; return 1; }
  [[ "${driver}" == "acme-acp" ]] || { err "target driver is ${driver}, not acme-acp"; return 1; }
  # The receipt is the provider's own signed `created`, verified by the foreign reader.
  local created
  created="$(verified_read "{\"kinds\":[44224],\"ids\":[\"${receipt}\"],\"#h\":[\"${CHANNEL}\"]}" create-receipt.json \
    | jq_py 'json.dumps([(e["id"], json.loads(e["content"])["status"]) for e in d])')"
  [[ "${created}" == "[[\"${receipt}\", \"created\"]]" ]] || { err "created receipt not read back verified: ${created}"; return 1; }
  pass 3 "bee agents create admitted actor ${ACTOR_PUBKEY:0:16}… on acme-primary (target driver acme-acp); the provider's signed created receipt ${receipt:0:16}… verified"
}

step_reply() {
  ACTOR_PUBKEY="$(cat "${WORKDIR}/actor.pubkey")"
  local sent msg_id
  sent="$(bee messages send --channel "${CHANNEL}" --mention "${ACTOR_PUBKEY}" \
    --content "@acme-bot which credential do you hold?")"
  msg_id="$(echo "${sent}" | jq_py 'd["event_id"]')"
  echo "${msg_id}" > "${WORKDIR}/message.id"
  local replies="[]" count=0
  for _ in $(seq 1 90); do
    replies="$(verified_read "{\"kinds\":[9],\"authors\":[\"${ACTOR_PUBKEY}\"],\"#h\":[\"${CHANNEL}\"]}" replies.json)"
    count="$(echo "${replies}" | jq_py 'len(d)')"
    [[ "${count}" -ge 1 ]] && break
    sleep 1
  done
  if [[ "${count}" -lt 1 ]]; then
    err "no reply from the actor within 90s; provider log tail:"; provider_log_tail; return 1
  fi
  # Settle, then split the actor's posts: the provider also narrates the turn
  # in the thread under the actor's key ("Request turn_queued.",
  # "Request turn_started.", "Turn ended (...)", and on a runtime with no
  # native steer the disclosed "Request queued for the next turn boundary:
  # STEER_UNSUPPORTED" downgrade, conversation.rs). Exactly one post is the
  # stub's answer, and it is identified by the witness it carries.
  sleep 3
  replies="$(verified_read "{\"kinds\":[9],\"authors\":[\"${ACTOR_PUBKEY}\"],\"#h\":[\"${CHANNEL}\"]}" replies.json)"
  local verdict
  verdict="$(python3 - "${WORKDIR}/records/replies.json" "${msg_id}" "${ACME_WITNESS}" "${ACME_SECRET}" <<'PY'
import json, sys
body = json.load(open(sys.argv[1]))["body"]
msg, witness, secret = sys.argv[2:5]
for e in body:
    assert e["kind"] == 9, e
    assert any(t[0] == "e" and t[1] == msg for t in e["tags"]), "post %s is not threaded under %s: %s" % (e["id"][:12], msg[:12], e["tags"])
    assert secret not in e["content"], "the secret leaked in %s" % e["id"]
answers = [e for e in body if ("prefix " + witness) in e["content"]]
assert len(answers) == 1, "expected exactly one witness-bearing reply, got %d: %s" % (len(answers), [e["content"] for e in body])
assert not any("prompt 2" in e["content"] for e in body), "a second prompt ran"
status = [e["content"].split(":")[0] for e in body if e is not answers[0]]
print("%s|%s|%d|%s" % (answers[0]["id"], answers[0]["content"], len(status), "; ".join(status)))
PY
)" || { err "reply assertion failed"; return 1; }
  local reply_id content narration_count narration
  IFS='|' read -r reply_id content narration_count narration <<< "${verdict}"
  local leaked
  leaked="$(bee events query --kinds 9,44220,44221,44222,44223,44224,44225,44252 --channel "${CHANNEL}" | grep -c "${ACME_SECRET}" || true)"
  [[ "${leaked}" == "0" ]] || { err "the secret appears in ${leaked} published event(s)"; return 1; }
  pass 4 "one addressed message ${msg_id:0:16}… → exactly one signed kind-9 answer ${reply_id:0:16}… from ${ACTOR_PUBKEY:0:16}… (\"${content}\"), threaded under it, carrying sha256(ACME_TOKEN)[:16]=${ACME_WITNESS}; ${narration_count} signed narration post(s) beside it (${narration}); the secret is in no published event"
}

step_metadata() {
  PROVIDER_PUBKEY="$(cat "${WORKDIR}/provider.pubkey")"
  local rows
  rows="$(verified_read "{\"kinds\":[44223],\"authors\":[\"${PROVIDER_PUBKEY}\"],\"#h\":[\"${CHANNEL}\"]}" metadata.json)"
  local summary
  echo "${rows}" > "${WORKDIR}/metadata-rows.json"
  summary="$(python3 - "${WORKDIR}/metadata-rows.json" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
assert rows, "no 44223 from the provider"
latest = [json.loads(r["content"]) for r in rows][-1]
src = latest.get("credentialSource")
assert src == "acme-vault", "credentialSource is %r" % src
assert latest.get("runtime") == "acme", "runtime is %r" % latest.get("runtime")
target = latest.get("session") or {}
assert target.get("driver") == "acme-acp", "driver is %r" % target.get("driver")
print("%d metadata event(s); latest status %r, runtime acme, driver acme-acp, credentialSource acme-vault, capabilities %s"
      % (len(rows), latest.get("status"), json.dumps(latest.get("capabilities"))))
PY
)" || { err "44223 assertion failed: ${rows}"; return 1; }
  pass 5 "signed 44223 from the provider: ${summary}"
}

step_presence() {
  PROVIDER_PUBKEY="$(cat "${WORKDIR}/provider.pubkey")"
  local rows summary=""
  for _ in $(seq 1 40); do
    rows="$(bee events query --kinds 24224 --authors "${PROVIDER_PUBKEY}" --channel "${CHANNEL}")"
    echo "${rows}" > "${WORKDIR}/presence.json"
    summary="$(python3 - "${WORKDIR}/presence.json" "${PROVIDER_PUBKEY}" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
live = [json.loads(r["content"]) for r in rows if r.get("pubkey") == sys.argv[2]]
if not live:
    sys.exit(1)
p = live[-1]
print("instanceId %s, state %s, seq %s, ttlSecs %s, liveTargets %d"
      % (p["instanceId"], p["state"], p["seq"], p["ttlSecs"], len(p["liveTargets"])))
PY
)" && break
    sleep 1
  done
  [[ -n "${summary}" ]] || { err "no 24224 presence from ${PROVIDER_PUBKEY} readable via /query within 40s: ${rows}"; return 1; }
  pass 6 "live 24224 presence from the provider readable via /query: ${summary}"
}

step_stop() {
  PROVIDER_PUBKEY="$(cat "${WORKDIR}/provider.pubkey")"
  ACTOR_PUBKEY="$(cat "${WORKDIR}/actor.pubkey")"
  HOST_PID="$(cat "${WORKDIR}/host.pid")"
  local op out terminal
  op="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  if ! out="$(bee agents stop --operation-id "${op}" --agent-pubkey "${ACTOR_PUBKEY}" \
      --channel "${CHANNEL}" --home "${HOST_HOME}" --wait-secs 120 2> "${WORKDIR}/stop.err")"; then
    err "agents stop failed:"; cat "${WORKDIR}/stop.err" >&2; provider_log_tail; return 1
  fi
  echo "${out}" > "${WORKDIR}/stop.json"
  terminal="$(echo "${out}" | jq_py 'd.get("terminal")')"
  [[ "${terminal}" == "True" ]] || { err "stop is not terminal: ${out}"; return 1; }
  local stopped
  stopped="$(verified_read "{\"kinds\":[44224],\"authors\":[\"${PROVIDER_PUBKEY}\"],\"#h\":[\"${CHANNEL}\"]}" receipts.json \
    | jq_py 'json.dumps([e["id"] for e in d if json.loads(e["content"])["status"] == "stopped"])')"
  [[ "${stopped}" != "[]" ]] || { err "no signed stopped receipt from the provider"; return 1; }
  local stopped_id; stopped_id="$(echo "${stopped}" | jq_py 'd[0]')"
  # Now the host: the provider ends, the host keeps answering.
  local host_out
  host_out="$(bee host stop 2>&1)"
  echo "${host_out}" | grep -q '"stopped": true' || { err "host stop did not report stopping: ${host_out}"; return 1; }
  local state=""
  for _ in $(seq 1 40); do
    state="$(provider_state)"
    [[ "${state}" == "notSupervised" ]] && break
    sleep 0.5
  done
  [[ "${state}" == "notSupervised" ]] || { err "host state after stop is ${state}"; return 1; }
  kill -0 "${HOST_PID}" 2>/dev/null || { err "the host itself exited on \`host stop\`"; return 1; }
  pass 7 "bee agents stop terminal; the provider's signed 44224 stopped receipt ${stopped_id:0:16}… verified; bee host stop ended the provider (host ${HOST_PID} still answers: ${state})"
}

run_step 1 "commission a fresh host home with the acme descriptor and a 0600 credential file" step_commission
run_step 2 "run the host in the foreground; it spawns and supervises the provider" step_host_run
if [[ "${HEADLESS_HIRE_CHECK:-0}" == 1 ]]; then
  step_hire() {
    BEEKEEPER_RELAY_URL="${RELAY_HTTP}" BEEKEEPER_PRIVATE_KEY="${OWNER_KEY}" \
      BEEKEEPER_HOST_HOME="${HOST_HOME}" BEEKEEPER_HOST_SOCK="${SOCK}" BEEKEEPER_HOST_INSTANCE=production \
      python3 "${REPO_ROOT}/conformance/acme-acp/hire_acceptance.py" "${WORKDIR}" "${CHANNEL}" "${OWNER_PUBKEY}" "${BEE_BIN}" || return $?
    pass 3 "canonical role install, headless hire, signed authority and actor-authored assignment report"
  }
  run_step 3 "deterministic headless hire vertical path" step_hire
else
run_step 3 "bee agents create admits one participant on acme-primary" step_create
run_step 4 "one addressed message, exactly one signed reply carrying the credential witness" step_reply
run_step 5 "signed 44223 metadata discloses credentialSource acme-vault" step_metadata
run_step 6 "live 24224 presence readable via /query" step_presence
run_step 7 "bee agents stop → signed stopped receipt; bee host stop → provider gone, host answering" step_stop

fi
