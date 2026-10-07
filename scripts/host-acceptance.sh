#!/usr/bin/env bash
# =============================================================================
# host-acceptance.sh — real process proof for the agent host's lifecycle
# =============================================================================
#
# Composes, as independent OS processes talking over a real Unix socket:
#   - the actual `beekeeper-host` binary, commissioned from a throwaway `$HOME`
#     with a real keypair and a real `0600` key file;
#   - the actual `beekeeper-session-provider` binary as its supervised child;
#   - the actual `bee` CLI, driving `bee host …` over the control socket;
#   - a second `beekeeper-host` against the same state directory, to exercise
#     the refusal that stops two launchers fighting over one provider.
#
# Every assertion reads a process table, a file on disk, a provider's own log,
# or a signed-nothing JSON answer off the socket. A command's exit code is
# never the evidence for a step.
#
# ── What this does NOT prove (named, not faked) ──────────────────────────────
#
#   - **Transcript items published to a relay.** No relay runs here: the
#     provider is configured against an unreachable URL and spends its life
#     retrying, which is exactly the state that lets this script observe its
#     supervision without a database. So this says nothing about a coding
#     session's output reaching a channel. That proof needs a real relay and
#     lives in the human runbook — see `docs/remote-agents.md` § Launchers and
#     the plan's verification step 6.
#   - **The desktop app quitting.** There is no `Beekeeper.app` in this
#     composition, so "the session survived the app" is proven here only in
#     its mechanical half: the provider's parent is the *host*, and no desktop
#     exists that could take it down. That the app's own shutdown path no
#     longer touches the provider is asserted by the desktop's unit suite and
#     by the absence of any call to it (`shutdown.rs`).
#   - **Login start.** `launchctl`/`systemctl` registration is exercised by
#     `beekeeper-host install` and its unit tests, not here: loading a real
#     LaunchAgent would register a background service on the machine running
#     this script.
#   - **A real model.** Nothing spawns an ACP adapter. The provider never gets
#     a create command, because it never reaches a relay to receive one.
#
# Run: just test-host   (or ./scripts/host-acceptance.sh)
# =============================================================================

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

# Short base: an `AF_UNIX` path is capped at ~104 bytes, and a scratch dir
# under a long TMPDIR would blow it — a failure the host reports by name, but
# not one this script should walk into.
BASE="$(mktemp -d /tmp/host-acc.XXXXXX)"
FAKE_HOME="$BASE/home"
SOCK="$BASE/h.sock"
SOCK_B="$BASE/h-b.sock"
PIDS_FILE="$BASE/pids"
: >"$PIDS_FILE"

PASS=0
FAIL=0

cleanup() {
    # Every pid this script started, in reverse, whether a step failed or not.
    # Recorded in a file rather than a shell array because the steps run in
    # subshells and an array would not survive back here.
    if [[ -f "$PIDS_FILE" ]]; then
        while read -r pid; do
            [[ -n "$pid" ]] && kill -9 "$pid" 2>/dev/null || true
        done < <(tac "$PIDS_FILE" 2>/dev/null || tail -r "$PIDS_FILE")
    fi
    # Any provider that outlived its host — the thing this script is about, so
    # leaving one behind would poison the next run.
    pkill -f "$BASE" 2>/dev/null || true
    rm -rf "$BASE"
}
trap cleanup EXIT

record_pid() { echo "$1" >>"$PIDS_FILE"; }

pass() { PASS=$((PASS + 1)); printf 'PASS  %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL  %s\n' "$1" >&2; }
note() { printf '      %s\n' "$1"; }
step() { printf '\n── %s\n' "$1"; }

# Strip ANSI escapes from stdin.
#
# `tracing`'s terminal formatter colourises its fields, so a raw log line reads
# `attempt<esc>=<esc>1` and a plain `grep attempt=1` silently never matches —
# which is how an assertion about a log comes to pass or fail for reasons that
# have nothing to do with the log's content.
strip_ansi() { sed -E $'s/\x1b\[[0-9;]*[a-zA-Z]//g'; }

