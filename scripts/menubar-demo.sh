#!/usr/bin/env bash
# menubar-demo.sh — see the menu bar app, without installing anything.
#
# Runs the *bundled* agent host and the *bundled* menu bar app against a
# throwaway `$HOME`, on a socket under /tmp, and pushes a couple of
# managed-agent rows so the menu has something in it. Touches nothing in your
# real home: no LaunchAgent, no keychain, no app-data directory.
#
# The relay in the throwaway commissioning is unreachable on purpose, so the
# provider retries forever and no *coding sessions* appear. The rows you see
# are pushed ones — which is the same path Beekeeper uses for managed agents.
#
# Ctrl-C to stop. Everything it started dies with it.

set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$REPO/desktop/src-tauri/target/release/bundle/macos/Beekeeper.app"
HOST="$APP/Contents/MacOS/beekeeper-host"
MENUBAR="$APP/Contents/Library/LoginItems/Beekeeper Menu Bar.app/Contents/MacOS/beekeeper-menubar"
FAKE_HOME="${MENUBAR_DEMO_HOME:-}"
SOCK=/tmp/beekeeper-menubar-demo.sock

for path in "$HOST" "$MENUBAR"; do
    [[ -x "$path" ]] || {
        echo "missing $path" >&2
        echo "Build it first: ./scripts/stage-menubar.sh && (cd desktop && pnpm tauri build --bundles app --config src-tauri/tauri.local-prod.conf.json)" >&2
        exit 1
    }
done
[[ -n "$FAKE_HOME" && -f "$FAKE_HOME/.local/state/buzz/host/host.json" ]] || {
    echo "set MENUBAR_DEMO_HOME to a throwaway \$HOME holding a commissioned host.json" >&2
    exit 2
}

rm -f "$SOCK"
cleanup() { kill %1 %2 2>/dev/null || true; rm -f "$SOCK"; }
trap cleanup EXIT

echo "→ starting the bundled agent host"
BEEKEEPER_HOST_SOCK=$SOCK HOME="$FAKE_HOME" "$HOST" run 2>&1 | sed 's/^/   host: /' &
for _ in $(seq 1 40); do [[ -S "$SOCK" ]] && break; sleep 0.25; done
[[ -S "$SOCK" ]] || { echo "the host never bound its socket" >&2; exit 1; }

echo "→ starting the bundled menu bar app — look for the hat in your menu bar"
BEEKEEPER_HOST_SOCK=$SOCK HOME="$FAKE_HOME" "$MENUBAR" 2>&1 | sed 's/^/   menubar: /' &
sleep 2

echo "→ pushing two agent rows, so the menu has something in it"
NOW_MS=$(( $(date +%s) * 1000 ))
python3 - "$SOCK" "$NOW_MS" <<'PY'
import json, socket, sys
sock, now = sys.argv[1], int(sys.argv[2])
rows = [
    {"activityId": "demo-1", "agentName": "Scout", "agentPubkey": "a" * 64,
     "channelId": "11111111-1111-1111-1111-111111111111",
     "channelName": "planning", "startedAtMs": now - 192_000},
    {"activityId": "demo-2", "agentName": "Builder", "agentPubkey": "b" * 64,
     "channelId": "22222222-2222-2222-2222-222222222222",
     "channelName": "mobile", "startedAtMs": now - 68_000},
    {"activityId": "demo-3", "agentName": "Reviewer", "agentPubkey": "c" * 64,
     "channelId": "33333333-3333-3333-3333-333333333333",
     "channelName": "design", "startedAtMs": now - 265_000, "recent": True},
]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock)
s.sendall((json.dumps({"op": "push-activity", "rows": rows}) + "\n").encode())
print("   " + s.recv(65536).decode().strip())
PY

cat <<'NOTE'

Click the hat. You should see:

  Agent host: running · 2 agents     (a disabled header — the true state)
  Scout · 3m 12s                     (with #planning underneath, on macOS 14+)
  Builder · 1m 8s                       …and the seconds ticking
  ─────────
  Recent
  Reviewer · 4m 25s
  ─────────
  (a notice: the provider has not written state.json yet — true, the relay
   in this throwaway commissioning is unreachable)
  ─────────
  Open Beekeeper · Open Agent Log… · Restart Agents · Stop Agents…
  ─────────
  Quit Menu Bar
  Quitting this leaves your agents running

Worth checking specifically:
  - the clock ticks without the host being polledper second (it is asked every 5s)
  - "Quit Menu Bar" does not offer to stop your agents, and says so
  - the rows disappear ~20s after this script stops pushing — that is the
    lease, and it is how they vanish when Beekeeper quits

Ctrl-C when you are done.
NOTE

wait
