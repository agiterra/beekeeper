# The agent host

`beekeeper-host` is the process that runs this machine's agents. It starts at
login, survives every Beekeeper launch, quit and update, and installs on a
server with no GUI anywhere.

Quitting Beekeeper is a window closing. It is not a shutdown.

```
login (launchd / systemd)        login (launchd)
        │                               │
        ▼                               ▼
  beekeeper-host ──────────►  Beekeeper Menu Bar.app
  (headless daemon)  host.sock  (LSUIElement, tray only)
        │                  ▲
        │ child process    │ host.sock
        ▼                  │
  buzz-session-provider ───┴────────  Beekeeper.app
        │                             (desktop client)
        │ one per coding session
        ▼                            All three also talk to
  claude-agent-acp                   the relay independently
```

The host is the only process that supervises the provider. The menu bar app
and the desktop app are both *clients* of its socket, and both read agent
content from the relay as they always did.

## What it does and does not own

**Owns:** the coding-session provider (`buzz-session-provider`) — spawning it,
restarting it on the backoff ladder, stopping it with SIGINT so its durable
outbox flushes, and taking its state directory over from a stale owner.

**Does not own:** managed agents (`buzz-acp`). Those are still the desktop
app's children and still end when it quits. That is a known limit of this
landing, not an accident; the menu bar app shows them while Beekeeper is
running by asking it, and says out loud when it cannot.

**Cannot see:** the provider's relay connection. The host *supervises* the
provider rather than linking it in, so it holds no relay socket of its own and
reports `relayConnection: unknown` with a reason. The desktop answers that
question, from its own connection plus the provider's published kind:44222
catalog. Synthesising "connected" from "the child is alive" would be a guess.

## On a Mac

**Beekeeper asks; it does not help itself.** Something that starts at every
login, forever, is a change to your machine, so the first time the app finds a
commissioned provider and no registration it puts the question: *Keep your
agents running?* Saying yes installs two login items from inside the bundle —
the agent host and the menu bar icon — and starts them immediately, without a
logout.

Saying no is recorded, in `~/.local/state/buzz[-dev]/host/login-refused.json`,
and nothing asks again. **Settings → Coding sessions → Running when Beekeeper
is closed** is where the answer lives afterwards, in both directions.

Only the refusal is stored. A grant is the registration itself, so there is no
second copy of "they said yes" to fall out of step with the plist — and after
you have said yes once, an app update that moves the binary the registration
names is repaired silently rather than re-proposed. `bee host uninstall`
records the refusal too, so an uninstall from a terminal is not undone by the
next launch.

**Owed:** the app's reset path does not clear `~/.local/state/buzz[-dev]/host/`
at all — not `host.json`, not `provider-key`, and not the refusal — so a reset
machine keeps whatever it had, including a "no" nobody can see. The code that
would do it exists (`agent_host::autostart::unregister`, which clears the
refusal rather than recording one, precisely because a reset is not a
refusal); wiring it into `reset.rs` is a separate change.

To check:

```bash
bee host installed     # both services, and anything wrong with either
bee host status        # what the running host says about the provider
```

## On a server

There is no GUI and no app, so the identity is commissioned on a machine that
has one and copied across.

```bash
# 1. Build and install the binaries somewhere your shell and systemd can see.
just install-bee

# 2. Register the service.
bee host install

# 3. A user unit dies at logout. Without this, the host stops the moment you
#    disconnect — the single most common way this silently fails.
loginctl enable-linger "$USER"
```

Then give it an identity. Two files, both written by hand here:

`~/.local/state/buzz/host/host.json`

```json
{
  "version": 1,
  "instance": "production",
  "relayUrl": "wss://hive.example.org",
  "providerPubkey": "<64 lowercase hex>",
  "sessionProviderBaseDir": "/home/agent/.local/share/beekeeper/session-provider",
  "providerStateDir": "/home/agent/.local/share/beekeeper/session-provider/<providerPubkey>",
  "runtimes": [],
  "writtenAt": "2026-09-30T00:00:00Z"
}
```

`<sessionProviderBaseDir>/coding-session-provider.json` — the record store,
copied from the machine that provisioned the identity, and
`~/.local/state/buzz/host/provider-key` — the nsec, `chmod 600`.

```bash
beekeeper-host check      # reads everything back, and prints where the key
                          # came from. Never the key.
systemctl --user start beekeeper-host
bee host status
```

`beekeeper-host check` is the command to run when something is wrong. It names
the file it wanted and what to do about it, for every way a commissioning can
be incomplete.

## Where the key lives, and what that costs

**The host is never a keychain client.** That is a rule, and the reason is
structural. The desktop keeps every secret in one keychain entry through the
*legacy* SecKeychain API, deliberately, so signed release builds and unsigned
dev builds share one store (`desktop/src-tauri/src/secret_store.rs`). Legacy
items carry a per-item ACL keyed to a code signature, and
[local-desktop-instances.md](local-desktop-instances.md) already records the
consequence: the ACL is bound to the binary's signature, which changes per
build. A daemon reading that entry would mean a GUI prompt at login, a human
at the keyboard, and a grant that dissolves on the next rebuild. A server has
no keychain at all.