# Wait until `$1` (a shell snippet) succeeds, up to `$2` deciseconds.
wait_until() {
    local check=$1 limit=${2:-100} i=0
    while ((i < limit)); do
        if eval "$check" >/dev/null 2>&1; then return 0; fi
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

host_status() { BEEKEEPER_HOST_SOCK="$SOCK" HOME="$FAKE_HOME" "$BEE" host status 2>/dev/null; }
provider_state() { host_status | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["provider"]["state"])' 2>/dev/null; }
provider_pid() { host_status | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["provider"].get("pid",""))' 2>/dev/null; }

# ── build ────────────────────────────────────────────────────────────────────

step "Building the binaries this script composes"
export PATH="$REPO/bin:$PATH"
# Pinned, not inherited. `just` loads this repo's `.env`, which sets a
# relay-focused `RUST_LOG` — and the steps below assert on the host's own log
# lines, so an inherited filter would make them pass or fail for reasons that
# have nothing to do with the host. (The host now appends its own directive
# rather than being silenced by an unrelated filter; this pins it anyway, so
# the assertions do not depend on that behaviour either.)
export RUST_LOG="info,beekeeper_host=info"
PYTHON=$(command -v python3)
cargo build --quiet -p beekeeper-host -p beekeeper-cli -p beekeeper-session-provider
HOST="$REPO/target/debug/beekeeper-host"
BEE="$REPO/target/debug/bee"
PROVIDER="$REPO/target/debug/beekeeper-session-provider"
for binary in "$HOST" "$BEE" "$PROVIDER"; do
    [[ -x "$binary" ]] || { echo "missing $binary" >&2; exit 1; }
done
note "$(basename "$HOST"), $(basename "$BEE"), $(basename "$PROVIDER")"

# ── commission ───────────────────────────────────────────────────────────────

step "Commissioning a host from a throwaway \$HOME"
RELAY="wss://unreachable.invalid"
IDENTITY=$(cargo run --quiet -p beekeeper-host --example mint-test-identity)
NSEC=$(printf '%s' "$IDENTITY" | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["nsec"])')
PUBKEY=$(printf '%s' "$IDENTITY" | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["pubkey"])')
BASE_DIR="$FAKE_HOME/app-data/session-provider"
STATE_DIR="$BASE_DIR/$PUBKEY"
mkdir -p "$STATE_DIR" "$BASE_DIR/logs" "$FAKE_HOME/.local/state/buzz/host"

"$PYTHON" - "$BASE_DIR" "$PUBKEY" "$RELAY" <<'PY'
import json, pathlib, sys
base, pubkey, relay = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
(base / "coding-session-provider.json").write_text(json.dumps({
    "version": 1,
    "providers": {relay: {
        "providerPubkey": pubkey,
        "instanceId": pubkey[:16],
        "createdAt": "2026-09-30T00:00:00Z",
        "relayUrl": relay,
    }},
}, indent=2))
PY
"$PYTHON" - "$FAKE_HOME" "$BASE_DIR" "$PUBKEY" "$RELAY" <<'PY'
import json, pathlib, sys
home, base, pubkey, relay = (pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]),
                             sys.argv[3], sys.argv[4])
(home / ".local/state/buzz/host/host.json").write_text(json.dumps({
    "version": 1, "instance": "production", "relayUrl": relay,
    "providerPubkey": pubkey,
    "sessionProviderBaseDir": str(base),
    "providerStateDir": str(base / pubkey),
    "runtimes": [], "writtenAt": "2026-09-30T00:00:00Z",
}, indent=2))
PY
printf '%s\n' "$NSEC" >"$FAKE_HOME/.local/state/buzz/host/provider-key"
chmod 600 "$FAKE_HOME/.local/state/buzz/host/provider-key"
PROVIDER_LOG="$BASE_DIR/logs/$PUBKEY.log"

CHECK=$(HOME="$FAKE_HOME" BEEKEEPER_HOST_SOCK="$SOCK" "$HOST" check)
if grep -q "key from:      File" <<<"$CHECK"; then
    pass "the host resolves its identity from the 0600 key file, not a keychain"
else
    fail "the host did not resolve the key file"; note "$CHECK"
fi

# ── 1. the provider is the host's child, with no desktop anywhere ────────────

step "1. A provider runs as the host's child, with no desktop in the picture"
HOME="$FAKE_HOME" BEEKEEPER_HOST_SOCK="$SOCK" "$HOST" run >"$BASE/host.log" 2>&1 &
HOST_PID=$!
record_pid "$HOST_PID"

if wait_until "grep -q 'control socket listening' '$BASE/host.log'" 200; then
    pass "the control socket is bound before supervision starts"
else
    fail "the host never bound its control socket"; note "$(cat "$BASE/host.log")"
fi
if ! wait_until "[[ \$(pgrep -P $HOST_PID | wc -l) -ge 1 ]]" 200; then
    fail "the host never spawned a provider"; note "$(cat "$BASE/host.log")"
fi
CHILD=$(pgrep -P "$HOST_PID" | head -1)
record_pid "$CHILD"

