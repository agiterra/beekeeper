#!/usr/bin/env bash
# =============================================================================
# handover-acceptance.sh — real composition proof for absent-participant
# handover (docs/HANDOVER_IMPL.md §6)
# =============================================================================
#
# Composes, as independent OS processes talking over real sockets:
#   - the actual `beekeeper-relay` binary, against a scratch Postgres database
#     (dropped on exit) and Redis logical DB 14 — never the dev database or
#     Redis DB 0;
#   - the built `bee` CLI, driving every step two real operators would;
#   - **two** real `beekeeper-session-provider` processes, P_A and P_B, with
#     distinct keys, distinct state dirs and distinct working directories,
#     each running a minimal ACP-speaking bash script in place of a model
#     adapter (the `scripts/ci-continuation-acceptance.sh` technique — a real
#     subprocess boundary, no network model call);
#   - a **relay-hosted git repository**, pushed to and fetched from over the
#     relay's own smart-HTTP transport with NIP-98 credentials
#     (`git-credential-nostr`), so the kind:30618 ref state a handover
#     checkpoint points at is produced by the relay, not asserted by this
#     script;
#   - a **one-shot hint file** per reconstruction
#     (`<dir of projects.json>/pending-hints/<createCommandId>.json`), which the
#     CLI writes and the provider consumes on admission. Step 3 rewrites
#     `projects.json` continuously while a reconstruction runs, because that is
#     what erased the binding when it lived inside that file;
#   - **two different working directories for B**: the folder its provider is
#     configured to run this project and channel in (`checkout-b-default`, a
#     plain clone) and the folder B recovers into (`checkout-b-work`). They are
#     deliberately not the same. A composition that pre-seeds one folder for
#     both cannot tell a reconstruction that placed the work from one that
#     opened the model on an untouched tree and said "recovered" anyway — the
#     defect this split exists to catch.
#
# Nothing here is hand-signed as the relay: every 40099 receipt, every 30618,
# every acceptance is produced by the relay binary itself, and every assertion
# below reads signed events back with `bee`, or reads a provider's own log,
# the ACP stub's raw request log, or git's own state. A CLI's success message
# is never the evidence for a step.
#
# Each numbered step below is one PASS line, and each asserts on signed events
# read back with `bee`, on a provider's own log, on the ACP stub's raw request
# log, or on git's state — never on a CLI's success message. What each step
# proves is written beside its assertions, in the step function itself.
#
# ── What this does NOT prove (named, not faked) ──────────────────────────────
#   - **A real model.** Both ACP adapters are bash stubs. No network model call
#     happens anywhere in this script, so nothing here says anything about what
#     an agent does with a reconstructed checkout.
#   - **Two machines.** P_A and P_B are two processes on one host, sharing a
#     filesystem, a clock and a network stack. The cross-machine claims this
#     step makes are exercised by the human runbook in
#     `plans/archive/COLLABORATION_TWO_MACHINE_ACCEPTANCE.md` §6, not here.
#   - **Seated (agent-actor) restore and re-staging.** Every session here is
#     operator-created, so `seat-requests.json` never lists one of them and the
#     `fenced`/retired seat-request behaviour is proven only by the provider's
#     own unit suite (`crates/beekeeper-session-provider/src/tests/handover_*`).
#   - **Native Windows.** Deferred and labelled (§9).
#   - **A host that disagrees with the projects file.** The binding is checked
#     as written and as the adapter observed it; a desktop host rewriting the
#     same file underneath a running provider is not modelled.
#   - **Blossom-sized captures.** Every patch here fits inside a NIP-34 patch
#     event; the blob path above the event limit is not exercised.
#
# Usage: ./scripts/handover-acceptance.sh
# Requires: hermit activated, Postgres+Redis+MinIO up (`just _ensure-services`),
#   and BEE_BIN/RELAY_BIN/PROVIDER_BIN/GIT_CREDENTIAL_NOSTR_BIN built (the
#   `test-handover` just recipe builds them; set the env vars to reuse builds).
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
BEE_BIN="${BEE_BIN:-${TARGET_DIR}/debug/bee}"
RELAY_BIN="${RELAY_BIN:-${TARGET_DIR}/debug/beekeeper-relay}"
PROVIDER_BIN="${PROVIDER_BIN:-${TARGET_DIR}/debug/beekeeper-session-provider}"
CREDENTIAL_BIN="${GIT_CREDENTIAL_NOSTR_BIN:-${TARGET_DIR}/debug/git-credential-nostr}"

for bin in "${BEE_BIN}" "${RELAY_BIN}" "${PROVIDER_BIN}" "${CREDENTIAL_BIN}"; do
  if [[ ! -x "${bin}" ]]; then
    echo "missing built binary: ${bin} (run \`just test-handover\`, which builds it first)" >&2
    exit 1
  fi
done

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[0;33m'; NC='\033[0m'
log()  { echo -e "${BLUE}[handover]${NC} $*"; }
ok()   { echo -e "${GREEN}[handover]${NC} $*"; }
warn() { echo -e "${YELLOW}[handover]${NC} $*"; }
err()  { echo -e "${RED}[handover]${NC} $*" >&2; }

START_EPOCH="$(date +%s)"
WORKDIR="$(mktemp -d /tmp/handover-acceptance.XXXXXX)"
DB_NAME="buzz_ci_handover_$$_$(date +%s)"
RELAY_PID=""
PID_FILE="${WORKDIR}/pids"
: > "${PID_FILE}"
RESULT_FILE="${WORKDIR}/results"
: > "${RESULT_FILE}"
VARS_FILE="${WORKDIR}/vars.sh"
: > "${VARS_FILE}"

# Every provider process this script has spawned, in a file rather than an
# array: steps run in subshells (see `run_step`), so a pid recorded inside one
# has to survive back into the parent's cleanup.
record_pid() { printf '%s\n' "$1" >> "${PID_FILE}"; }

cleanup() {
  local status=$? pid
  while read -r pid; do
    [[ -n "${pid}" ]] && kill -9 "${pid}" 2>/dev/null || true
  done < "${PID_FILE}"
  if [[ -n "${RELAY_PID}" ]]; then kill "${RELAY_PID}" 2>/dev/null || true; fi
  sleep 1
  docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
    -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null 2>&1 || true
  print_summary "${status}"
  if [[ ${status} -ne 0 || -s "${WORKDIR}/failures" ]]; then
    err "logs and state preserved at ${WORKDIR}"
    exit 1
  fi
  rm -rf "${WORKDIR}"
  exit 0
}

print_summary() {
  local status="$1" elapsed=$(( $(date +%s) - START_EPOCH ))
  local passes fails
  passes="$(grep -c '^PASS' "${RESULT_FILE}" 2>/dev/null || true)"
  fails="$(grep -c '^FAIL' "${RESULT_FILE}" 2>/dev/null || true)"
  passes="${passes:-0}"; fails="${fails:-0}"
  echo
  echo "──────────────────────────────────────────────────────────────────────"
  echo " handover acceptance — ${passes} passed, ${fails} failed, ${elapsed}s wall"
  echo "──────────────────────────────────────────────────────────────────────"
  sed 's/^/  /' "${RESULT_FILE}" 2>/dev/null || true
  if [[ "${fails}" -eq 0 && "${status}" -eq 0 ]]; then
    ok "ALL STEPS PASSED"
  else
    err "run did not pass cleanly (exit ${status})"
  fi
}
trap cleanup EXIT

# ── Step bookkeeping ────────────────────────────────────────────────────────
# Each step runs in a subshell under the parent's `set -e`, so a failed
# assertion ends that step and only that step: the remaining steps still run
# and the script still exits non-zero. Values a later step needs are handed
# forward through `state_put`/`state_load` (a file), because a subshell's
# variables do not survive it.
state_put() {
  local name="$1" value="$2"
  case "${value}" in
    *"'"*) err "state_put ${name}: value contains a single quote"; return 1 ;;
  esac
  printf "%s='%s'\n" "${name}" "${value}" >> "${VARS_FILE}"
  # The file carries the value to the *next* step (a new subshell). This binds
  # it for the rest of *this* one, which would otherwise hit `set -u`.
  eval "${name}=\"\${2}\""
}
state_load() { set -a; . "${VARS_FILE}"; set +a; }

pass() { printf 'PASS %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"; echo -e "${GREEN}[PASS $1]${NC} $2"; }
fail() {
  printf 'FAIL %-2s %s\n' "$1" "$2" >> "${RESULT_FILE}"
  echo -e "${RED}[FAIL $1]${NC} $2" >&2
  : >> "${WORKDIR}/failures"
  printf 'step %s\n' "$1" >> "${WORKDIR}/failures"
}

# Run one step and record its verdict.
#
# The subshell is NOT written as an `if` condition. Bash ignores `set -e` for
# every command inside a condition — including a subshell that re-runs
# `set -e` itself — so an `if ( set -e; step ); then` harness silently keeps
# going past a failed assertion and still reaches the step's own `pass` line.
# This script printed three such false passes before the shape below replaced
# it. Read the status out of `$?` instead, with errexit off in the parent only
# for the length of the call.
run_step() {
  local num="$1" desc="$2" fn="$3" started status
  started="$(date +%s)"
  log "── step ${num}: ${desc}"
  state_load
  set +e
  ( set -euo pipefail; "${fn}" )
  status=$?
  set -e
  [[ "${status}" -eq 0 ]] || fail "${num}" "${desc} — exit ${status}; see ${WORKDIR}"
  log "   step ${num} took $(( $(date +%s) - started ))s"
}

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}

# ── Shared python helpers ───────────────────────────────────────────────────
# The target-key encoding rule (`crates/beekeeper-sdk/src/builders.rs::
# coding_session_target_key`) and the event readers live here once, so no step
# re-implements them.
cat > "${WORKDIR}/helpers.py" <<'PY'
import json


def target_to_key(target):
    fields = [target["driver"], target["instanceId"], target["sessionId"], str(target["generation"])]
    return "coding-session/v1|" + "".join(f"{len(field.encode('utf-8'))}:{field}" for field in fields)


def load_json_file(path):
    with open(path, encoding="utf-8") as source:
        return json.load(source)


def receipts_for(events, command_id):
    """Every 44224 receipt naming `command_id`, oldest first."""
    rows = []
    for event in events:
        try:
            content = json.loads(event["content"])
        except (KeyError, TypeError, json.JSONDecodeError):
            continue
        if content.get("commandId") == command_id:
            rows.append((event, content))
    rows.sort(key=lambda pair: (pair[0].get("created_at", 0), pair[0]["id"]))
    return rows


def refusal_code(content):
    return (content.get("error") or {}).get("code")
PY

# BIP-340 Schnorr, from the BIP's own reference implementation.
#
# Used for exactly one thing: minting the NIP-OA owner attestation that lets
# `bee sessions create` **found an umbrella** (`crew_cmds.rs::
# prepare_creator_owner_governance`) — the only path in the CLI that publishes
# a kind:44226 genesis, and the thing every handover in this script is rooted
# at. Nothing is trusted on this module's say-so: `bee` runs
# `beekeeper_sdk::nip_oa::verify_auth_tag` over every tag before it signs anything,
# so a wrong signature here stops the run at the first `bee` call rather than
# silently weakening a step.
cat > "${WORKDIR}/bip340.py" <<'PY'
import hashlib
import json

P = 2**256 - 2**32 - 977
N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
G = (
    0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
    0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8,
)


def _tagged_hash(tag, msg):
    digest = hashlib.sha256(tag.encode()).digest()
    return hashlib.sha256(digest + digest + msg).digest()


def _point_add(p1, p2):
    if p1 is None:
        return p2
    if p2 is None:
        return p1
    if p1[0] == p2[0] and p1[1] != p2[1]:
        return None
    if p1 == p2:
        lam = (3 * p1[0] * p1[0] * pow(2 * p1[1], P - 2, P)) % P
    else:
        lam = ((p2[1] - p1[1]) * pow(p2[0] - p1[0], P - 2, P)) % P
    x3 = (lam * lam - p1[0] - p2[0]) % P
    return (x3, (lam * (p1[0] - x3) - p1[1]) % P)


def _point_mul(point, scalar):
    result = None
    for bit in range(256):
        if (scalar >> bit) & 1:
            result = _point_add(result, point)
        point = _point_add(point, point)
    return result


def _bytes_from_int(value):
    return value.to_bytes(32, byteorder="big")


def xonly_pubkey(seckey_hex):
    d0 = int(seckey_hex, 16)
    if not 1 <= d0 <= N - 1:
        raise ValueError("secret key out of range")
    return _bytes_from_int(_point_mul(G, d0)[0]).hex()


def sign(seckey_hex, msg32, aux32=b"\x00" * 32):
    d0 = int(seckey_hex, 16)
    if not 1 <= d0 <= N - 1:
        raise ValueError("secret key out of range")
    point = _point_mul(G, d0)
    d = d0 if point[1] % 2 == 0 else N - d0
    t = d ^ int.from_bytes(_tagged_hash("BIP0340/aux", aux32), "big")
    rand = _tagged_hash("BIP0340/nonce", _bytes_from_int(t) + _bytes_from_int(point[0]) + msg32)
    k0 = int.from_bytes(rand, "big") % N
    if k0 == 0:
        raise RuntimeError("nonce is zero")
    r_point = _point_mul(G, k0)
    k = k0 if r_point[1] % 2 == 0 else N - k0
    challenge = _tagged_hash(
        "BIP0340/challenge", _bytes_from_int(r_point[0]) + _bytes_from_int(point[0]) + msg32
    )
    e = int.from_bytes(challenge, "big") % N
    return (_bytes_from_int(r_point[0]) + _bytes_from_int((k + e * d) % N)).hex()


def auth_tag(owner_seckey_hex, agent_pubkey_hex, conditions=""):
    """A NIP-OA auth tag, byte-for-byte what `nip_oa::compute_auth_tag` builds."""
    preimage = f"nostr:agent-auth:{agent_pubkey_hex}:{conditions}"
    digest = hashlib.sha256(preimage.encode()).digest()
    return json.dumps(
        ["auth", xonly_pubkey(owner_seckey_hex), conditions, sign(owner_seckey_hex, digest)]
    )
PY

py() { PYTHONPATH="${WORKDIR}" python3 "$@"; }

# ── Scratch database (never the dev DB) ─────────────────────────────────────
log "creating scratch database ${DB_NAME}..."
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q -d postgres \
  -c "CREATE DATABASE ${DB_NAME};" >/dev/null

# ── Real relay binary, scratch DB, Redis logical DB 14 ──────────────────────
RELAY_PORT="$(free_port)"
RELAY_URL="ws://127.0.0.1:${RELAY_PORT}"
RELAY_HTTP="http://127.0.0.1:${RELAY_PORT}"
RELAY_KEY="$(openssl rand -hex 32)"

log "starting beekeeper-relay on ${RELAY_HTTP} (scratch DB, Redis DB 14, git scratch under ${WORKDIR})..."
DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/${DB_NAME}" \
  REDIS_URL="redis://localhost:6379/14" \
  RELAY_URL="${RELAY_URL}" \
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
RELAY_SELF="$(curl -s -H 'Accept: application/nostr+json' "${RELAY_HTTP}/" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["self"])')"
ok "relay ready at ${RELAY_HTTP} (self ${RELAY_SELF:0:16}…)"

export BEEKEEPER_RELAY_URL="${RELAY_HTTP}"

# ── Two owner identities, one attestation owner, two provider identities ────
# A and B are the two participants. `ATTEST_KEY` is the human owner whose
# NIP-OA attestation lets A's and B's `bee sessions create` found umbrellas;
# it never steers anything.
A_KEY="$(openssl rand -hex 32)"
B_KEY="$(openssl rand -hex 32)"
ATTEST_KEY="$(openssl rand -hex 32)"
PROVIDER_A_KEY="$(openssl rand -hex 32)"
PROVIDER_B_KEY="$(openssl rand -hex 32)"

A_HEX="$(py -c "import bip340,sys; print(bip340.xonly_pubkey(sys.argv[1]))" "${A_KEY}")"
B_HEX="$(py -c "import bip340,sys; print(bip340.xonly_pubkey(sys.argv[1]))" "${B_KEY}")"
A_TAG="$(py -c "import bip340,sys; print(bip340.auth_tag(sys.argv[1], sys.argv[2], ''))" "${ATTEST_KEY}" "${A_HEX}")"
B_TAG="$(py -c "import bip340,sys; print(bip340.auth_tag(sys.argv[1], sys.argv[2], ''))" "${ATTEST_KEY}" "${B_HEX}")"

bee_a()  { BEEKEEPER_PRIVATE_KEY="${A_KEY}" "${BEE_BIN}" "$@"; }
bee_b()  { BEEKEEPER_PRIVATE_KEY="${B_KEY}" "${BEE_BIN}" "$@"; }
# The git-capable forms: `handover checkpoint` and `handover continue` shell
# out to git, which reaches the relay through git-credential-nostr, which signs
# with $NOSTR_PRIVATE_KEY. Same identity, two env vars, because the CLI and the
# credential helper read different ones.
bee_a_git() { BEEKEEPER_PRIVATE_KEY="${A_KEY}" NOSTR_PRIVATE_KEY="${A_KEY}" "${BEE_BIN}" "$@"; }
bee_b_git() { BEEKEEPER_PRIVATE_KEY="${B_KEY}" NOSTR_PRIVATE_KEY="${B_KEY}" "${BEE_BIN}" "$@"; }

# ── Repository, project, channel ────────────────────────────────────────────
REPO_ID="handover-repo-$$"
PROJECT_SLUG="handover-project-$$"
INSTANCE="claude-primary"

log "announcing repository, project and channel as A..."
bee_a repos create --id "${REPO_ID}" > "${WORKDIR}/repo.json"
OWNER_FROM_LINK="$(py -c "
import json, sys
from urllib.parse import urlparse, parse_qs
print(parse_qs(urlparse(json.load(open(sys.argv[1]))['link']).query)['owner'][0])
" "${WORKDIR}/repo.json")"
[[ "${OWNER_FROM_LINK}" == "${A_HEX}" ]] || {
  err "the relay's repo link names owner ${OWNER_FROM_LINK}, but this script derived A as ${A_HEX}"
  exit 1
}
PROJECT="30621:${A_HEX}:${PROJECT_SLUG}"
REPO_COORD="30617:${A_HEX}:${REPO_ID}"
bee_a projects create "${PROJECT_SLUG}" --repo "${REPO_ID}" --access public >/dev/null

CHANNEL="$(bee_a channels create --name "handover-$$" --type stream --visibility open \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])')"
# The git transport authorizes on the repository's channel binding, so this is
# a prerequisite for every push below, not decoration.
bee_a repos bind --id "${REPO_ID}" --channel "${CHANNEL}" --project "${PROJECT}" >/dev/null

for key in "${B_KEY}" "${PROVIDER_A_KEY}" "${PROVIDER_B_KEY}"; do
  BEEKEEPER_PRIVATE_KEY="${key}" "${BEE_BIN}" channels join --channel "${CHANNEL}" >/dev/null
done
ok "fixtures ready: repo=${REPO_COORD} project=${PROJECT} channel=${CHANNEL}"

# ── Checkouts of the relay-hosted repository ────────────────────────────────
GIT_SCOPE="${RELAY_HTTP}/git"
GIT_REMOTE_URL="${RELAY_HTTP}/git/${A_HEX}/${REPO_ID}"

# One checkout wired to the relay's git transport: `origin` is the relay and
# NIP-98 is the credential. `credential.<scope>.helper` is reset to empty first
# so a system-wide helper (osxkeychain here) cannot answer ahead of ours — the
# same reset `bee git setup` writes.
init_checkout() {
  local dir="$1" who="$2"
  mkdir -p "${dir}"
  git -C "${dir}" init -q -b main
  git -C "${dir}" config user.email "${who}@handover.invalid"
  git -C "${dir}" config user.name "${who}"
  git -C "${dir}" config commit.gpgsign false
  git -C "${dir}" remote add origin "${GIT_REMOTE_URL}"
  git -C "${dir}" config "credential.${GIT_SCOPE}.helper" ""
  git -C "${dir}" config --add "credential.${GIT_SCOPE}.helper" "${CREDENTIAL_BIN}"
  git -C "${dir}" config "credential.${GIT_SCOPE}.useHttpPath" true
}

CHECKOUT_A="${WORKDIR}/checkout-a"
init_checkout "${CHECKOUT_A}" "owner-a"
printf 'handover acceptance fixture\n' > "${CHECKOUT_A}/README.md"
git -C "${CHECKOUT_A}" add -A
git -C "${CHECKOUT_A}" commit -q -m "initial commit"
NOSTR_PRIVATE_KEY="${A_KEY}" git -C "${CHECKOUT_A}" push -q --no-verify origin HEAD:refs/heads/main
BASE_SHA="$(git -C "${CHECKOUT_A}" rev-parse HEAD)"
ok "A's checkout pushed main ${BASE_SHA:0:12} to the relay over NIP-98"

# B's checkouts: one per reconstruction, each fetched from the relay so the
# reconstruction is a genuine fetch, not a local copy.
seed_from_relay() {
  local dir="$1" who="$2"
  init_checkout "${dir}" "${who}"
  NOSTR_PRIVATE_KEY="${B_KEY}" git -C "${dir}" fetch -q origin \
    "refs/heads/main:refs/remotes/origin/main"
  git -C "${dir}" checkout -q -B main refs/remotes/origin/main
}
# Two folders for B, deliberately different, because the seam this composition
# has to test is exactly the one a single folder hides. `B-default` is where
# B's provider is configured to run *anything* in this project and channel — a
# plain clone, nobody's handover. `B-work` is the checkout B recovers into with
# `--cwd`. If a reconstruction does not bind the create to `B-work`, the model
# opens in `B-default` and works on an untouched tree while the continuation
# says "recovered": a pre-seeded projects file pointing both names at one
# directory would report that as a pass.
CHECKOUT_B_DEFAULT="${WORKDIR}/checkout-b-default"
CHECKOUT_B="${WORKDIR}/checkout-b-work"
CHECKOUT_B9="${WORKDIR}/checkout-b9"
CHECKOUT_B10="${WORKDIR}/checkout-b10"
seed_from_relay "${CHECKOUT_B_DEFAULT}" "owner-b"
seed_from_relay "${CHECKOUT_B}" "owner-b"
seed_from_relay "${CHECKOUT_B9}" "owner-b"
seed_from_relay "${CHECKOUT_B10}" "owner-b"
ok "B's checkouts fetched ${BASE_SHA:0:12} from the relay"

# ── The ACP adapter both providers run (no model call) ──────────────────────
# `crates/beekeeper-session-provider/src/session.rs`'s `testing::GOOD_AGENT`
# technique: a real subprocess speaking JSON-RPC on stdio. Every raw
# session/new, session/load and session/prompt goes to $FABLE_ACP_REQUEST_LOG
# (so a step can assert on the exact bytes an execution was handed) and every
# method name to $FABLE_METHODS_LOG.
FAKE_AGENT="${WORKDIR}/fake-agent.sh"
cat > "${FAKE_AGENT}" <<'AGENT'
#!/bin/bash
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
      if [[ -n "$requested" ]] && grep -Fxq -- "$requested" "${FABLE_ACP_REQUEST_LOG}.cursors" 2>/dev/null; then
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
      # One line per prompt, index-aligned with the request log: the directory
      # this adapter *process* was spawned in. Recorded as context, never as
      # the answer to "where does the agent work" — ACP passes that as the
      # `cwd` field on `session/new`, which is what the assertions read.
      printf '%s\n' "$PWD" >> "${FABLE_CWD_LOG}"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}\n' "$SESSION_ID"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$LAST_PROMPT" ;;
  esac