So the app hands the key over at commissioning and the host resolves it from
its own routes, in order:

1. `BEEKEEPER_HOST_PRIVATE_KEY` — the container path.
2. `BEEKEEPER_HOST_KEY_FILE`, or `~/.local/state/buzz[-dev]/host/provider-key`
   — a `0600` file, the only route that works headless.
3. The record's own inline nsec, when the app left one there (a build with no
   keyring backend, or a keyring outage).

**The cost, stated plainly:** on macOS the provider nsec's at-rest protection
drops from an ACL-gated keychain item to a `0600` file under `$HOME`. Three
facts bound that:

- it is the same protection `nokeyring` dev builds already use for this key
  and for managed-agent keys;
- it is the same protection the provider's **state directory** already has — a
  `0700` directory holding a durable outbox of *pre-signed events*, so
  compromising that directory already allows publishing as the provider;
- `BEEKEEPER_HOST_KEY_FILE` points at a secrets mount or a tmpfs file with no
  code change.

The keychain entry is not deleted. It remains the recovery path.

When no route resolves, the host **refuses to start the provider** and says
which routes it tried and the path it expected. It does not mint a
replacement: a new key would strand every event the real one signed.

## The control socket

`~/.local/state/buzz[-dev]/host/host.sock` — `0600` in a `0700` directory,
newline-delimited JSON, one request per connection, and the peer's uid must
match the host's. `BEEKEEPER_HOST_SOCK` overrides the path.

It carries **launcher-local facts only**: is the host running, its child's pid,
its logs, start and stop. Agent turns, steering and `!shutdown` stay on the
relay. See [remote-agents.md § A launcher's own control surface](remote-agents.md#launcher-control-surface)
for why that is not a substrate control channel.

| op | does |
| --- | --- |
| `hello` | protocol version, host version, host pid |
| `status` | everything below, in one round trip |
| `logs` | a bounded tail of the provider's log |
| `start` / `stop` / `restart` | the provider child; the host keeps running |
| `bind` | re-read `host.json` — the community switch |
| `adopt-identity` | re-read the key file after the app wrote it |
| `push-activity` | the app contributes its managed-agent rows, under a lease |

The secret never crosses the socket. The app writes the `0600` file and
`adopt-identity` only says "look again", so a socket capture reveals nothing
signable.

## Reading a status

Four situations must never be confused, and `bee host status` distinguishes
all four:

| | socket | registration | means |
| --- | --- | --- | --- |
| not installed | absent | absent | run `bee host install` |
| installed, not running | absent | present | start it; `beekeeper-host run` in a terminal shows why it exits |
| running, no provider | answers | — | the `provider.state` says which: `keyUnresolved`, `backoff`, `gaveUp`, `lockHeldElsewhere`, `notSupervised` |
| running, relay unreachable | answers, child live | — | the host cannot tell; Beekeeper can |

`provider.state` values that are *not* failures:

- **`backoff`** — the host is restarting it, and says which attempt. A child
  that crashes comes back on a doubling delay, up to five times in ten
  minutes.
- **`notSupervised`** — nothing is commissioned, or somebody stopped it.

And two that are:

- **`gaveUp`** — it crashed too often, too fast. `bee host logs` says why.
  Nothing will start it again until you ask.
- **`lockHeldElsewhere`** — somebody else owns the provider's state directory.
  Usually a pre-upgrade Beekeeper still supervising its own child. The host
  refuses to fight for it rather than signalling whoever holds it, so neither
  side restarts the other's provider in a loop.

A provider that exits **cleanly** is never revived
([remote-agents.md](remote-agents.md) § I5). That is deliberate: reviving it
would make an intentional shutdown impossible to observe.

## Logs

Two files, and they answer different questions.

- **The provider's** — `<sessionProviderBaseDir>/logs/<providerPubkey>.log`,
  via `bee host logs`. What the agents did, plus the host's `=== … ===`
  lifecycle markers for every start, exit, restart and takeover.
- **The host's own** — `~/.local/state/buzz[-dev]/host/host.log` under
  launchd, or the journal under systemd. What the host decided.

`RUST_LOG` is respected, and `beekeeper_host=info` is appended unless you name
`beekeeper_host` yourself — so a filter aimed at another crate cannot silence
the service's own lifecycle lines.

## Two instances on one machine

A dev build and a release build register, listen and store separately:
`~/.local/state/buzz/host/` against `~/.local/state/buzz-dev/host/`, and
`io.agiterra.beekeeper.host` against `io.agiterra.beekeeper.host.dev`. The
host is *told* which it is — `BEEKEEPER_HOST_INSTANCE=production|dev`, written
into its registration — rather than sniffing, because a launchd-started
process has no app-data directory to infer it from.

## Testing it

```bash
just test-host    # composes a real host, a real provider, a real socket and a
                  # second host competing for the same state directory
```

`scripts/host-acceptance.sh` names at its top what it does *not* prove: no
relay runs, so it says nothing about transcript items reaching a channel; there
is no `Beekeeper.app`, so "the session survived the app" is proven only in its
mechanical half; and it never loads a real LaunchAgent, because that would
register a background service on whoever ran it.