# Linux truncates a process's comm to 15 bytes, so `beekeeper-session-provider`
# reads there as `beekeeper-sessi`; macOS reports the full path. Accept either.
PROVIDER_NAME="beekeeper-session-provider"
CHILD_COMM="$(ps -o comm= -p "$CHILD" 2>/dev/null | xargs basename 2>/dev/null || true)"
if [[ -n "$CHILD" ]] && { [[ "$CHILD_COMM" == "$PROVIDER_NAME" ]] || [[ "$CHILD_COMM" == "${PROVIDER_NAME:0:15}" ]]; }; then
    pass "the child is a real beekeeper-session-provider (pid $CHILD)"
else
    fail "the child is not a provider"; note "$(ps -o pid,ppid,comm= -p "${CHILD:-1}")"
fi
PPID_OF_CHILD=$(ps -o ppid= -p "$CHILD" | tr -d ' ')
if [[ "$PPID_OF_CHILD" == "$HOST_PID" ]]; then
    pass "the provider's parent is the agent host, not a desktop app"
else
    fail "the provider's parent is $PPID_OF_CHILD, not the host $HOST_PID"
fi
if ! pgrep -f "Beekeeper.app/Contents/MacOS/beekeeper-desktop" >/dev/null 2>&1; then
    pass "no desktop app is running, and the provider is up regardless"
else
    note "a Beekeeper.app is running on this machine; it is not this provider's parent"
    pass "the provider's parent is the host even with a desktop running"
fi
if grep -q "under agent host pid $HOST_PID" "$PROVIDER_LOG"; then
    pass "the provider's own log records which host started it"
else
    fail "the provider log does not name its host"; note "$(tail -3 "$PROVIDER_LOG" 2>/dev/null)"
fi

# ── 2. the socket answers while nothing else runs ────────────────────────────

step "2. bee host status answers for the live provider with no desktop running"
if [[ "$(provider_state)" == "live" ]]; then
    pass "status reports the provider live"
else
    fail "status did not report live"; note "$(host_status)"
fi
if [[ "$(provider_pid)" == "$CHILD" ]]; then
    pass "the pid in status is the process actually running ($CHILD)"
else
    fail "status pid $(provider_pid) != the real child $CHILD"
fi
RELAY_STATE=$(host_status | "$PYTHON" -c 'import json,sys; print(json.load(sys.stdin)["relayConnection"]["state"])')
if [[ "$RELAY_STATE" == "unknown" ]]; then
    pass "the host reports the relay as unknown rather than guessing from a live child"
else
    fail "the host claimed a relay state it cannot observe: $RELAY_STATE"
fi
OWNER_FILE="$STATE_DIR/host-owner.json"
if [[ -f "$OWNER_FILE" ]] && grep -q "\"hostPid\": $HOST_PID" "$OWNER_FILE"; then
    pass "the host recorded its ownership of the state directory"
else
    fail "no host-owner.json naming pid $HOST_PID"; note "$(cat "$OWNER_FILE" 2>/dev/null)"
fi
if [[ ! -f "$STATE_DIR/provider.lock" ]] || [[ "$(tr -d '[:space:]' <"$STATE_DIR/provider.lock")" =~ ^[0-9]+$ ]]; then
    pass "provider.lock is still a bare pid — the host wrote a separate file"
else
    fail "provider.lock is no longer one integer"; note "$(cat "$STATE_DIR/provider.lock")"
fi

# ── 3. a second host refuses rather than fighting ────────────────────────────

step "3. A second host refuses the state directory instead of starting a war"
HOME="$FAKE_HOME" BEEKEEPER_HOST_SOCK="$SOCK_B" "$HOST" run >"$BASE/host-b.log" 2>&1 &
SECOND=$!
record_pid "$SECOND"
if wait_until "grep -q 'refusing to take over' '$PROVIDER_LOG'" 120; then
    pass "the second host refused by name, and said which host holds it"
    note "$(grep -m1 'refusing to take over' "$PROVIDER_LOG")"
else
    fail "the second host did not refuse"; note "$(cat "$BASE/host-b.log")"
fi
if kill -0 "$CHILD" 2>/dev/null; then
    pass "the first host's provider was never signalled (still pid $CHILD)"
else
    fail "the second host killed the first host's provider — this is the restart war"
fi
kill -TERM "$SECOND" 2>/dev/null || true
wait "$SECOND" 2>/dev/null || true

# ── 4. the restart ladder ────────────────────────────────────────────────────