done
AGENT
chmod +x "${FAKE_AGENT}"

runtimes_json() {
  py -c "
import json
print(json.dumps([{
  'instanceRef': '${INSTANCE}',
  'driver': 'claude-agent-acp',
  'runtime': 'claude',
  'agentCommand': 'bash',
  'agentArgs': ['${FAKE_AGENT}'],
  'defaultModel': 'default',
  'allowedModels': ['default'],
}]))
"
}
RUNTIMES_JSON="$(runtimes_json)"

STATE_DIR_A="${WORKDIR}/provider-state-a"
STATE_DIR_B="${WORKDIR}/provider-state-b"
mkdir -p "${STATE_DIR_A}" "${STATE_DIR_B}"
PROJECTS_A="${WORKDIR}/projects-a.json"
PROJECTS_B="${WORKDIR}/projects-b.json"
cat > "${PROJECTS_A}" <<EOF
{"version":1,"pending":{},"projects":{"${PROJECT}":"${CHECKOUT_A}"},"channels":{"${CHANNEL}":"${CHECKOUT_A}"}}
EOF
# B's projects file has a canonical shape kept beside it, because step 3
# rewrites the live file from it mid-reconstruction to imitate an unrelated
# desktop save. It never contains a pending entry: the CLI does not write one,
# and the hint it does write lives in `pending-hints/` precisely so a save like
# this cannot erase it.
PROJECTS_B_CANONICAL="${WORKDIR}/projects-b.canonical.json"
PENDING_HINTS_B="${WORKDIR}/pending-hints"
cat > "${PROJECTS_B_CANONICAL}" <<EOF
{"version":1,"pending":{},"projects":{"${PROJECT}":"${CHECKOUT_B_DEFAULT}"},"channels":{"${CHANNEL}":"${CHECKOUT_B_DEFAULT}"}}
EOF
cp "${PROJECTS_B_CANONICAL}" "${PROJECTS_B}"
ACP_LOG_A="${WORKDIR}/acp-a.jsonl"; : > "${ACP_LOG_A}"
ACP_LOG_B="${WORKDIR}/acp-b.jsonl"; : > "${ACP_LOG_B}"
METHODS_LOG_A="${WORKDIR}/methods-a.log"; : > "${METHODS_LOG_A}"
METHODS_LOG_B="${WORKDIR}/methods-b.log"; : > "${METHODS_LOG_B}"
CWD_LOG_A="${WORKDIR}/cwd-a.log"; : > "${CWD_LOG_A}"
CWD_LOG_B="${WORKDIR}/cwd-b.log"; : > "${CWD_LOG_B}"

# Spawn one provider generation. `LAST_PID` is the caller's handle; the pid is
# also appended to the pid file so cleanup finds it whichever subshell spawned
# it. Mirrors `spawn_provider` in scripts/ci-continuation-acceptance.sh.
spawn_provider() {
  local key="$1" state_dir="$2" projects_file="$3" runtimes_json="$4" \
        log_file="$5" methods_log="$6" acp_log="$7" cwd_log="$8"
  BEEKEEPER_PRIVATE_KEY="${key}" \
    BEEKEEPER_RELAY_URL="${RELAY_URL}" \
    BEEKEEPER_CSP_STATE_DIR="${state_dir}" \
    BEEKEEPER_CSP_PROJECTS_FILE="${projects_file}" \
    BEEKEEPER_CSP_RUNTIMES="${runtimes_json}" \
    FABLE_ACP_REQUEST_LOG="${acp_log}" \
    FABLE_METHODS_LOG="${methods_log}" \
    FABLE_CWD_LOG="${cwd_log}" \
    RUST_LOG=info \
    "${PROVIDER_BIN}" > "${log_file}" 2>&1 &
  LAST_PID=$!
  record_pid "${LAST_PID}"
}

# A just-spawned provider's own pubkey, from its startup log. tracing's ANSI
# styling splits "pubkey=" with an escape sequence, so the codes come off first.
wait_provider_pubkey() {
  local pid="$1" log="$2" pubkey
  for _ in $(seq 1 60); do
    if ! kill -0 "${pid}" 2>/dev/null; then
      err "provider process died during startup; log: ${log}"; cat "${log}" >&2; return 1
    fi
    sed 's/\x1b\[[0-9;]*m//g' "${log}" 2>/dev/null | grep -q 'pubkey=' && break
    sleep 1
  done
  pubkey="$(sed 's/\x1b\[[0-9;]*m//g' "${log}" | grep -o 'pubkey=[0-9a-f]\{64\}' | head -1 | cut -d= -f2)"
  [[ -n "${pubkey}" ]] || { err "could not read a provider pubkey from ${log}"; return 1; }
  echo "${pubkey}"
}

# Wait for a (re)started provider to reach the network. "witnessed relay
# identity" is the last plain-text startup milestone and precedes this provider
# observing any event. A restart publishes no new 44222 (csp::catalog skips an
# unchanged catalog), so this — not a catalog count — is what a restart waits on.
wait_provider_relay_connected() {
  local pid="$1" log="$2"
  for _ in $(seq 1 60); do
    if ! kill -0 "${pid}" 2>/dev/null; then
      err "provider process died during startup; log: ${log}"; cat "${log}" >&2; return 1
    fi
    grep -q 'witnessed relay identity' "${log}" 2>/dev/null && return 0
    sleep 1
  done
  err "${log} never logged 'witnessed relay identity' within 60s"; cat "${log}" >&2; return 1
}

wait_pid_exit() {
  local pid="$1"
  for _ in $(seq 1 120); do
    kill -0 "${pid}" 2>/dev/null || return 0
    sleep 0.5
  done
  err "pid ${pid} did not exit within 60s of kill -9"; return 1
}

# The channel's 44222 catalog count: a provider's FIRST-EVER spawn grows it, so
# this is how the initial spawns prove a completed discover/subscribe pass.
channel_catalog_count() {
  bee_a events query --kinds 44222 --channel "${CHANNEL}" \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'
}
wait_catalog_above() {
  local min_count="$1" count=0
  for _ in $(seq 1 45); do
    count="$(channel_catalog_count)"
    [[ "${count}" -gt "${min_count}" ]] && { echo "${count}"; return 0; }
    sleep 1
  done
  err "channel catalog count did not exceed ${min_count} within 45s (stayed at ${count})"
  return 1
}

