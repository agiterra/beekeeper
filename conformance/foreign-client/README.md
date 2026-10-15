# Foreign protocol clients

Minimal Nostr clients that observe and drive a Beekeeper relay using **only**
Node built-ins and [`nostr-tools`](https://github.com/nbd-wtf/nostr-tools): no
Beekeeper SDK, no host socket, no service files, no desktop layout. They are
the "outside" view the acceptance stories in
[`docs/INTEGRATING.md`](../../docs/INTEGRATING.md) are told from, and
`scripts/foreign-client-check.sh` (`just foreign-client-check`) runs them
against an isolated relay and asserts the documented outcomes.

If you are starting from zero, read `docs/INTEGRATING.md` first: it gets a
host and an agent running with `bee`, then hands over to these scripts for
the view a client of your own will have.

They were lifted from the 2026-10-11 headless acceptance run (agents
repository, `plans/archive/2026-10-11-headless-extensibility/foreign-*.mjs`
and `collect-conversation.mjs`). Two things changed: the hardcoded
`127.0.0.1:3037` guard became a `RELAY_URL` environment variable with the
same safety (non-loopback is refused unless `FOREIGN_CLIENT_ALLOW_REMOTE=1`),
and the keys, channel and output directory come from the environment instead
of files beside the script.

## Configuration

| variable | meaning |
| --- | --- |
| `RELAY_URL` | `http(s)://` or `ws(s)://` relay origin; default `http://127.0.0.1:3037`. Non-loopback hosts are refused unless `FOREIGN_CLIENT_ALLOW_REMOTE=1`. |
| `FOREIGN_CLIENT_KEY` | 64-hex private key. Alternatives: `FOREIGN_CLIENT_KEY_FILE` (an env file with `BEEKEEPER_PRIVATE_KEY=`), then `BEEKEEPER_PRIVATE_KEY`. Never printed. |
| `FOREIGN_CLIENT_CHANNEL` | lowercase channel UUID the reads and writes are scoped to |
| `FOREIGN_CLIENT_OUT` | directory for the JSON records each script writes; default `.` |
| `FOREIGN_CLIENT_LABEL` | identity label used in `foreign-session-<label>-<action>.json`; default `client` |
| `FOREIGN_CLIENT_RELAY_PUBKEY` | the relay `self` key you witnessed through a trusted origin; without it `foreign-hook.mjs` takes NIP-11 `self` from the same relay and says so (`witnessed: false`) |
| `FOREIGN_CLIENT_NOSTR_TOOLS` | a directory or `package.json` from which `nostr-tools` resolves; default this repository's `desktop/` install |

Every script prints exactly one JSON line on success and writes its full
signed record under `FOREIGN_CLIENT_OUT`. A relay **refusal is a result, not
an error**: `foreign-session.mjs stop` with an ungranted key prints the 400
and exits 0, so a harness can assert on it. Scripts exit non-zero only for an
invalid signature in a relay answer, a malformed answer, a missing input, or
(`foreign-hook.mjs correlate`) an incomplete exchange (exit 3).

## Scripts

| script | does |
| --- | --- |
| `foreign-info.mjs [kind …]` | unauthenticated NIP-11 read: `supported_kinds`, `self`, build; asserts the named kinds are advertised; shows an unsigned `/query` is refused |
| `foreign-identity.mjs` | prints the public key of the configured key, nothing else |
| `foreign-session.mjs read` | NIP-98 read of 44222–44225 in the channel, every signature verified |
| `foreign-session.mjs <stop\|resume\|restart> <operation> '<target json>' <authority>` | publish one kind:44221 lifecycle command for an exact public target ([NIP-CSL](../../docs/nips/NIP-CSL.md)) |
| `foreign-native-read.mjs '<filter json>' <out>` | any authenticated filter, verified; `kinds` is mandatory |
| `foreign-native-control.mjs <steer\|stop\|resume\|restart> '<spec json>'` | kind:44220 steer or kind:44221 lifecycle command, with the `cs-target` key computed locally |
| `foreign-hook.mjs correlate <runId>` | read 46013/46014/46022/46023, verify, and prove request → accepted claim → result → relay echo for one run ([NIP-HS](../../docs/nips/NIP-HS.md)) |
| `foreign-hook.mjs claim <runId> <stepId> <requestedEventId>` | claim a request and, if admitted, answer it with a truthful `refused` result — this client executes nothing |
| `collect-conversation.mjs [label]` | collect and verify kinds 9, 44220/1/3/4/5, 44234 in the channel |

`_common.mjs` holds the shared plumbing: the relay-URL guard, key loading,
`nostr-tools` resolution, the NIP-98 `POST` helper, signature verification and
the `cs-target` key encoding. It imports nothing from Beekeeper either.

## What this proves, and does not

Each run proves the **selected wire contract** is reachable and honest from
outside: the relay authenticates NIP-98, scopes reads to membership, admits a
member's signed intent, refuses a non-member's by name, advertises what it
speaks, and correlates a host-step exchange it signed. It does not prove a
foreign *runtime*: no script here executes a command, holds a session, or
recovers one. `foreign-hook.mjs claim` says `FOREIGN_EXECUTION_UNSUPPORTED`
rather than pretending.

The offline counterpart — signature and correlation checks over an exported
exchange, in standard-library Python — is
[`conformance/headless-host-step/`](../headless-host-step/README.md).