step "4. Killing the provider makes the host restart it, on the ladder"
kill -9 "$CHILD"
if wait_until "grep -q 'restarting' '$BASE/host.log'" 120; then
    LADDER=$(grep -m1 'restarting' "$BASE/host.log" | strip_ansi)
    if grep -q 'attempt=1' <<<"$LADDER" && grep -q 'delay_secs=2' <<<"$LADDER"; then
        pass "the host logged the first rung of the ladder (attempt 1, 2s)"
    else
        fail "the ladder's first rung is not attempt=1 delay_secs=2"; note "$LADDER"
    fi
else
    fail "the host never logged a restart"; note "$(cat "$BASE/host.log")"
fi
if wait_until '[[ "$(provider_state)" == "live" ]]' 200; then
    NEW_CHILD=$(provider_pid)
    record_pid "$NEW_CHILD"
    if [[ -n "$NEW_CHILD" && "$NEW_CHILD" != "$CHILD" ]]; then
        pass "a new provider is live with a different pid ($CHILD → $NEW_CHILD)"
    else
        fail "the pid did not change after a restart"
    fi
else
    fail "the provider never came back"; note "$(cat "$BASE/host.log")"
fi

# ── 5. stop is a request, and the host outlives it ───────────────────────────

step "5. bee host stop ends the provider and leaves the host answering"
STOP=$(BEEKEEPER_HOST_SOCK="$SOCK" HOME="$FAKE_HOME" "$BEE" host stop 2>&1)
if grep -q '"stopped": true' <<<"$STOP"; then
    pass "stop reported that it stopped something"
else
    fail "stop did not report stopping"; note "$STOP"
fi
if wait_until "! kill -0 $NEW_CHILD 2>/dev/null" 200; then
    pass "the provider is gone — stop waited for it, rather than answering early"
else
    fail "the provider is still alive after stop returned"
fi
if [[ "$(provider_state)" == "notSupervised" ]]; then
    pass "the host is still answering, and says notSupervised rather than going silent"
else
    fail "the host's state after stop is $(provider_state)"
fi
if [[ ! -f "$OWNER_FILE" ]]; then
    pass "the ownership claim was released, so the next host takes over cleanly"
else
    fail "host-owner.json survived a stop"; note "$(cat "$OWNER_FILE")"
fi

# ── 6. a clean exit is not revived ───────────────────────────────────────────

step "6. A provider that exits 0 is not restarted (docs/remote-agents.md § I5)"
CLEAN="$BASE/clean-exit-provider"
cat >"$CLEAN" <<'EOF'
#!/bin/sh
echo "fake provider: exiting cleanly"
exit 0
EOF
chmod +x "$CLEAN"
"$PYTHON" - "$FAKE_HOME" "$CLEAN" <<'PY'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1]) / ".local/state/buzz/host/host.json"
c = json.loads(p.read_text()); c["providerCommand"] = sys.argv[2]
p.write_text(json.dumps(c, indent=2))
PY
BIND=$(BEEKEEPER_HOST_SOCK="$SOCK" HOME="$FAKE_HOME" "$BEE" host bind 2>&1 || true)
BEEKEEPER_HOST_SOCK="$SOCK" HOME="$FAKE_HOME" "$BEE" host start >/dev/null 2>&1 || true
if wait_until "grep -q 'exited cleanly; the host is not restarting it' '$PROVIDER_LOG'" 200; then
    pass "the host logged that it will not revive a clean exit"
else
    fail "the host did not record the clean-exit rule"; note "$(tail -4 "$PROVIDER_LOG")"
fi
sleep 3   # longer than the ladder's first rung: a revival would have happened.
REVIVALS=$(grep -c "fake provider: exiting cleanly" "$PROVIDER_LOG" || true)
if [[ "$REVIVALS" -le 1 ]]; then
    pass "it ran once and was not respawned ($REVIVALS start(s) logged)"
else
    fail "a clean exit was revived $REVIVALS times"
fi

# ── 7. the host stops when asked, and takes its child with it ────────────────

step "7. SIGTERM stops the host itself"
kill -TERM "$HOST_PID"
if wait_until "! kill -0 $HOST_PID 2>/dev/null" 200; then
    pass "the host exited on SIGTERM, as a service manager would stop it"
else
    fail "the host ignored SIGTERM"
fi
if [[ ! -S "$SOCK" ]]; then
    pass "the control socket was removed, so a client sees 'not running'"
else
    fail "a stale socket survived, which reads as a running host"
fi

# ── result ───────────────────────────────────────────────────────────────────

printf '\n══ %d passed, %d failed\n' "$PASS" "$FAIL"
if ((FAIL > 0)); then exit 1; fi