acp_prompt_count() {
  py -c "
import json, sys
with open(sys.argv[1], encoding='utf-8') as source:
    print(sum(json.loads(line).get('method') == 'session/prompt' for line in source if line.strip()))
" "$1"
}

log "starting P_A and P_B (fake ACP adapters, no model)..."
CATALOG_BEFORE_A="$(channel_catalog_count)"
spawn_provider "${PROVIDER_A_KEY}" "${STATE_DIR_A}" "${PROJECTS_A}" "${RUNTIMES_JSON}" \
  "${WORKDIR}/provider-a.log" "${METHODS_LOG_A}" "${ACP_LOG_A}" "${CWD_LOG_A}"
PROVIDER_A_PID="${LAST_PID}"
PROVIDER_A_HEX="$(wait_provider_pubkey "${PROVIDER_A_PID}" "${WORKDIR}/provider-a.log")"
CATALOG_AFTER_A="$(wait_catalog_above "${CATALOG_BEFORE_A}")"

spawn_provider "${PROVIDER_B_KEY}" "${STATE_DIR_B}" "${PROJECTS_B}" "${RUNTIMES_JSON}" \
  "${WORKDIR}/provider-b.log" "${METHODS_LOG_B}" "${ACP_LOG_B}" "${CWD_LOG_B}"
PROVIDER_B_PID="${LAST_PID}"
PROVIDER_B_HEX="$(wait_provider_pubkey "${PROVIDER_B_PID}" "${WORKDIR}/provider-b.log")"
wait_catalog_above "${CATALOG_AFTER_A}" >/dev/null
[[ "${PROVIDER_A_HEX}" != "${PROVIDER_B_HEX}" ]] || { err "both providers came up on one key"; exit 1; }
ok "P_A live (${PROVIDER_A_HEX:0:16}…), P_B live (${PROVIDER_B_HEX:0:16}…)"

state_put A_HEX "${A_HEX}"
state_put B_HEX "${B_HEX}"
state_put PROVIDER_A_HEX "${PROVIDER_A_HEX}"
state_put PROVIDER_B_HEX "${PROVIDER_B_HEX}"
state_put PROVIDER_A_PID "${PROVIDER_A_PID}"
state_put PROVIDER_B_PID "${PROVIDER_B_PID}"

# ── Domain helpers ──────────────────────────────────────────────────────────

# Every event of `kinds` in the channel, as JSON on stdout, read as A.
q() { bee_a events query --kinds "$1" --channel "${CHANNEL}"; }

# Found a fresh umbrella on `provider_pubkey` as `who` (a|b); prints
# sessionRef, genesisRef and the confirmed cs-target. Founded by the product's
# own path: a bare `bee sessions create` under a NIP-OA attestation publishes
# the 44226 genesis, grants the attested owner `collaborator`, then creates.
found_umbrella() {
  local who="$1" provider_pubkey="$2" brief="$3" key tag out
  case "${who}" in
    a) key="${A_KEY}"; tag="${A_TAG}" ;;
    b) key="${B_KEY}"; tag="${B_TAG}" ;;
    *) err "found_umbrella: unknown identity ${who}"; return 1 ;;
  esac
  out="$(BEEKEEPER_PRIVATE_KEY="${key}" BEEKEEPER_AUTH_TAG="${tag}" "${BEE_BIN}" sessions create \
    --channel "${CHANNEL}" --provider-instance "${INSTANCE}" \
    --provider-authority "${provider_pubkey}" --project "${PROJECT}" --repo "${REPO_ID}" \
    --brief - --wait --timeout-secs 90 <<< "${brief}")"
  py -c "
import json, sys
created = json.loads(sys.argv[1])
for field in ('sessionRef', 'genesisRef', 'target'):
    value = created.get(field)
    assert value, f'sessions create returned no {field}: {created!r}'
    print(value)
assert created['outcome'] == 'created', created
" "${out}"
}

# Join one more execution to an existing umbrella and print its cs-target.
join_execution() {
  local session_ref="$1" genesis_ref="$2" provider_pubkey="$3" brief="$4" out
  out="$(bee_a sessions create --channel "${CHANNEL}" --session-ref "${session_ref}" \
    --genesis "${genesis_ref}" --provider-instance "${INSTANCE}" \
    --provider-authority "${provider_pubkey}" --project "${PROJECT}" --repo "${REPO_ID}" \
    --brief - --wait --timeout-secs 90 <<< "${brief}")"
  py -c "
import json, sys
created = json.loads(sys.argv[1])
assert created.get('target'), created
assert created['outcome'] == 'created', created
print(created['target'])
" "${out}"
}

# Publish one turn and print its commandId. Always `--no-wait`: the verdict is
# read from the provider's own receipt, so a refusal must not arrive as the
# CLI's exit code and kill the run before it can be inspected.
send_turn() {
  local who="$1" target="$2" text="$3" key out
  case "${who}" in a) key="${A_KEY}" ;; b) key="${B_KEY}" ;; esac
  if ! out="$(BEEKEEPER_PRIVATE_KEY="${key}" "${BEE_BIN}" sessions send --channel "${CHANNEL}" \
    --to "${target}" --content "${text}" --no-wait 2> "${WORKDIR}/send.err")"; then
    err "the relay refused the turn itself (no provider receipt will exist): $(cat "${WORKDIR}/send.err")"
    return 1
  fi
  py -c "
import json, sys
sent = json.loads(sys.argv[1])
assert sent.get('accepted') is True, sent
print(sent['commandId'])
" "${out}"
}

# Wait for the newest 44224 receipt naming `command_id` and print
# "<status>:<refusal code or empty>". Prints "none:" if none arrives.
wait_receipt() {
  local command_id="$1" deadline="${2:-45}" verdict
  for _ in $(seq 1 "${deadline}"); do
    verdict="$(q 44224 | py -c "
import json, sys, helpers
rows = helpers.receipts_for(json.load(sys.stdin), sys.argv[1])
if not rows:
    print('none:')
else:
    # turn_started/turn_refused wins over a turn_queued published in the same
    # second, which second-granularity timestamps cannot separate — the same
    # rule the CLI's own newest_stage_for applies.
    def rank(pair):
        event, content = pair
        terminal = 0 if content.get('status') == 'turn_queued' else 1
        return (terminal, event.get('created_at', 0))
    _, content = max(rows, key=rank)
    print(f\"{content.get('status')}:{helpers.refusal_code(content) or ''}\")
" "${command_id}")"
    [[ "${verdict}" != "none:" ]] && { echo "${verdict}"; return 0; }
    sleep 1
  done
  echo "none:"
}

# Assert that `command_id`'s newest receipt reads exactly `expected`
# ("<status>:<code>"), naming the target it answered for.
expect_receipt() {
  local command_id="$1" expected="$2" label="$3" verdict
  verdict="$(wait_receipt "${command_id}")"
  if [[ "${verdict}" != "${expected}" ]]; then
    err "${label}: command ${command_id} answered '${verdict}', expected '${expected}'"
    q 44224 | py -c "
import json, sys, helpers
for event, content in helpers.receipts_for(json.load(sys.stdin), sys.argv[1]):
    print(json.dumps({'signer': event['pubkey'], 'content': content}))
" "${command_id}" >&2
    return 1
  fi
}

# `bee sessions handover status --json` for one umbrella, as JSON on stdout.
handover_status() {
  local who="$1" session_ref="$2" genesis_ref="$3" out errfile
  errfile="${WORKDIR}/status-${session_ref:0:8}.err"
  case "${who}" in
    a) out="$(bee_a sessions handover status --channel "${CHANNEL}" --session-ref "${session_ref}" --genesis "${genesis_ref}" --json 2> "${errfile}")" || {
         err "bee sessions handover status (as A) failed: $(cat "${errfile}")"; return 1; } ;;
    b) out="$(bee_b sessions handover status --channel "${CHANNEL}" --session-ref "${session_ref}" --genesis "${genesis_ref}" --json 2> "${errfile}")" || {
         err "bee sessions handover status (as B) failed: $(cat "${errfile}")"; return 1; } ;;
  esac
  printf '%s\n' "${out}"
}

# Wait until no execution of `session_ref` reads `live`.
#
# The relay serves kind 24223 from a Redis snapshot with a 180s TTL
# (`beekeeper_pubsub::session_lease::SESSION_LEASE_TTL_SECS`), so a `kill -9`'d
# provider's execution keeps reading `live` for up to three minutes. Step 3
# asserts that the DEFAULT plan picks reconstruction *because* nothing is
# reachable, so it has to wait that snapshot out rather than force the mode.
wait_no_live_execution() {
  local session_ref="$1" genesis_ref="$2" live
  for _ in $(seq 1 240); do
    live="$(handover_status a "${session_ref}" "${genesis_ref}" | py -c "
import json, sys
fold = json.load(sys.stdin)
print(sum(1 for row in fold['executions'] if row['liveness'] == 'live'))
")"
    [[ "${live}" -eq 0 ]] && return 0
    sleep 2
  done
  err "executions of ${session_ref} still read live after 480s"
  return 1
}

# Wait until `target` reads `live` for `session_ref`.
#
# `--native` requires a live kind-24223 lease on the candidate's current
# generation, and a provider publishes leases on a 60s cadence
# (`beekeeper_session_provider::lease::LEASE_RENEWAL_INTERVAL`) — so an execution
# that was confirmed `created` seconds ago is not yet reachable by that rule.
# Waiting is the harness's job; refusing is the CLI's, and it is right to.
wait_execution_live() {
  local session_ref="$1" genesis_ref="$2" target="$3" live
  for _ in $(seq 1 60); do
    live="$(handover_status a "${session_ref}" "${genesis_ref}" | py -c "
import json, sys
rows = [row for row in json.load(sys.stdin)['executions'] if row['target'] == sys.argv[1]]
print(rows[0]['liveness'] if rows else 'absent')
" "${target}")"
    [[ "${live}" == "live" ]] && return 0
    sleep 2
  done
  err "${target} never read live within 120s (last: ${live}); --native cannot be exercised"
  return 1
}

# The physical path of a directory, so macOS's /tmp -> /private/tmp symlink
# never decides an assertion about where something ran.
real_path() { py -c "import os, sys; print(os.path.realpath(sys.argv[1]))" "$1"; }

# The directory the adapter *process* was spawned in for prompt `index`.
# Context only — see `session_cwd_for_prompt` for the directory that decides
# where an agent works.
prompt_process_cwd() {
  py -c "
import os, sys
with open(sys.argv[1], encoding='utf-8') as source:
    lines = [line.strip() for line in source if line.strip()]
index = int(sys.argv[2])
print(os.path.realpath(lines[index]) if index < len(lines) else 'not recorded')
" "$1" "$2"
}

# The working directory ACP told the adapter to use for prompt number `index`
# (0-based, index-aligned with the request log).
#
# **This is the `session/new` `cwd` parameter, not the child process's own
# directory.** ACP conveys the working directory as a field on the request; the
# adapter process itself legitimately runs wherever the provider does, and an
# earlier version of this helper read the stub's `$PWD` and reported the
# provider's directory as "where the work resumed" — a false alarm that would
# have been filed as a defect. The stub's `$PWD` is still recorded, and still
# reads as the provider's own directory, which is exactly why it is context and
# not evidence.
#
# The session ids are paired by order: the stub appends every minted id to
# `<request log>.cursors` in the same order it logs the `session/new` requests,
# so the Nth new pairs with the Nth cursor. A prompt is then resolved through
# its own `sessionId`, which also covers a native resume — where the prompt
# rides a session opened long before, by a different command.
session_cwd_for_prompt() {
  py -c "
import json, os, sys
log_path, index = sys.argv[1], int(sys.argv[2])
requests = []
with open(log_path, encoding='utf-8') as source:
    for line in source:
        line = line.strip()
        if line:
            requests.append(json.loads(line))
news = [r for r in requests if r.get('method') == 'session/new']
with open(log_path + '.cursors', encoding='utf-8') as source:
    cursors = [line.strip() for line in source if line.strip()]
assert len(cursors) >= len(news), (len(cursors), len(news))
opened = {cursor: new['params'].get('cwd') for new, cursor in zip(news, cursors)}
prompts = [r for r in requests if r.get('method') == 'session/prompt']
assert index < len(prompts), f'no prompt #{index} (have {len(prompts)})'
session_id = prompts[index]['params'].get('sessionId')
cwd = opened.get(session_id)
assert cwd, f'prompt #{index} names session {session_id!r}, which no session/new opened: {sorted(opened)}'
print(os.path.realpath(cwd))
" "$1" "$2"
}

# How many 44228 links this genesis's chain has right now — the count a refusal
# that happens *before* any claim must leave untouched.
authority_link_count() {
  bee_a events query --kinds 44228 --channel "${CHANNEL}" | py -c "
import json, sys
genesis = sys.argv[1]
print(sum(1 for event in json.load(sys.stdin)
          if ['csat-genesis', genesis] in event.get('tags', [])))
" "$1"
}

