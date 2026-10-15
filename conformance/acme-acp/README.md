# acme-acp — a plugged runtime's stub adapter

`acme-acp` is a stub [ACP](https://agentclientprotocol.com) v2 agent in
standard-library Python, speaking JSON-RPC 2.0 over newline-delimited stdio.
It stands in for a runtime Beekeeper has never heard of — the `acme` word of
[`docs/runtime-descriptor.md`](../../docs/runtime-descriptor.md) — so that
`just headless-acme-check` (`scripts/headless-acme-check.sh`) can prove the
whole plug end to end with real processes: an isolated relay, a freshly
commissioned `beekeeper-host`, its supervised `beekeeper-session-provider`,
and this adapter started inside the generic managed boundary with only the
credential the descriptor permits.

Round 1 proved the same descriptor through the provider's in-process harness
(`crates/beekeeper-session-provider/src/runtime_descriptor_plug_tests.rs`).
This is the out-of-process half: the CLI commissions it, the host spawns the
provider, the provider spawns this file. The check script is also the
executable form of the ten-minute path in
[`docs/INTEGRATING.md`](../../docs/INTEGRATING.md): commission, run the host,
create, message, read presence and receipts, stop — with this stub where a
real adapter would be. To plug your own runtime, write an adapter that
answers what this one answers and commission it the same way.

## What it says it can do

| request | answer |
| --- | --- |
| `initialize` | `protocolVersion: 2`, `agentCapabilities.loadSession: false`, no image prompts, no `_meta.steering.supported`. |
| `session/new` | one session id (`acme-session-<n>`). |
| `session/prompt` | one `agent_message_chunk` then `stopReason: "end_turn"`. |
| `session/cancel` | ends an open turn with `stopReason: "cancelled"`; silent otherwise. |
| `session/load`, `session/resume`, `_session/steering`, `session/set_model`, `authenticate`, anything else | JSON-RPC `-32601` method not found. |

Nothing is faked: unsupported is refused by code, never answered with an
empty success.

## How it proves credential custody without leaking it

The reply text is

```
acme answered prompt <n>: ACME_TOKEN sha256 prefix <16 hex>
```

where the 16 hex characters are the prefix of `sha256(ACME_TOKEN)` as the
process sees it, or the literal `unset` when the variable is absent. The
check script computes the same prefix from the secret it wrote into the 0600
credential file and asserts equality, and separately asserts the secret
itself appears in no published event. That is the descriptor's
`credentialEnv: ["ACME_TOKEN"]` leg, observed from outside the process.

Two things the published copy of that line shows that the stub did not
write: the provider's credential redactor masks the word after
`ACME_TOKEN` (so the channel reads `ACME_TOKEN •••••••• prefix <hex>`; the
`token` key class in `coding_session_context.rs` does not know `sha256` is
not a value), and the provider narrates the turn in the same thread under
the actor's key (`Request turn_queued.`, `Request turn_started.`, the
disclosed `STEER_UNSUPPORTED` boundary delivery every mention gets on a
runtime with no native steer, `Turn ended (success): completed`). The
check identifies the stub's answer by the witness, not by being the only
post.

## Running it by hand

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":2}}' \
  '{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}' \
  '{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"acme-session-1","prompt":[{"type":"text","text":"hi"}]}}' \
  '{"jsonrpc":"2.0","id":4,"method":"session/load","params":{}}' \
  | ACME_TOKEN=example conformance/acme-acp/acme-acp
```

The fourth line is answered `-32601`. Needs `python3` on `PATH` (the
shebang is `/usr/bin/env python3`; inside the macOS boundary that resolves
to `/usr/bin/python3`).