# Imitate the desktop rewriting `projects.json` for its own reasons, over and
# over, for as long as a reconstruction is running.
#
# This is the failure the hint file exists for: the CLI used to write
# `pending[commandId]` into `projects.json`, and any unrelated desktop save
# between that write and the provider's admission erased it — after which the
# create resolved to the project/channel entry and the model opened somewhere
# else, while the continuation said "recovered".
#
# Each pass writes the canonical (pending-free) content through a temp file and
# an atomic rename, so a provider reading concurrently never sees a torn file —
# a real desktop save would do the same, and a torn read would degrade every
# entry to "no mapping" and prove nothing. After each rename it checks whether a
# hint file still exists: if one does, this save definitively landed *before*
# the provider consumed it, which is the ordering the step needs. The count of
# such passes goes to the marker file.
start_desktop_saver() {
  local marker="$1"
  rm -f "${WORKDIR}/desktop-saver.stop"
  (
    overlaps=0
    while [[ ! -e "${WORKDIR}/desktop-saver.stop" ]]; do
      cp "${PROJECTS_B_CANONICAL}" "${PROJECTS_B}.saving"
      mv -f "${PROJECTS_B}.saving" "${PROJECTS_B}"
      for hint in "${PENDING_HINTS_B}"/*.json; do
        if [[ -e "${hint}" ]]; then
          overlaps=$(( overlaps + 1 ))
        fi
        break
      done
      sleep 0.01
    done
    printf '%s\n' "${overlaps}" > "${marker}"
  ) &
  DESKTOP_SAVER_PID=$!
  record_pid "${DESKTOP_SAVER_PID}"
}

# Stop the saver and wait for it to write its count.
stop_desktop_saver() {
  local marker="$1"
  : > "${WORKDIR}/desktop-saver.stop"
  wait "${DESKTOP_SAVER_PID}" 2>/dev/null || true
  local i
  for i in $(seq 1 100); do
    [[ -s "${marker}" ]] && return 0
    sleep 0.1
  done
  err "the simulated desktop save never reported how many passes it made"
  return 1
}

# Every commit id the relay's kind-30618 ref state currently names for the repo.
relay_ref_shas() {
  bee_a events query --kinds 30618 | py -c "
import json, sys
repo = sys.argv[1]
shas = set()
for event in json.load(sys.stdin):
    tags = event.get('tags', [])
    if ['d', repo] not in tags:
        continue
    for tag in tags:
        if len(tag) >= 2 and tag[0].startswith('refs/') and len(tag[1]) == 40:
            shas.add(tag[1].lower())
print('\n'.join(sorted(shas)))
" "${REPO_ID}"
}

# Re-deliver one already-accepted event, verbatim, at POST /events — the same
# path the WebSocket uses, and the only honest way to replay signed bytes.
replay_event() {
  local event_id="$1" body
  body="$(bee_a events query --kinds "$2" --channel "${CHANNEL}" --ids "${event_id}" \
    | py -c "
import json, sys
events = json.load(sys.stdin)
assert len(events) == 1, events
event = events[0]
assert 'sig' in event and event['sig'], 'the relay returned an unsigned row; nothing to replay'
print(json.dumps(event))
")"
  # `X-Pubkey` is the relay's dev-mode bridge auth, live because this relay
  # runs with BEEKEEPER_REQUIRE_AUTH_TOKEN=false. It authenticates the *caller*, not
  # the event: the bytes being replayed keep their original signature, which is
  # the whole point of the step.
  curl -s -o "${WORKDIR}/replay-${event_id:0:12}.json" -w '%{http_code}' \
    -X POST "${RELAY_HTTP}/events" -H 'Content-Type: application/json' \
    -H "x-pubkey: ${B_HEX}" -d "${body}"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 1 — A founds the umbrella on P_A, grants B, opens a sibling, sends a turn
# ═══════════════════════════════════════════════════════════════════════════
step_1() {
  local lines target_a target_a2 grant_json turn_cmd
  lines="$(found_umbrella a "${PROVIDER_A_HEX}" "carry the handover acceptance work")"
  S1="$(sed -n 1p <<< "${lines}")"
  G1="$(sed -n 2p <<< "${lines}")"
  target_a="$(sed -n 3p <<< "${lines}")"
  state_put S1 "${S1}"; state_put G1 "${G1}"; state_put TARGET_A "${target_a}"

  grant_json="$(bee_a sessions grant --channel "${CHANNEL}" --genesis "${G1}" \
    --pubkey "${B_HEX}" --role collaborator)"
  py -c "
import json, sys
granted = json.loads(sys.argv[1])
assert granted.get('accepted') is True, granted
" "${grant_json}"

  # The sibling execution step 5 fences: same umbrella, same provider, never
  # named by any claim.
  target_a2="$(join_execution "${S1}" "${G1}" "${PROVIDER_A_HEX}" "second execution under the same umbrella")"
  state_put TARGET_A2 "${target_a2}"

  turn_cmd="$(send_turn a "${target_a}" "start on the fold")"
  expect_receipt "${turn_cmd}" "turn_started:" "step 1"

  # Read every claim back off signed events rather than off the CLI's word:
  # the `created` receipts must be P_A's, and must name these exact targets.
  q 44224 | py -c "
import json, sys, helpers
events = json.load(sys.stdin)
provider, target_a, target_a2, turn_cmd = sys.argv[1:]
created = {}
for event in events:
    content = json.loads(event['content'])
    if content.get('status') != 'created':
        continue
    assert event['pubkey'] == provider, event
    created[helpers.target_to_key(content['session'])] = event['id']
for target in (target_a, target_a2):
    assert target in created, f'no created receipt from P_A for {target}: {sorted(created)}'
started = [
    (event, content)
    for event, content in helpers.receipts_for(events, turn_cmd)
    if content.get('status') == 'turn_started'
]
assert len(started) == 1, started
assert started[0][0]['pubkey'] == provider, started
assert helpers.target_to_key(started[0][1]['session']) == target_a, started
" "${PROVIDER_A_HEX}" "${target_a}" "${target_a2}" "${turn_cmd}"

  # The grant is a fact on the accepted chain, not a CLI claim.
  bee_a sessions roster --channel "${CHANNEL}" --genesis "${G1}" | py -c "
import json, sys
roster = json.load(sys.stdin)
grants = roster.get('grants') or {}
holders = grants if isinstance(grants, dict) else {row['pubkey']: row for row in grants}
assert sys.argv[1] in holders, roster
" "${B_HEX}"

  pass 1 "A founded umbrella ${S1} on P_A (E_A ${target_a: -18}, sibling E_A2 ${target_a2: -18}), granted B collaborator on the accepted chain, and one turn reached E_A (turn_started from P_A)"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 2 — A checkpoints a dirty worktree; then one over the per-file bound
# ═══════════════════════════════════════════════════════════════════════════
step_2() {
  local head_sha checkpoint_json checkpoint_id patch_event partial_json

  # A tracked, committed edit — the thing the wip ref carries.
  printf 'handover acceptance fixture\ntracked edit made by A\n' > "${CHECKOUT_A}/README.md"
  git -C "${CHECKOUT_A}" add README.md
  git -C "${CHECKOUT_A}" commit -q -m "tracked edit by A"
  head_sha="$(git -C "${CHECKOUT_A}" rev-parse HEAD)"
  state_put HEAD_SHA_A "${head_sha}"

  # …and the four dirty kinds the capture has to carry.
  printf 'staged by A, never committed\n' > "${CHECKOUT_A}/staged.txt"
  git -C "${CHECKOUT_A}" add staged.txt
  printf 'handover acceptance fixture\ntracked edit made by A\nunstaged edit by A\n' > "${CHECKOUT_A}/README.md"
  printf 'untracked note from A\n' > "${CHECKOUT_A}/untracked.txt"
  head -c 2048 /dev/urandom > "${CHECKOUT_A}/binary.bin"
  shasum -a 256 "${CHECKOUT_A}/binary.bin" | cut -d' ' -f1 > "${WORKDIR}/binary-sha.txt"

  checkpoint_json="$(bee_a_git sessions handover checkpoint --channel "${CHANNEL}" \
    --session-ref "${S1}" --genesis "${G1}" --cwd "${CHECKOUT_A}" --repo "${REPO_ID}" \
    --task "finish the handover fold and prove it end to end" \
    --next "run the composition and read the fold back off the relay" \
    --unresolved "whether the sibling execution should say so in its own metadata" \
    --decision "${G1}:the umbrella is the unit of handover" \
    --json)"
  printf '%s\n' "${checkpoint_json}" > "${WORKDIR}/checkpoint-1.json"
  checkpoint_id="$(py -c "import json,sys; print(json.loads(sys.argv[1])['eventId'])" "${checkpoint_json}")"
  state_put CHECKPOINT_1 "${checkpoint_id}"

  # 1. The wip ref is on the relay: the relay's own 30618 names the sha.
  relay_ref_shas > "${WORKDIR}/relay-shas.txt"
  grep -Fxq "${head_sha}" "${WORKDIR}/relay-shas.txt" || {
    err "the relay's 30618 ref state does not name ${head_sha}"
    cat "${WORKDIR}/relay-shas.txt" >&2
    return 1
  }

  # 2. The checkpoint as the relay stored it, not as the CLI printed it.
  q 44247 | py -c "
import json, sys
events = json.load(sys.stdin)
checkpoint_id, author, head_sha, session_ref, genesis_ref, sink_path = sys.argv[1:]
stored = [event for event in events if event['id'] == checkpoint_id]
assert len(stored) == 1, f'the relay has no 44247 {checkpoint_id}'
event = stored[0]
assert event['pubkey'] == author, event
tags = {tag[0]: tag[1] for tag in event['tags'] if len(tag) >= 2}
assert tags['d'] == session_ref and tags['csh-genesis'] == genesis_ref, event['tags']
assert tags['csh-type'] == 'checkpoint' and tags['csh-v'] == 'csh1', event['tags']
payload = json.loads(event['content'])
body = payload['body']
assert body['revision']['preserved'] == 'all', body['revision']
assert body['revision']['headSha'] == head_sha, body['revision']
assert body['revision']['dirty'] is True, body['revision']
wip = [a for a in body['artifacts'] if a['kind'] == 'wip-ref']
patch = [a for a in body['artifacts'] if a['kind'] == 'patch']
assert len(wip) == 1 and wip[0]['sha'] == head_sha, body['artifacts']
assert wip[0]['ref'].startswith('refs/heads/wip/'), wip
assert len(patch) == 1 and patch[0]['eventId'], body['artifacts']
assert body['missing'] == [], body['missing']
assert 'handover fold' in body['task'], body['task']
with open(sink_path, 'w', encoding='utf-8') as sink:
    sink.write(patch[0]['eventId'])
" "${checkpoint_id}" "${A_HEX}" "${head_sha}" "${S1}" "${G1}" "${WORKDIR}/patch-event-id.txt"
  patch_event="$(cat "${WORKDIR}/patch-event-id.txt")"
  state_put PATCH_EVENT_1 "${patch_event}"

  # 3. The four dirty kinds are inside the patch bytes the relay is holding.
  bee_a events query --kinds 1617 --ids "${patch_event}" | py -c "
import json, sys
events = json.load(sys.stdin)
assert len(events) == 1, events
patch = events[0]['content']
for needle in ('staged.txt', 'untracked.txt', 'binary.bin', 'unstaged edit by A'):
    assert needle in patch, f'{needle!r} is not in the published patch'
assert 'GIT binary patch' in patch, 'the binary file was not carried as binary content'
"

  # 4. A second checkpoint over an UNCHANGED tree, deliberately back to back.
  #    Two captures of the same tree against the same base sign the same patch
  #    bytes and, inside one second, the same NIP-34 event id — so the relay
  #    answers "duplicate". That is the relay confirming the artifact is there,
  #    not a failure, and the record must keep it: a checkpoint reporting
  #    `preserved: none` over bytes that are on the relay sends the next
  #    participant looking for work that is not lost.
  #
  #    Retried until the two land in one second, because a pair that crosses a
  #    second boundary signs two different ids and exercises nothing. Every
  #    attempt is a real checkpoint that supersedes the one before it, so the
  #    chain stays well formed however many it takes.
  local repeat_json repeat_id repeat_patch prev_id prev_patch before_repeat_id duplicate_seen attempt
  prev_id="${checkpoint_id}"
  prev_patch="${patch_event}"
  before_repeat_id="${checkpoint_id}"
  duplicate_seen=0
  for attempt in 1 2 3 4 5 6 7 8; do
    repeat_json="$(bee_a_git sessions handover checkpoint --channel "${CHANNEL}" \
      --session-ref "${S1}" --genesis "${G1}" --cwd "${CHECKOUT_A}" --repo "${REPO_ID}" \
      --task "the same work, checkpointed twice in one second" \
      --next "the second record must still name the patch it built" --json)"
    printf '%s\n' "${repeat_json}" > "${WORKDIR}/checkpoint-repeat.json"
    repeat_id="$(py -c "import json,sys; print(json.loads(sys.argv[1])['eventId'])" "${repeat_json}")"
    # Asserted on every attempt, duplicate or not: a repeat must never drop the
    # patch it built, and must name the record it replaces.
    repeat_patch="$(py -c "
import json, sys
body = json.loads(sys.argv[1])['checkpoint']
patch = [a for a in body['artifacts'] if a['kind'] in ('patch', 'blob')]
assert patch, ('the repeat checkpoint kept no patch artifact', body['artifacts'], body['missing'])
assert body['revision']['preserved'] == 'all', (body['revision'], body['missing'])
assert not any('already accepted' in line for line in body['missing']), body['missing']
assert body['prevCheckpointRef'] == sys.argv[2], (body['prevCheckpointRef'], sys.argv[2])
print(patch[0].get('eventId') or patch[0].get('hash'))
" "${repeat_json}" "${prev_id}")"
    before_repeat_id="${prev_id}"
    if [[ "${repeat_patch}" == "${prev_patch}" ]]; then
      duplicate_seen=1
      log "attempt ${attempt}: the repeat re-signed the same patch id ${repeat_patch:0:16}… (the relay saw a duplicate) and kept it as its own artifact"
      break
    fi
    prev_id="${repeat_id}"
    prev_patch="${repeat_patch}"
  done
  state_put CHECKPOINT_REPEAT "${repeat_id}"
  [[ "${duplicate_seen}" -eq 1 ]] || \
    log "no two consecutive checkpoints landed in one second in 8 attempts, so the duplicate-patch case was not exercised this run"

  # 5. One file over the per-file bound: preserved drops to partial and the
  #    path is named, not silently dropped. The README edit alongside it keeps
  #    this capture's bytes different from the previous one's, so the outcome
  #    turns on the bound and never on an event-id collision.
  #
  #    Deliberately NOT spaced by a second: all three checkpoints landing
  #    inside one second is the case `prevCheckpointRef` exists for, and step 3
  #    then reconstructs from whatever the fold names latest.
  head -c 307200 /dev/urandom > "${CHECKOUT_A}/oversize.bin"
  printf 'handover acceptance fixture\ntracked edit made by A\nunstaged edit by A\nsecond unstaged edit by A\n' > "${CHECKOUT_A}/README.md"
  partial_json="$(bee_a_git sessions handover checkpoint --channel "${CHANNEL}" \
    --session-ref "${S1}" --genesis "${G1}" --cwd "${CHECKOUT_A}" --repo "${REPO_ID}" \
    --task "same work, one file too large to carry" \
    --next "reconstruct without oversize.bin" --json)"
  printf '%s\n' "${partial_json}" > "${WORKDIR}/checkpoint-2.json"
  py -c "
import json, sys
body = json.loads(sys.argv[1])['checkpoint']
assert body['revision']['preserved'] == 'partial', (body['revision'], body['missing'])
assert any('oversize.bin' in line for line in body['missing']), body['missing']
assert [a for a in body['artifacts'] if a['kind'] == 'patch'], body['artifacts']
assert body['prevCheckpointRef'] == sys.argv[2], body['prevCheckpointRef']
" "${partial_json}" "${repeat_id}"
  local newest_id
  newest_id="$(py -c "import json,sys; print(json.loads(sys.argv[1])['eventId'])" "${partial_json}")"
  state_put CHECKPOINT_2 "${newest_id}"
  # The oversize file is fixture, not work: leave the tree as the
  # reconstruction is meant to find it.
  rm -f "${CHECKOUT_A}/oversize.bin"

  # 6. Which of three checkpoints is "latest" is answered by the author's own
  #    `prevCheckpointRef` chain, not by a clock and not by a hash. The two it
  #    replaces stay listed as history, each naming its successor, so a reader
  #    can follow the chain rather than guess.
  local status_json same_second
  status_json="$(handover_status a "${S1}" "${G1}")"
  same_second="$(py -c "
import json, sys
fold = json.loads(sys.argv[1])['fold']
first_id, before_id, repeat_id, newest_id = sys.argv[2:]
by_id = {c['eventId']: c for c in fold['checkpoints']}
for name, wanted in (('first', first_id), ('predecessor', before_id),
                     ('repeat', repeat_id), ('newest', newest_id)):
    assert wanted in by_id, f'the fold does not list the {name} checkpoint {wanted}'
assert fold['latestAuthorizedCheckpoint'] == newest_id, (
    'the fold named', fold['latestAuthorizedCheckpoint'], 'latest, not the newest link', newest_id)
assert by_id[newest_id]['standing'] == 'authorized', by_id[newest_id]
assert by_id[newest_id]['supersededBy'] is None, by_id[newest_id]
# The two records the newest one chains back through are history, each naming
# its own successor, so a reader follows references rather than guessing.
for older, successor in ((before_id, repeat_id), (repeat_id, newest_id)):
    entry = by_id[older]
    assert entry['standing'] == 'superseded', (older, entry['standing'])
    assert entry['supersededBy'] == successor, (older, entry['supersededBy'], successor)
assert by_id[first_id]['body']['prevCheckpointRef'] is None, by_id[first_id]['body']
stamps = {by_id[i]['createdAt'] for i in (before_id, repeat_id, newest_id)}
print('yes' if len(stamps) == 1 else 'no')
" "${status_json}" "${checkpoint_id}" "${before_repeat_id}" "${repeat_id}" "${newest_id}")"
  if [[ "${same_second}" == "yes" ]]; then
    log "the last three checkpoints share one created_at, and the fold still ordered them by reference"
  else
    log "the three checkpoints span more than one second, so the reference chain was not the only thing ordering them this run"
  fi

  pass 2 "checkpoint ${checkpoint_id:0:12}… landed wip ref at ${head_sha:0:12} (named by the relay's own 30618), carried staged+unstaged+untracked+binary in patch ${patch_event:0:12}… with preserved=all; a repeat over the unchanged tree kept its patch artifact at preserved=all; a 300 KiB file drove a third to preserved=partial naming oversize.bin; and the fold named the newest link ${newest_id:0:12}… latest with the two it replaces listed as superseded, each naming its successor"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 3 — P_A dies; B continues, and the default plan reconstructs
# ═══════════════════════════════════════════════════════════════════════════
step_3() {
  local baseline_a baseline_b continue_json takeover_id target_b branch checked_out

  baseline_a="$(acp_prompt_count "${ACP_LOG_A}")"
  baseline_b="$(acp_prompt_count "${ACP_LOG_B}")"
  state_put ACP_BASELINE_A "${baseline_a}"

  log "kill -9 P_A (pid ${PROVIDER_A_PID})"
  kill -9 "${PROVIDER_A_PID}"
  wait_pid_exit "${PROVIDER_A_PID}"
  log "waiting out the relay's 180s kind-24223 lease snapshot so nothing reads live…"
  wait_no_live_execution "${S1}" "${G1}"

  # ── the negative probe, first, while the chain is still untouched ────────
  # A reconstruction that cannot bind the create to `--cwd` would open the
  # model in whatever folder the provider is configured for — here a plain
  # clone with none of the recovered work — while the continuation says
  # "recovered". That must be refused, and refused *before* a claim: a run that
  # takes the session over and then discovers it cannot place the work has
  # already fenced the absent participant for nothing.
  local links_before links_after probe_exit
  links_before="$(authority_link_count "${G1}")"
  set +e
  env -u BEEKEEPER_CSP_PROJECTS_FILE \
    BEEKEEPER_PRIVATE_KEY="${B_KEY}" NOSTR_PRIVATE_KEY="${B_KEY}" "${BEE_BIN}" \
    sessions handover continue --channel "${CHANNEL}" --session-ref "${S1}" \
    --genesis "${G1}" --cwd "${CHECKOUT_B}" --body "${PROVIDER_B_HEX}" \
    --provider-instance "${INSTANCE}" --remote origin --wait-secs 120 --json \
    > "${WORKDIR}/continue-no-projects.json" 2> "${WORKDIR}/continue-no-projects.err"
  probe_exit=$?
  set -e
  [[ "${probe_exit}" -ne 0 ]] || {
    err "continue reconstructed with no projects file to bind --cwd to; the model would have run in the provider's default folder"
    cat "${WORKDIR}/continue-no-projects.json" >&2
    return 1
  }
  grep -Fq -- "--projects-file" "${WORKDIR}/continue-no-projects.err" || {
    err "the refusal does not name the remedy (--projects-file):"
    cat "${WORKDIR}/continue-no-projects.err" >&2
    return 1
  }
  links_after="$(authority_link_count "${G1}")"
  [[ "${links_before}" == "${links_after}" ]] || {
    err "the refused run still extended the authority chain (${links_before} -> ${links_after} links): it claimed the session and then refused to place the work"
    return 1
  }

  # ── the real run, under a desktop that keeps saving ─────────────────────
  # No --native, no --reconstruct: the point is that the DEFAULT plan picks
  # reconstruction because nothing is reachable. `--projects-file` is the seam
  # the create is bound through; B-work is NOT where this provider would
  # otherwise run anything in this channel. Throughout, the desktop is
  # rewriting `projects.json` from its own canonical state — the thing that
  # used to erase the binding.
  local projects_before projects_after saver_marker overlaps
  projects_before="$(shasum -a 256 "${PROJECTS_B}" | cut -d' ' -f1)"
  saver_marker="${WORKDIR}/desktop-saver.count"
  start_desktop_saver "${saver_marker}"
  continue_json="$(bee_b_git sessions handover continue --channel "${CHANNEL}" \
    --session-ref "${S1}" --genesis "${G1}" --cwd "${CHECKOUT_B}" \
    --body "${PROVIDER_B_HEX}" --provider-instance "${INSTANCE}" \
    --projects-file "${PROJECTS_B}" \
    --remote origin --wait-secs 120 --json)"
  stop_desktop_saver "${saver_marker}"
  overlaps="$(cat "${saver_marker}")"
  projects_after="$(shasum -a 256 "${PROJECTS_B}" | cut -d' ' -f1)"
  printf '%s\n' "${continue_json}" > "${WORKDIR}/continue-1.json"
  py -c "
import json, sys
out = json.loads(sys.argv[1])
assert out['type'] == 'continuation' and out['accepted'] is True, out
assert out['continuation']['mode'] == 'reconstructed', out['continuation']
assert out['continuation']['recovered'], out['continuation']
" "${continue_json}"
  target_b="$(py -c "
import json, sys, helpers
print(helpers.target_to_key(json.loads(sys.argv[1])['continuation']['target']))
" "${continue_json}")"
  state_put TARGET_B "${target_b}"
  state_put CONTINUATION_1 "$(py -c "import json,sys; print(json.loads(sys.argv[1])['eventId'])" "${continue_json}")"

  # 1. The takeover is on the accepted chain, with the relay's own receipt
  #    carrying the body it fenced to.
  takeover_id="$(q 40099 | py -c "
import json, sys
events = json.load(sys.stdin)
relay_self, genesis_ref, claimant, body = sys.argv[1:]
rows = []
for event in events:
    if event['pubkey'] != relay_self:
        continue
    content = json.loads(event['content'])
    if content.get('type') != 'coding_session_authority_transition_accepted':
        continue
    if content.get('genesisRef') != genesis_ref or content.get('transitionType') != 'takeover':
        continue
    rows.append(content)
assert len(rows) == 1, f'expected exactly one accepted takeover receipt, found {rows!r}'
assert rows[0]['granteePubkey'] == claimant, rows[0]
assert rows[0].get('bodyPubkey') == body, rows[0]
print(rows[0]['acceptedEventId'])
" "${RELAY_SELF}" "${G1}" "${B_HEX}" "${PROVIDER_B_HEX}")"
  state_put TAKEOVER_1 "${takeover_id}"

  # 2. B's checkout really is at the checkpoint's head with the dirty bytes back.
  checked_out="$(git -C "${CHECKOUT_B}" rev-parse HEAD)"
  [[ "${checked_out}" == "${HEAD_SHA_A}" ]] || {
    err "B's checkout is at ${checked_out}, not the checkpoint's ${HEAD_SHA_A}"; return 1
  }
  branch="$(git -C "${CHECKOUT_B}" rev-parse --abbrev-ref HEAD)"
  # The contract's name (§4): one session, one branch, so a second
  # reconstruction lands where the first one did instead of scattering a
  # branch per head sha.
  [[ "${branch}" == "handover/${S1:0:8}" ]] || {
    err "B's checkout is on branch '${branch}', not the session's handover/${S1:0:8}"; return 1
  }
  grep -q 'second unstaged edit by A' "${CHECKOUT_B}/README.md" || { err "the unstaged edit did not come back"; return 1; }
  grep -q 'tracked edit made by A' "${CHECKOUT_B}/README.md" || { err "the tracked edit did not come back"; return 1; }
  [[ -f "${CHECKOUT_B}/staged.txt" ]] || { err "the staged file did not come back"; return 1; }
  [[ -f "${CHECKOUT_B}/untracked.txt" ]] || { err "the untracked file did not come back"; return 1; }
  [[ -f "${CHECKOUT_B}/binary.bin" ]] || { err "the binary file did not come back"; return 1; }
  [[ "$(shasum -a 256 "${CHECKOUT_B}/binary.bin" | cut -d' ' -f1)" == "$(cat "${WORKDIR}/binary-sha.txt")" ]] \
    || { err "binary.bin came back with different bytes"; return 1; }

  # 3. The reconstruction is a real join on P_B: same sessionRef/genesisRef, a
  #    `created` receipt from P_B, and exactly one new ACP prompt carrying the
  #    checkpoint's own task and next action.
  local create_command_id
  create_command_id="$(q 44221 | py -c "
import json, sys
events = json.load(sys.stdin)
author, session_ref, genesis_ref, provider_b = sys.argv[1:]
creates = []
for event in events:
    if event['pubkey'] != author:
        continue
    payload = json.loads(event['content'])
    action = payload.get('action', {})
    if action.get('type') != 'session.create':
        continue
    creates.append((event, payload, action))
joined = [
    row for row in creates
    if row[2].get('sessionRef') == session_ref and row[2].get('genesisRef') == genesis_ref
]
assert len(joined) == 1, f'expected one session.create by B joining {session_ref}, found {len(joined)}'
assert joined[0][2]['providerAuthorityPubkey'] == provider_b, joined[0][2]
print(joined[0][1]['commandId'])
" "${B_HEX}" "${S1}" "${G1}" "${PROVIDER_B_HEX}")"

  # 3a. The binding survived a desktop that would not stop saving, and the
  # provider consumed it.
  #
  #   - the save landed at least once while the hint was still on disk, so the
  #     ordering this exists to test actually occurred;
  #   - `projects.json` is byte-identical across the whole reconstruction: the
  #     CLI never touched it, which is why the save could not undo anything;
  #   - the hint file is gone now, consumed on admission rather than left
  #     lying around binding some future create nobody asked it to.
  [[ "${overlaps}" -ge 1 ]] || {
    err "the simulated desktop save never landed while a pending hint existed (${overlaps} overlapping passes), so this run did not exercise the race"
    return 1
  }
  [[ "${projects_before}" == "${projects_after}" ]] || {
    err "projects.json changed across the reconstruction (${projects_before:0:12} -> ${projects_after:0:12}); the CLI must never write to it"
    return 1
  }
  [[ ! -e "${PENDING_HINTS_B}/${create_command_id}.json" ]] || {
    err "the hint ${PENDING_HINTS_B}/${create_command_id}.json is still live after the created receipt; a one-shot hint must be spent"
    cat "${PENDING_HINTS_B}/${create_command_id}.json" >&2
    return 1
  }
  # Spent, not vanished: the provider renames the hint to a `.consumed` marker
  # once the record is persisted, and that marker is the durable answer to
  # "which directory did this create actually get, and which execution came of
  # it". An empty directory would be the wrong assertion — it would pass just as
  # well if the hint had been deleted without ever being honoured.
  [[ -e "${PENDING_HINTS_B}/${create_command_id}.consumed" ]] || {
    err "no consumed marker at ${PENDING_HINTS_B}/${create_command_id}.consumed: the hint is gone with nothing recording what it resolved to"
    ls -la "${PENDING_HINTS_B}" >&2
    return 1
  }
  py -c "
import json, os, sys
path, command_id, expected, target = sys.argv[1:]
with open(path, encoding='utf-8') as source:
    marker = json.load(source)
assert marker['commandId'] == command_id, marker
got = os.path.realpath(marker['path'])
assert got == os.path.realpath(expected), (got, os.path.realpath(expected))
session_id = marker.get('sessionId')
assert session_id, ('the marker records no execution for a create that produced one', marker)
assert session_id in target, (
    'the marker names an execution that is not the reconstructed one', session_id, target)
" "${PENDING_HINTS_B}/${create_command_id}.consumed" "${create_command_id}" "${CHECKOUT_B}" "${target_b}"
  # …and the fallbacks still point somewhere else, or none of this proves
  # anything about where the work went.
  py -c "
import json, os, sys
path, recovered = sys.argv[1:]
with open(path, encoding='utf-8') as source:
    projects = json.load(source)
assert not (projects.get('pending') or {}), (
    'the projects file carries a pending entry; the CLI is still writing to it', projects['pending'])
target = os.path.realpath(recovered)
for key in ('projects', 'channels'):
    for value in (projects.get(key) or {}).values():
        assert os.path.realpath(value) != target, (
            f'{key} already points at the recovered checkout, so no hint was ever needed')
" "${PROJECTS_B}" "${CHECKOUT_B}"

  q 44224 | py -c "
import json, sys, helpers
events = json.load(sys.stdin)
provider_b, target_b = sys.argv[1:]
for_target = [
    (event, json.loads(event['content'])) for event in events
    if helpers.target_to_key(json.loads(event['content'])['session']) == target_b
]
created = [event for event, content in for_target if content.get('status') == 'created']
if len(created) != 1:
    # Print every receipt this execution got. created_with_failed_initial_turn
    # can carry HANDOVER_FENCED or AUTHORITY_NOT_REVERIFIED when a claim lands
    # during adapter startup, and the status alone would not say which.
    # (No backticks in here: this block lives inside a double-quoted shell
    # string, where they would be command substitution.)
    for event, content in for_target:
        print(json.dumps({'id': event['id'], 'signer': event['pubkey'], 'content': content}),
              file=sys.stderr)
assert len(created) == 1, f'expected exactly one created receipt for {target_b}, found {len(created)}'
assert created[0]['pubkey'] == provider_b, created[0]
" "${PROVIDER_B_HEX}" "${target_b}"

  local after_b task next_action delivered_cwd
  after_b="$(acp_prompt_count "${ACP_LOG_B}")"
  [[ "$(( after_b - baseline_b ))" -eq 1 ]] || {
    err "P_B's ACP log gained $(( after_b - baseline_b )) prompts, expected exactly 1"; return 1
  }

  # 3b. The directory ACP handed the adapter for this turn. B-default is a
  # real, plausible, wrong answer sitting right next to it — the answer this
  # provider would have given for anything else in this channel.
  delivered_cwd="$(session_cwd_for_prompt "${ACP_LOG_B}" "${baseline_b}")"
  [[ "${delivered_cwd}" == "$(real_path "${CHECKOUT_B}")" ]] || {
    err "the reconstruction's session was opened on ${delivered_cwd}, not the recovered checkout $(real_path "${CHECKOUT_B}")"
    [[ "${delivered_cwd}" == "$(real_path "${CHECKOUT_B_DEFAULT}")" ]] \
      && err "  — that is B's default folder for this channel: the create was never bound to --cwd"
    return 1
  }
  log "the reconstructed session opened on ${delivered_cwd}; its adapter *process* runs in $(prompt_process_cwd "${CWD_LOG_B}" "${baseline_b}"), which is the provider's own directory and not the seam"
  # The task and next action come from the checkpoint the FOLD names latest —
  # the product's own selection, which is the thing a reconstruction is
  # supposed to follow. Both are asserted non-empty first: an empty task would
  # make `assert task in text` pass against anything.
  local latest_checkpoint
  latest_checkpoint="$(handover_status a "${S1}" "${G1}" | py -c "
import json, sys
fold = json.load(sys.stdin)['fold']
latest = fold['latestAuthorizedCheckpoint']
assert latest == sys.argv[1], ('the fold names', latest, 'latest, not', sys.argv[1])
body = next(c['body'] for c in fold['checkpoints'] if c['eventId'] == latest)
assert body['task'] and body['nextAction'], body
print(json.dumps({'task': body['task'], 'nextAction': body['nextAction']}))
" "${CHECKPOINT_2}")"
  task="$(py -c "import json,sys; print(json.loads(sys.argv[1])['task'])" "${latest_checkpoint}")"
  next_action="$(py -c "import json,sys; print(json.loads(sys.argv[1])['nextAction'])" "${latest_checkpoint}")"
  py -c "
import json, sys
log_path, index, task, next_action = sys.argv[1:]
with open(log_path, encoding='utf-8') as source:
    prompts = [json.loads(line) for line in source if line.strip()
               and json.loads(line).get('method') == 'session/prompt']
delivered = prompts[int(index)]['params']['prompt']
text = ''.join(block.get('text', '') for block in delivered)
assert task in text, f'the delivered prompt does not carry the checkpoint task:\n{text}'
assert next_action in text, f'the delivered prompt does not carry the next action:\n{text}'
" "${ACP_LOG_B}" "${baseline_b}" "${task}" "${next_action}"

  # 3c. The execution's own report of the folder, from the provider's bounded
  # git probe (`git_probe.rs`) rather than from anything the CLI wrote: the
  # branch and the commit it sees must be the recovered ones. Polled, because
  # `branch`/`observedCommit` are null until that probe completes.
  local metadata_ok=""
  for _ in $(seq 1 60); do
    metadata_ok="$(q 44223 | py -c "
import json, sys, helpers
provider, target, branch, head_sha = sys.argv[1:]
rows = []
for event in json.load(sys.stdin):
    if event['pubkey'] != provider:
        continue
    content = json.loads(event['content'])
    if helpers.target_to_key(content['session']) != target:
        continue
    if content.get('branch') is None and content.get('observedCommit') is None:
        continue
    rows.append((event['created_at'], event['id'], content))
if not rows:
    print('')
    raise SystemExit(0)
rows.sort()
first = rows[0][2]
problems = []
if first.get('branch') != branch:
    problems.append(f\"branch={first.get('branch')!r} (wanted {branch!r})\")
if (first.get('observedCommit') or '').lower() != head_sha.lower():
    problems.append(f\"observedCommit={first.get('observedCommit')!r} (wanted {head_sha!r})\")
print('ok' if not problems else 'BAD ' + '; '.join(problems))
" "${PROVIDER_B_HEX}" "${target_b}" "handover/${S1:0:8}" "${HEAD_SHA_A}")"
    [[ -n "${metadata_ok}" ]] && break
    sleep 1
  done
  [[ "${metadata_ok}" == "ok" ]] || {
    err "P_B's first observed metadata for the reconstructed execution: ${metadata_ok:-no branch/observedCommit published within 60s}"
    return 1
  }

  # 4. The continuation record itself, as the relay stored it.
  q 44247 | py -c "
import json, sys, helpers
events = json.load(sys.stdin)
continuation_id, author, takeover_id, target_b = sys.argv[1:]
stored = [event for event in events if event['id'] == continuation_id]
assert len(stored) == 1, f'the relay has no 44247 {continuation_id}'
assert stored[0]['pubkey'] == author, stored[0]
body = json.loads(stored[0]['content'])['body']
assert body['mode'] == 'reconstructed', body
assert body['claimRef'] == takeover_id, body
assert helpers.target_to_key(body['target']) == target_b, body
assert body['recovered'], body
# A continuation that had to disclose the execution opened elsewhere is a
# different (honest) outcome, and not the one this step is asserting.
for line in body['missing']:
    assert 'not the recovered checkout' not in line, line
" "${CONTINUATION_1}" "${B_HEX}" "${takeover_id}" "${target_b}"

  pass 3 "P_A killed; with no live lease the default plan reconstructed: a run with no projects file was refused naming --projects-file and left the chain at ${links_before} links; takeover ${takeover_id:0:12}… then accepted with bodyPubkey=P_B; B's checkout is on ${branch} at ${HEAD_SHA_A:0:12} with tracked+staged+unstaged+untracked+binary bytes restored; the create ${create_command_id} was bound through a one-shot hint file that survived ${overlaps} desktop saves of projects.json (byte-identical throughout, and its project/channel entries still point at B-default) and was spent on admission (its .consumed marker names B-work and the execution it minted), ACP opened the session on B-work, and P_B's first observed metadata reports branch handover/${S1:0:8} at ${HEAD_SHA_A:0:12}; one session.create joined ${S1} on P_B (created receipt, exactly one new ACP prompt carrying the task and next action), and continuation ${CONTINUATION_1:0:12}… records reconstructed with recovered lines and no relocation disclosure"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 4 — P_A comes back: fenced metadata, fenced queued turn, fenced new turn
# ═══════════════════════════════════════════════════════════════════════════
step_4() {
  local queued_cmd restart_epoch fresh_cmd after_a

  # The turn A queued while its own provider was dead. Published first, so the
  # restart genuinely finds it waiting rather than racing it.
  queued_cmd="$(send_turn a "${TARGET_A}" "pick the fold back up")"
  sleep 2

  restart_epoch="$(date +%s)"
  spawn_provider "${PROVIDER_A_KEY}" "${STATE_DIR_A}" "${PROJECTS_A}" "${RUNTIMES_JSON}" \
    "${WORKDIR}/provider-a-restart.log" "${METHODS_LOG_A}" "${ACP_LOG_A}" "${CWD_LOG_A}"
  local restarted_pid restarted_hex
  restarted_pid="${LAST_PID}"
  restarted_hex="$(wait_provider_pubkey "${restarted_pid}" "${WORKDIR}/provider-a-restart.log")"
  [[ "${restarted_hex}" == "${PROVIDER_A_HEX}" ]] || {
    err "restarted P_A came up as ${restarted_hex}, not ${PROVIDER_A_HEX}"; return 1
  }
  wait_provider_relay_connected "${restarted_pid}" "${WORKDIR}/provider-a-restart.log"
  state_put PROVIDER_A_PID "${restarted_pid}"

  # 1. The first metadata this process publishes for E_A discloses the fence in
  #    the same breath as the status (§3.1).
  local metadata_ok=""
  for _ in $(seq 1 60); do
    metadata_ok="$(q 44223 | py -c "
import json, sys, helpers
events = json.load(sys.stdin)
provider, target, epoch, claimant, body, takeover = sys.argv[1:]
rows = [
    (event, json.loads(event['content']))
    for event in events
    if event['pubkey'] == provider and event.get('created_at', 0) >= int(epoch)
]
rows = [row for row in rows if helpers.target_to_key(row[1]['session']) == target]
rows.sort(key=lambda pair: (pair[0]['created_at'], pair[0]['id']))
if not rows:
    print('')
    raise SystemExit(0)
first = rows[0][1]
handover = first.get('handover')
problems = []
if first.get('status') != 'disconnected':
    problems.append(f\"status={first.get('status')!r}\")
if handover is None:
    problems.append('no handover block')
elif (handover.get('claimant'), handover.get('bodyPubkey'), handover.get('acceptedEventId')) != (claimant, body, takeover):
    problems.append(f'handover={handover!r}')
print('ok' if not problems else 'BAD ' + '; '.join(problems))
" "${PROVIDER_A_HEX}" "${TARGET_A}" "${restart_epoch}" "${B_HEX}" "${PROVIDER_B_HEX}" "${TAKEOVER_1}")"
    [[ -n "${metadata_ok}" ]] && break
    sleep 1
  done
  [[ "${metadata_ok}" == "ok" ]] || {
    err "P_A's first metadata for E_A after the restart: ${metadata_ok:-none published within 60s}"
    return 1
  }

  # 2. Both turns — the one that was already waiting, and a fresh one — are
  #    refused by name.
  expect_receipt "${queued_cmd}" "turn_refused:HANDOVER_FENCED" "step 4 (queued turn)"
  fresh_cmd="$(send_turn a "${TARGET_A}" "are you still there")"
  expect_receipt "${fresh_cmd}" "turn_refused:HANDOVER_FENCED" "step 4 (fresh turn)"

  # 3. Nothing reached the adapter.
  after_a="$(acp_prompt_count "${ACP_LOG_A}")"
  [[ "${after_a}" -eq "${ACP_BASELINE_A}" ]] || {
    err "P_A's ACP log went from ${ACP_BASELINE_A} to ${after_a} prompts; a fenced execution ran a turn"
    return 1
  }

  pass 4 "restarted P_A re-derived the fence before publishing: its first 44223 for E_A is disconnected and carries handover{claimant=B, body=P_B, link=${TAKEOVER_1:0:12}…}, the pre-crash queued turn and a fresh one both answered turn_refused/HANDOVER_FENCED, and its ACP log gained zero prompts"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 5 — the sibling execution is fenced with the umbrella
# ═══════════════════════════════════════════════════════════════════════════
step_5() {
  local sibling_cmd
  sibling_cmd="$(send_turn a "${TARGET_A2}" "sibling execution, never named by the claim")"
  expect_receipt "${sibling_cmd}" "turn_refused:HANDOVER_FENCED" "step 5"
  pass 5 "E_A2 — a sibling execution the claim never names — answered turn_refused/HANDOVER_FENCED too: v1 hands over the whole umbrella (§1 scope decision)"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 6 — two claims at one chain head; the relay picks one
# ═══════════════════════════════════════════════════════════════════════════
step_6() {
  local lines s2 g2 a_exit b_exit winner
  lines="$(found_umbrella a "${PROVIDER_A_HEX}" "the racing-claims umbrella")"
  s2="$(sed -n 1p <<< "${lines}")"; g2="$(sed -n 2p <<< "${lines}")"
  bee_a sessions grant --channel "${CHANNEL}" --genesis "${g2}" --pubkey "${B_HEX}" --role collaborator >/dev/null

  # Retried, because "concurrent" is a hope rather than a guarantee: each claim
  # is its own `bee` process, and if one completes before the other reads the
  # chain then both are accepted at different seqs and nothing collided. That is
  # not a failure of the relay's serialization — it is a run that did not test
  # it — so it is retried on a fresh umbrella and, if it never collides, said
  # so rather than counted.
  local attempt raced=0
  for attempt in 1 2 3; do
    if [[ "${attempt}" -gt 1 ]]; then
      lines="$(found_umbrella a "${PROVIDER_A_HEX}" "the racing-claims umbrella ${attempt}")"
      s2="$(sed -n 1p <<< "${lines}")"; g2="$(sed -n 2p <<< "${lines}")"
      bee_a sessions grant --channel "${CHANNEL}" --genesis "${g2}" --pubkey "${B_HEX}" --role collaborator >/dev/null
    fi
    set +e
    ( bee_a sessions handover claim --channel "${CHANNEL}" --session-ref "${s2}" --genesis "${g2}" \
        --body-self --json > "${WORKDIR}/claim-a.json" 2> "${WORKDIR}/claim-a.err"
      echo $? > "${WORKDIR}/claim-a.exit" ) &
    ( bee_b sessions handover claim --channel "${CHANNEL}" --session-ref "${s2}" --genesis "${g2}" \
        --body "${PROVIDER_B_HEX}" --json > "${WORKDIR}/claim-b.json" 2> "${WORKDIR}/claim-b.err"
      echo $? > "${WORKDIR}/claim-b.exit" ) &
    wait
    set -e
    a_exit="$(cat "${WORKDIR}/claim-a.exit")"
    b_exit="$(cat "${WORKDIR}/claim-b.exit")"
    log "attempt ${attempt}: concurrent claims exited: A=${a_exit} B=${b_exit}"
    if [[ "${a_exit}" -eq 0 && "${b_exit}" -eq 0 ]]; then
      log "attempt ${attempt}: both claims were accepted, so they never met at one head; retrying on a fresh umbrella"
      continue
    fi
    raced=1
    break
  done
  [[ "${raced}" -eq 1 ]] || {
    err "the two claims never collided at one chain head in 3 attempts, so the relay's serialization was not exercised"
    return 1
  }

  # Exactly one accepted takeover for this genesis, whichever won.
  winner="$(q 40099 | py -c "
import json, sys
events = json.load(sys.stdin)
relay_self, genesis_ref = sys.argv[1:]
rows = []
for event in events:
    if event['pubkey'] != relay_self:
        continue
    content = json.loads(event['content'])
    if content.get('type') != 'coding_session_authority_transition_accepted':
        continue
    if content.get('genesisRef') == genesis_ref and content.get('transitionType') == 'takeover':
        rows.append(content)
assert len(rows) == 1, f'expected exactly one accepted takeover, found {len(rows)}: {rows!r}'
print(rows[0]['granteePubkey'])
" "${RELAY_SELF}" "${g2}")"

  local winner_exit loser_exit loser_out
  if [[ "${winner}" == "${A_HEX}" ]]; then
    winner_exit="${a_exit}"; loser_exit="${b_exit}"; loser_out="${WORKDIR}/claim-b"
  else
    winner_exit="${b_exit}"; loser_exit="${a_exit}"; loser_out="${WORKDIR}/claim-a"
  fi
  [[ "${winner_exit}" -eq 0 ]] || { err "the winning claim exited ${winner_exit}"; return 1; }
  [[ "${loser_exit}" -eq 5 ]] || {
    err "the losing claim exited ${loser_exit}, expected 5; stderr:"
    cat "${loser_out}.err" >&2
    return 1
  }
  # The CLI names the winner the way it names every pubkey a person reads —
  # `short_pubkey`, the first 8 hex — so accept that or the full key. What must
  # not happen is a bare "you lost" with nobody named.
  if ! grep -Fq "${winner}" "${loser_out}.err" "${loser_out}.json" \
     && ! grep -Fq "${winner:0:8}" "${loser_out}.err" "${loser_out}.json"; then
    err "the loser's output never names the winner ${winner} (nor its short form ${winner:0:8})"
    cat "${loser_out}.err" "${loser_out}.json" >&2
    return 1
  fi

  # And the fold agrees with the relay.
  handover_status a "${s2}" "${g2}" | py -c "
import json, sys
claim = json.load(sys.stdin)['fold']['claim']
assert claim['state'] == 'active', claim
assert claim['claimant'] == sys.argv[1], claim
" "${winner}"

  pass 6 "two concurrent takeovers at one chain head produced exactly one accepted 40099 (winner ${winner:0:12}…); the loser exited 5 naming the winner, and handover status folds the same answer"
}

# Send a turn and require a particular verdict, retrying the send while the
# provider is still catching up on a chain change. Used where the expected
# answer depends on the provider having folded a claim published seconds ago.
send_expect() {
  local who="$1" target="$2" text="$3" expected="$4" label="$5" attempts="${6:-1}"
  local command_id verdict i
  for i in $(seq 1 "${attempts}"); do
    command_id="$(send_turn "${who}" "${target}" "${text} (${i})")"
    verdict="$(wait_receipt "${command_id}")"
    [[ "${verdict}" == "${expected}" ]] && { echo "${command_id}"; return 0; }
    sleep 3
  done
  err "${label}: last verdict for ${command_id} was '${verdict}', expected '${expected}'"
  return 1
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 7 — revoke voids the claim; a regrant does not restore it
# ═══════════════════════════════════════════════════════════════════════════
step_7() {
  local state_after_revoke state_after_regrant claim_a

  bee_a sessions revoke --channel "${CHANNEL}" --genesis "${G1}" --pubkey "${B_HEX}" >/dev/null
  state_after_revoke="$(handover_status a "${S1}" "${G1}" | py -c "
import json, sys
claim = json.load(sys.stdin)['fold']['claim']
print(claim['state'])
")"
  [[ "${state_after_revoke}" == "voided" ]] || {
    err "after revoking the claimant the fold reads '${state_after_revoke}', expected 'voided'"; return 1
  }

  bee_a sessions grant --channel "${CHANNEL}" --genesis "${G1}" --pubkey "${B_HEX}" --role collaborator >/dev/null
  state_after_regrant="$(handover_status a "${S1}" "${G1}" | py -c "
import json, sys
claim = json.load(sys.stdin)['fold']['claim']
print(claim['state'])
")"
  [[ "${state_after_regrant}" == "voided" ]] || {
    err "after regranting the same pubkey the fold reads '${state_after_regrant}', expected 'voided' — a regrant must not restore a claim"
    return 1
  }

  # A real turn from B to the body it claimed, not a status read: the fence has
  # to hold at the provider, not only in a fold.
  send_expect b "${TARGET_B}" "still mine?" "turn_refused:HANDOVER_FENCED" "step 7 (voided, B → E_B)" 5 >/dev/null

  # The founder takes it back, on its own body.
  bee_a sessions handover claim --channel "${CHANNEL}" --session-ref "${S1}" --genesis "${G1}" \
    --body-self --json > "${WORKDIR}/claim-back.json"
  claim_a="$(handover_status a "${S1}" "${G1}" | py -c "
import json, sys
claim = json.load(sys.stdin)['fold']['claim']
assert claim['state'] == 'active', claim
assert claim['claimant'] == sys.argv[1], claim
assert claim['bodyPubkey'] == sys.argv[2], claim
print(claim['acceptedEventId'])
" "${A_HEX}" "${PROVIDER_A_HEX}")"

  send_expect b "${TARGET_B}" "one more" "turn_refused:HANDOVER_FENCED" "step 7 (A holds, B → E_B)" 5 >/dev/null

  # A now holds the claim on its own body, so E_A must stop answering
  # HANDOVER_FENCED. It does not follow that the turn runs: this execution's
  # provider was `kill -9`'d in step 3 and a fenced record is never reopened,
  # so the generation has no live ACP session and answers the ordinary
  # NO_LIVE_EXECUTION (what `bee sessions send --readdress` exists for).
  # Asserting `turn_started` here would demand a revival the contract never
  # promises; asserting "not fenced" is the thing the claim actually changed.
  local back_cmd back_verdict
  back_cmd="$(send_turn a "${TARGET_A}" "back to work")"
  back_verdict="$(wait_receipt "${back_cmd}")"
  case "${back_verdict}" in
    turn_refused:HANDOVER_FENCED)
      err "step 7: A holds the claim on its own body and E_A is still fenced (${back_verdict})"
      return 1 ;;
    none:)
      err "step 7: no receipt answered A's turn ${back_cmd} after the takeback"
      return 1 ;;
  esac

  pass 7 "revoking B voided the claim, regranting the same pubkey left it voided, and a real turn from B to its own claimed body stayed HANDOVER_FENCED; a fresh founder takeover ${claim_a:0:12}… on body P_A lifted the fence on E_A, which then answered '${back_verdict}' (not fenced — its provider was kill -9'd in step 3, so the generation has no live agent to run the turn), while B's turn to E_B stayed fenced"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 8 — replaying accepted bytes changes nothing
# ═══════════════════════════════════════════════════════════════════════════
step_8() {
  local before after created_before created_after code_takeover code_continuation

  count_created_for_b() {
    q 44224 | py -c "
import json, sys, helpers
target = sys.argv[1]
print(sum(
    1 for event in json.load(sys.stdin)
    if json.loads(event['content']).get('status') == 'created'
    and helpers.target_to_key(json.loads(event['content'])['session']) == target
))
" "${TARGET_B}"
  }
  fold_fingerprint() {
    handover_status a "${S1}" "${G1}" | py -c "
import json, sys
fold = json.load(sys.stdin)['fold']
print(json.dumps({
    'claim': fold['claim'],
    'continuations': [
        {'eventId': c['eventId'], 'claimRef': c['claimRef'], 'mode': c['mode'], 'standing': c['standing']}
        for c in fold['continuations']
    ],
}, sort_keys=True))
"
  }

  before="$(fold_fingerprint)"
  created_before="$(count_created_for_b)"
  printf '%s\n' "${before}" > "${WORKDIR}/fold-before-replay.json"

  code_takeover="$(replay_event "${TAKEOVER_1}" 44228)"
  code_continuation="$(replay_event "${CONTINUATION_1}" 44247)"
  log "replayed takeover → HTTP ${code_takeover}; replayed continuation → HTTP ${code_continuation}"
  sleep 5

  after="$(fold_fingerprint)"
  created_after="$(count_created_for_b)"
  printf '%s\n' "${after}" > "${WORKDIR}/fold-after-replay.json"

  [[ "${before}" == "${after}" ]] || {
    err "replaying accepted bytes changed the fold"
    diff <(printf '%s\n' "${before}") <(printf '%s\n' "${after}") >&2 || true
    return 1
  }
  [[ "${created_before}" == "${created_after}" ]] || {
    err "replaying accepted bytes created another execution on P_B (${created_before} → ${created_after} created receipts)"
    return 1
  }
  py -c "
import json, sys
fold = json.loads(sys.argv[1])
assert len(fold['continuations']) == 1, fold['continuations']
" "${after}"

  pass 8 "re-POSTing the accepted takeover bytes (HTTP ${code_takeover}) and the continuation bytes (HTTP ${code_continuation}) at /events left the fold byte-identical — one claim, one continuation — and P_B's created-receipt count unchanged at ${created_after}"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 9 — a checkpoint whose wip sha the relay never saw
# ═══════════════════════════════════════════════════════════════════════════
step_9() {
  local bare checkout lines s3 g3 checkpoint_json off_relay_sha refused_exit allowed_json

  bare="${WORKDIR}/bare-9.git"
  git init -q --bare "${bare}"
  checkout="${WORKDIR}/checkout-a9"
  init_checkout "${checkout}" "owner-a"
  git -C "${checkout}" remote add local "${bare}"
  printf 'work that never reached the relay\n' > "${checkout}/offline.txt"
  git -C "${checkout}" add -A
  git -C "${checkout}" commit -q -m "offline commit"
  off_relay_sha="$(git -C "${checkout}" rev-parse HEAD)"

  relay_ref_shas > "${WORKDIR}/relay-shas-9.txt"
  grep -Fxq "${off_relay_sha}" "${WORKDIR}/relay-shas-9.txt" && {
    err "test setup is wrong: ${off_relay_sha} is already named by the relay's 30618"; return 1
  }

  lines="$(found_umbrella a "${PROVIDER_A_HEX}" "the missing-artifact umbrella")"
  s3="$(sed -n 1p <<< "${lines}")"; g3="$(sed -n 2p <<< "${lines}")"
  bee_a sessions grant --channel "${CHANNEL}" --genesis "${g3}" --pubkey "${B_HEX}" --role collaborator >/dev/null

  checkpoint_json="$(bee_a_git sessions handover checkpoint --channel "${CHANNEL}" \
    --session-ref "${s3}" --genesis "${g3}" --cwd "${checkout}" --repo "${REPO_ID}" \
    --remote local --task "work pushed only to a bare remote" \
    --next "fetch the wip ref that is not on the relay" --json)"
  py -c "
import json, sys
body = json.loads(sys.argv[1])['checkpoint']
wip = [a for a in body['artifacts'] if a['kind'] == 'wip-ref']
assert len(wip) == 1 and wip[0]['sha'] == sys.argv[2], body['artifacts']
" "${checkpoint_json}" "${off_relay_sha}"

  set +e
  bee_b_git sessions handover continue --channel "${CHANNEL}" --session-ref "${s3}" --genesis "${g3}" \
    --cwd "${CHECKOUT_B9}" --body "${PROVIDER_B_HEX}" --provider-instance "${INSTANCE}" \
    --projects-file "${PROJECTS_B}" \
    --reconstruct --remote origin --wait-secs 120 --json \
    > "${WORKDIR}/continue-9-refused.json" 2> "${WORKDIR}/continue-9-refused.err"
  refused_exit=$?
  set -e
  [[ "${refused_exit}" -ne 0 ]] || {
    err "continue --reconstruct accepted a checkpoint whose sha the relay's 30618 does not name"
    cat "${WORKDIR}/continue-9-refused.json" >&2
    return 1
  }
  grep -Fq "${off_relay_sha}" "${WORKDIR}/continue-9-refused.err" || {
    err "the refusal does not name the sha it refused (${off_relay_sha})"
    cat "${WORKDIR}/continue-9-refused.err" >&2
    return 1
  }
  grep -Fq "30618" "${WORKDIR}/continue-9-refused.err" || {
    err "the refusal does not say which ref state it read"
    cat "${WORKDIR}/continue-9-refused.err" >&2
    return 1
  }

  # Nothing is rewriting `projects.json` here, so this is the uncontaminated
  # test of the same rule step 3 checks under a busy desktop: a reconstruction
  # binds its checkout through a hint file and leaves the projects file alone.
  local projects_hash_before projects_hash_after
  projects_hash_before="$(shasum -a 256 "${PROJECTS_B}" | cut -d' ' -f1)"
  allowed_json="$(bee_b_git sessions handover continue --channel "${CHANNEL}" \
    --session-ref "${s3}" --genesis "${g3}" --cwd "${CHECKOUT_B9}" \
    --body "${PROVIDER_B_HEX}" --provider-instance "${INSTANCE}" \
    --projects-file "${PROJECTS_B}" \
    --reconstruct --allow-no-artifact --remote origin --wait-secs 120 --json)"
  projects_hash_after="$(shasum -a 256 "${PROJECTS_B}" | cut -d' ' -f1)"
  [[ "${projects_hash_before}" == "${projects_hash_after}" ]] || {
    err "the CLI modified ${PROJECTS_B} during a reconstruction (${projects_hash_before:0:12} -> ${projects_hash_after:0:12})"
    return 1
  }
  printf '%s\n' "${allowed_json}" > "${WORKDIR}/continue-9-allowed.json"
  py -c "
import json, sys
out = json.loads(sys.argv[1])
sha = sys.argv[2]
assert out['continuation']['mode'] == 'reconstructed', out['continuation']
missing = out['continuation']['missing']
assert any(sha in line for line in missing), f'{sha} is not listed under missing: {missing!r}'
" "${allowed_json}" "${off_relay_sha}"

  pass 9 "a checkpoint whose wip sha ${off_relay_sha:0:12} the relay's 30618 never named was refused by name (exit ${refused_exit}, message quotes the sha and the ref state it read); --allow-no-artifact proceeded, listed that sha under the continuation's missing, and left projects.json byte-identical"
}


# ═══════════════════════════════════════════════════════════════════════════
# Step 10 — interrupted between the claim receipt and the continuation
# ═══════════════════════════════════════════════════════════════════════════
#
# Two legs, because the interesting state can be reached two ways and only one
# of them is deterministic:
#
#   (b) kill `handover continue` in the window between the accepted takeover
#       and the `session.create` that follows it. That is the real interruption,
#       and it is a race: with a stub adapter the window is short, so this is
#       attempted three times and, if it never lands, said so rather than
#       silently counted.
#   (a) reach the identical on-wire state with `handover claim` alone — an
#       accepted claim with no continuation — and prove the rerun of `continue`
#       is idempotent against it. Deterministic, and always run.
#
# Not covered, and named: a kill *after* the `session.create` is published but
# before the continuation. `already_continued` short-circuits on a continuation,
# so a rerun in that window would publish a second create; nothing in this
# script or in the CLI claims otherwise.
step_10() {
  local attempt lines s4 g4 cli_pid claim_id checkout landed=0 how=""

  b_creates_for() {
    q 44221 | py -c "
import json, sys
author, session_ref = sys.argv[1:]
count = 0
for event in json.load(sys.stdin):
    if event['pubkey'] != author:
        continue
    action = json.loads(event['content']).get('action', {})
    if action.get('type') == 'session.create' and action.get('sessionRef') == session_ref:
        count += 1
print(count)
" "${B_HEX}" "$1"
  }
  accepted_takeover_for() {
    q 40099 | py -c "
import json, sys
relay_self, genesis_ref = sys.argv[1:]
for event in json.load(sys.stdin):
    if event['pubkey'] != relay_self:
        continue
    content = json.loads(event['content'])
    if (content.get('type') == 'coding_session_authority_transition_accepted'
            and content.get('genesisRef') == genesis_ref
            and content.get('transitionType') == 'takeover'):
        print(content['acceptedEventId'])
        break
" "${RELAY_SELF}" "$1"
  }
  # A fresh umbrella, granted to B, with a checkpoint carrying real artifacts
  # (a wip ref on the relay and a patch), so a reconstruction has genuine work
  # to do between the claim and the create. Prints sessionRef and genesisRef.
  seed_interrupt_umbrella() {
    local label="$1" seeded
    seeded="$(found_umbrella a "${PROVIDER_A_HEX}" "${label}")"
    local session_ref genesis_ref
    session_ref="$(sed -n 1p <<< "${seeded}")"
    genesis_ref="$(sed -n 2p <<< "${seeded}")"
    bee_a sessions grant --channel "${CHANNEL}" --genesis "${genesis_ref}" \
      --pubkey "${B_HEX}" --role collaborator >/dev/null
    bee_a_git sessions handover checkpoint --channel "${CHANNEL}" --session-ref "${session_ref}" \
      --genesis "${genesis_ref}" --cwd "${CHECKOUT_A}" --repo "${REPO_ID}" \
      --task "work interrupted mid-continuation" \
      --next "re-run continue and change nothing" --json > /dev/null
    printf '%s\n%s\n' "${session_ref}" "${genesis_ref}"
  }

  # ── leg (b): the real interruption ────────────────────────────────────────
  for attempt in 1 2 3; do
    lines="$(seed_interrupt_umbrella "the interrupted-publication umbrella ${attempt}")"
    s4="$(sed -n 1p <<< "${lines}")"; g4="$(sed -n 2p <<< "${lines}")"
    checkout="${WORKDIR}/checkout-b10-kill-${attempt}"
    seed_from_relay "${checkout}" "owner-b"

    # The binary directly, never the `bee_b_git` wrapper: backgrounding a shell
    # function makes `$!` the wrapper subshell's pid, so `kill -9 $!` kills the
    # wrapper and leaves the real `bee` running. That happened, and it left a
    # `handover continue` alive for minutes after this step believed it had
    # killed it — which is the one thing this step must not get wrong.
    BEEKEEPER_PRIVATE_KEY="${B_KEY}" NOSTR_PRIVATE_KEY="${B_KEY}" "${BEE_BIN}" \
      sessions handover continue --channel "${CHANNEL}" --session-ref "${s4}" \
      --genesis "${g4}" --cwd "${checkout}" --body "${PROVIDER_B_HEX}" \
      --provider-instance "${INSTANCE}" --projects-file "${PROJECTS_B}" \
      --reconstruct --remote origin --wait-secs 120 --json \
      > "${WORKDIR}/continue-10-killed-${attempt}.json" 2>&1 &
    cli_pid=$!
    record_pid "${cli_pid}"

    claim_id=""
    for _ in $(seq 1 200); do
      claim_id="$(accepted_takeover_for "${g4}")"
      [[ -n "${claim_id}" ]] && break
      kill -0 "${cli_pid}" 2>/dev/null || break
    done
    kill -9 "${cli_pid}" 2>/dev/null || true
    wait "${cli_pid}" 2>/dev/null || true
    wait_pid_exit "${cli_pid}"
    if [[ -z "${claim_id}" ]]; then
      log "attempt ${attempt}: no takeover was accepted before the CLI ended; retrying"
      continue
    fi
    if [[ "$(b_creates_for "${s4}")" -ne 0 ]]; then
      log "attempt ${attempt}: the kill landed after the session.create; retrying"
      continue
    fi
    landed=1; how="killed mid-continue"
    break
  done

  # ── leg (a): the deterministic route to the same state ────────────────────
  if [[ "${landed}" -ne 1 ]]; then
    warn "the claim→create window never opened wide enough to kill into in 3 attempts; proving idempotency from the same on-wire state reached with \`handover claim\` instead"
    lines="$(seed_interrupt_umbrella "the interrupted-publication umbrella (claim-only)")"
    s4="$(sed -n 1p <<< "${lines}")"; g4="$(sed -n 2p <<< "${lines}")"
    checkout="${WORKDIR}/checkout-b10-claim"
    seed_from_relay "${checkout}" "owner-b"
    bee_b sessions handover claim --channel "${CHANNEL}" --session-ref "${s4}" --genesis "${g4}" \
      --body "${PROVIDER_B_HEX}" --json > "${WORKDIR}/claim-10.json"
    claim_id="$(accepted_takeover_for "${g4}")"
    [[ -n "${claim_id}" ]] || { err "the claim-only leg published no accepted takeover"; return 1; }
    how="claim published, continuation never reached"
  fi

  # The interrupted state: a claim, and nothing else.
  handover_status a "${s4}" "${g4}" | py -c "
import json, sys
fold = json.load(sys.stdin)['fold']
assert fold['claim']['state'] == 'active', fold['claim']
assert fold['claim']['acceptedEventId'] == sys.argv[1], fold['claim']
assert fold['continuations'] == [], fold['continuations']
assert fold['activeContinuation'] is None, fold['activeContinuation']
" "${claim_id}"
  [[ "$(b_creates_for "${s4}")" -eq 0 ]] || { err "an execution already exists before the rerun"; return 1; }

  bee_b_git sessions handover continue --channel "${CHANNEL}" --session-ref "${s4}" \
    --genesis "${g4}" --cwd "${checkout}" --body "${PROVIDER_B_HEX}" \
    --provider-instance "${INSTANCE}" --projects-file "${PROJECTS_B}" \
    --reconstruct --remote origin --wait-secs 120 --json \
    > "${WORKDIR}/continue-10-rerun.json"

  handover_status a "${s4}" "${g4}" | py -c "
import json, sys
fold = json.load(sys.stdin)['fold']
claim_id = sys.argv[1]
assert fold['claim']['state'] == 'active', fold['claim']
assert fold['claim']['acceptedEventId'] == claim_id, ('the rerun published a second claim', fold['claim'])
assert len(fold['continuations']) == 1, fold['continuations']
assert fold['continuations'][0]['claimRef'] == claim_id, fold['continuations'][0]
" "${claim_id}"
  local creates
  creates="$(b_creates_for "${s4}")"
  [[ "${creates}" -eq 1 ]] || {
    err "the rerun left ${creates} session.creates for ${s4}, expected exactly 1"; return 1
  }

  pass 10 "interrupted after claim ${claim_id:0:12}… was accepted (${how}): status showed an active claim with no continuation, and re-running continue was idempotent — same claim, exactly one session.create on P_B, exactly one continuation"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 11 — retirement: a deleted umbrella is resumable by nobody
# ═══════════════════════════════════════════════════════════════════════════
step_11() {
  local delete_json receipt_id restart_epoch restarted_pid restarted_hex

  delete_json="$(bee_a sessions delete --channel "${CHANNEL}" --session-ref "${S1}")"
  printf '%s\n' "${delete_json}" > "${WORKDIR}/delete.json"

  # The relay's own signed acceptance, not the CLI's word for it.
  receipt_id=""
  for _ in $(seq 1 45); do
    receipt_id="$(q 40099 | py -c "
import json, sys
relay_self, genesis_ref, session_ref = sys.argv[1:]
for event in json.load(sys.stdin):
    if event['pubkey'] != relay_self:
        continue
    content = json.loads(event['content'])
    if content.get('type') != 'coding_session_deletion_accepted':
        continue
    if content.get('genesisRef') == genesis_ref:
        assert content.get('sessionRef') == session_ref, content
        assert content.get('deletionEventId'), content
        print(event['id'])
        break
" "${RELAY_SELF}" "${G1}" "${S1}")"
    [[ -n "${receipt_id}" ]] && break
    sleep 1
  done
  [[ -n "${receipt_id}" ]] || { err "the relay published no coding_session_deletion_accepted receipt for ${G1}"; return 1; }

  # A provider that was already running when the deletion landed retires from
  # the live event, not from a restart.
  send_expect a "${TARGET_A}" "anyone home" "turn_refused:SESSION_RETIRED" "step 11 (live P_A)" 10 >/dev/null

  # …and one that restarts afterwards retires on `recover`, before it
  # republishes anything.
  log "restarting P_B over its own state dir"
  kill -9 "${PROVIDER_B_PID}"
  wait_pid_exit "${PROVIDER_B_PID}"
  restart_epoch="$(date +%s)"
  spawn_provider "${PROVIDER_B_KEY}" "${STATE_DIR_B}" "${PROJECTS_B}" "${RUNTIMES_JSON}" \
    "${WORKDIR}/provider-b-restart.log" "${METHODS_LOG_B}" "${ACP_LOG_B}" "${CWD_LOG_B}"
  restarted_pid="${LAST_PID}"
  restarted_hex="$(wait_provider_pubkey "${restarted_pid}" "${WORKDIR}/provider-b-restart.log")"
  [[ "${restarted_hex}" == "${PROVIDER_B_HEX}" ]] || { err "restarted P_B is ${restarted_hex}"; return 1; }
  wait_provider_relay_connected "${restarted_pid}" "${WORKDIR}/provider-b-restart.log"
  state_put PROVIDER_B_PID "${restarted_pid}"
  sleep 10

  q 44223 | py -c "
import json, sys, helpers
provider, target, epoch = sys.argv[1:]
rows = [
    event for event in json.load(sys.stdin)
    if event['pubkey'] == provider and event.get('created_at', 0) >= int(epoch)
    and helpers.target_to_key(json.loads(event['content'])['session']) == target
]
assert not rows, f'a restarted provider published {len(rows)} metadata events for a retired execution'
" "${PROVIDER_B_HEX}" "${TARGET_B}" "${restart_epoch}"

  # seat-requests.json must not carry the retired execution. Every session in
  # this script is operator-created, so an unseated execution would not appear
  # here anyway — this asserts the file's contents, and the seated case is
  # proven only by the provider's own unit suite.
  if [[ -f "${STATE_DIR_B}/seat-requests.json" ]]; then
    py -c "
import json, sys
data = json.load(open(sys.argv[1], encoding='utf-8'))
blob = json.dumps(data)
assert sys.argv[2] not in blob, f'seat-requests.json still names the retired umbrella: {blob}'
" "${STATE_DIR_B}/seat-requests.json" "${S1}"
  fi

  send_expect b "${TARGET_B}" "did that survive" "turn_refused:SESSION_RETIRED" "step 11 (restarted P_B)" 10 >/dev/null

  handover_status a "${S1}" "${G1}" | py -c "
import json, sys
status = json.load(sys.stdin)
assert status['retired'] is True, status
assert status['retirement'], status
assert status['genesisUnavailable'] is None, ('an absent genesis was reported as unknown alongside a receipt', status)
"

  pass 11 "deleting the umbrella produced the relay's signed coding_session_deletion_accepted receipt ${receipt_id:0:12}…; the already-running P_A answered SESSION_RETIRED from the live deletion, a restarted P_B published no metadata for its execution, kept it out of seat-requests.json and answered SESSION_RETIRED, and handover status prints retired"
}

# ═══════════════════════════════════════════════════════════════════════════
# Step 12 — the native leg: B steers A's execution on A's own provider
# ═══════════════════════════════════════════════════════════════════════════
step_12() {
  local lines s5 g5 target_a5 baseline continue_json after

  lines="$(found_umbrella a "${PROVIDER_A_HEX}" "the native-continuation umbrella")"
  s5="$(sed -n 1p <<< "${lines}")"; g5="$(sed -n 2p <<< "${lines}")"
  target_a5="$(sed -n 3p <<< "${lines}")"
  bee_a sessions grant --channel "${CHANNEL}" --genesis "${g5}" --pubkey "${B_HEX}" --role collaborator >/dev/null
  bee_a_git sessions handover checkpoint --channel "${CHANNEL}" --session-ref "${s5}" \
    --genesis "${g5}" --cwd "${CHECKOUT_A}" --repo "${REPO_ID}" --no-push \
    --task "the native leg: P_A is alive and B has a grant" \
    --next "run the gate on the branch already open on A's machine" --json > "${WORKDIR}/checkpoint-12.json"

  wait_execution_live "${s5}" "${g5}" "${target_a5}"
  baseline="$(acp_prompt_count "${ACP_LOG_A}")"
  continue_json="$(bee_b_git sessions handover continue --channel "${CHANNEL}" \
    --session-ref "${s5}" --genesis "${g5}" --native --wait-secs 120 --json)"
  printf '%s\n' "${continue_json}" > "${WORKDIR}/continue-12.json"
  py -c "
import json, sys, helpers
out = json.loads(sys.argv[1])
assert out['continuation']['mode'] == 'native-resume', out['continuation']
assert helpers.target_to_key(out['continuation']['target']) == sys.argv[2], out['continuation']
" "${continue_json}" "${target_a5}"

  # The next action really reached A's own adapter.
  after="$(acp_prompt_count "${ACP_LOG_A}")"
  [[ "$(( after - baseline ))" -eq 1 ]] || {
    err "P_A's ACP log gained $(( after - baseline )) prompts on the native resume, expected exactly 1"
    return 1
  }
  py -c "
import json, sys
log_path, index = sys.argv[1:3]
with open(log_path, encoding='utf-8') as source:
    prompts = [json.loads(line) for line in source if line.strip()
               and json.loads(line).get('method') == 'session/prompt']
text = ''.join(block.get('text', '') for block in prompts[int(index)]['params']['prompt'])
assert 'run the gate on the branch already open' in text, text
" "${ACP_LOG_A}" "${baseline}"

  # A native resume is the original context on the original machine: it must run
  # in A's own folder. Nothing was recovered and nothing should have moved.
  local native_cwd
  native_cwd="$(session_cwd_for_prompt "${ACP_LOG_A}" "${baseline}")"
  [[ "${native_cwd}" == "$(real_path "${CHECKOUT_A}")" ]] || {
    err "the native resume was delivered to a session opened on ${native_cwd}, not A's own checkout $(real_path "${CHECKOUT_A}")"
    return 1
  }

  # A is not the claimant, so A's own turn on its own execution is fenced.
  send_expect a "${target_a5}" "my machine, my turn" "turn_refused:HANDOVER_FENCED" "step 12 (A fenced out)" 5 >/dev/null

  bee_a sessions handover claim --channel "${CHANNEL}" --session-ref "${s5}" --genesis "${g5}" \
    --body-self --json > "${WORKDIR}/claim-12-back.json"
  send_expect a "${target_a5}" "taken back" "turn_started:" "step 12 (A takes it back)" 10 >/dev/null

  pass 12 "with P_A alive, B's --native continuation reached E_A on P_A (exactly one new ACP prompt carrying the checkpoint's next action, run in A's own checkout and no other) and recorded native-resume; A's own turn was then HANDOVER_FENCED until A's founder takeover on its own body admitted it again"
}

# ═══════════════════════════════════════════════════════════════════════════
run_step 1  "A founds the umbrella, grants B, opens a sibling, sends a turn" step_1
run_step 2  "A checkpoints a dirty worktree; a 300 KiB file forces partial"  step_2
run_step 3  "P_A dies; B's default continue reconstructs on P_B"             step_3
run_step 4  "P_A returns fenced: metadata, queued turn, fresh turn"          step_4
run_step 5  "the sibling execution is fenced with the umbrella"              step_5
run_step 6  "two claims at one chain head; the relay picks one"              step_6
run_step 7  "revoke voids the claim; a regrant does not restore it"          step_7
run_step 8  "replaying accepted bytes changes nothing"                       step_8
run_step 9  "a checkpoint whose wip sha the relay never saw"                 step_9
run_step 10 "interrupted between the claim receipt and the continuation"     step_10
run_step 11 "retirement: a deleted umbrella is resumable by nobody"          step_11
run_step 12 "the native leg: B steers A's execution on A's own provider"     step_12
