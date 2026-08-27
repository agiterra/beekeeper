# NIP-CSC: Coding-Session Commands

`kind:44220` is a durable, channel-scoped command from a Buzz operator to a
coding-session provider adapter. It is a storage and fan-out event: the relay
validates it and stores it, but does not route it through `command_executor`
and never executes it.

Session creation uses the companion
[NIP-CSL lifecycle contract](NIP-CSL.md) (`kind:44221`); after creation,
NIP-CSC addresses an existing exact provider session generation. Provider
capability advertisement uses [NIP-CSPC](NIP-CSPC.md) (`kind:44222`), and
signed provider-neutral transcript facts use [NIP-CST](NIP-CST.md)
(`kind:44225`).

> **Fork amendment — native kinds only.** This fork owns its relay, so
> `kind:44220` is the only wire form. The donor contract described a kind-9
> compatibility fallback for older relays that do not recognize the kind; that
> fallback does not exist here, in the client, the producer, or the relay. A
> rejected 44220 is a failure, never a signal to re-sign as chat.

## Wire contract

The event content is exactly this JSON shape (no additional fields):

```json
{
  "schema": "buzz-coding-session-command/v1",
  "commandId": "client-idempotency-id",
  "target": {
    "driver": "capability-advertised-slug",
    "instanceId": "provider-instance",
    "sessionId": "provider-session",
    "generation": 1
  },
  "action": {
    "type": "thread.turn.start",
    "text": "operator instruction",
    "deliver": "boundary"
  }
}
```

`driver` is an open capability-driven slug, not a Buzz enum. `commandId` and
all target identifiers are nonempty after trimming and at most 256 UTF-8 bytes.
Start-action text is nonempty after trimming and at most 12 KiB (12,288 UTF-8
bytes). `deliver` is an **optional** key on `thread.turn.start` — absent means
`"boundary"` — whose value is one of exactly `"boundary"`, `"steer"`, or
`"interrupt"`; any other value makes the command malformed, never a silent
default. `thread.turn.interrupt` carries no `deliver` key: the action is the
class. Producers on this fork write `deliver` explicitly even though it has a
default, so a reader of a signed command never has to know the default to know
what was asked for. `generation` is a positive JavaScript-safe integer
(≤ 9,007,199,254,740,991) — the bound is JSON's, not Rust's, because the
consumer is JavaScript and a `u64` that survives a Rust round-trip but loses
precision in the browser would silently address a different session.

Tags are exactly one of each:

- `h`: the channel UUID
- `cs-v`: `csc1-1`
- `cs-target`: `coding-session/v1|` followed by byte-length-prefixed `driver`,
  `instanceId`, `sessionId`, and decimal `generation`

The structured-key encoding is length-prefixed rather than delimiter-joined
because session ids and driver slugs are arbitrary strings: joining on a
separator would let a field containing that separator forge a different
tuple's key. The golden vector for
`(provider-a, instance-1, session-1, 1)` is:

```
coding-session/v1|10:provider-a10:instance-19:session-11:1
```

The relay re-derives `cs-target` from the decoded content and rejects any
mismatch. A command whose tag names one generation and whose content names
another would steer a session the operator never addressed; the tag is what
adapters route on, so the two can never be allowed to disagree.

The command author is the signed event pubkey. Consumers must never use JSON
content as claimed operator attribution.

### `commandId` correlates the receipt and the echo

A `thread.turn.start` command's `commandId` is the join key for everything
that command produces downstream: the provider's per-stage
[NIP-CSL](NIP-CSL.md) `kind:44224` receipts (`turn_queued`, `turn_started`,
`turn_degraded`, `turn_dropped`, `turn_refused`; and `interrupt_delivered` for
a `thread.turn.interrupt`) and the [NIP-CST](NIP-CST.md) `kind:44225`
`user_prompt` transcript item that opens the turn, which carries this same
`commandId` when present. A consumer that wants to know what became of one
signed command reads both streams keyed on it, rather than matching prompt
text or polling.

### Fork amendment: delivery classes

A turn command says *when* it wants to reach an execution that may already be
working. The sender chooses; the provider executes what it can and reports
what it did in a [NIP-CSL](NIP-CSL.md) receipt. There are three classes.

- **`boundary` (default).** The turn is held in the provider's mailbox and run
  when the current turn settles. This is the only class a client may assume
  works, and the one every provider on this contract implements.
- **`steer`.** The turn is injected into the *running* turn where this
  execution's runtime advertised native steering at initialize. Steering
  capability is a fact about the execution, not about the driver slug, and it
  is published per execution as `capabilities.threadSteer` in the
  `kind:44223` metadata. **Boundary-only in this build:** see the
  implementation-status note under the downgrade rule — native injection is
  deferred until an adapter advertises it, so no execution publishes
  `capabilities.threadSteer: true` today.
- **`interrupt`.** The provider cancels the running turn and then delivers this
  turn at the boundary that creates. Authority for this class is the **session
  founder only**; any other signer — including an operator holding
  `grant-operator` — is refused with `turn_refused` /
  `UNAUTHORIZED_OPERATOR` and the running turn is left alone. Stated exactly,
  because the boundary is narrower than it looks: what is founder-only is the
  *class on a `thread.turn.start`*. A `thread.turn.interrupt` command is open
  to any signer who may steer the execution, so a granted operator can still
  cancel a running turn and then send an ordinary `boundary` turn. Tightening
  that is authority work; until it happens, no document here should imply
  otherwise.

**The downgrade rule.** A `steer` addressed to an execution whose runtime does
not offer native steering is *not* refused and *not* escalated. The provider
publishes `turn_degraded` (`STEER_UNSUPPORTED`) naming the command, and then
treats the turn exactly as `boundary` — so the ordinary `turn_queued` follows.
It never cancels or restarts the running turn to make room, and it never
merges the two turns' text: cancel-and-merge loses work that has already been
done, and doing it silently on a sender's behalf is worse than saying "not
here, at the boundary instead". A consumer that does not understand
`turn_degraded` still sees the `turn_queued` and is merely less informed, never
wrong.

**Implementation status, 2026-08-26: `steer` is boundary-only in this build;
native injection is deferred until an adapter advertises it.** The rule above
is the contract, not a description of what ships today. In this fork's provider
`NATIVE_STEER_DELIVERABLE` is `false`
(`crates/buzz-session-provider/src/session.rs:78`) and `metadata_for`
AND-gates the per-execution witness with it
(`crates/buzz-session-provider/src/lib.rs:2845-2852`), so
`capabilities.threadSteer` is `false` for **every** execution and the desktop
composer never sends `deliver: "steer"`
(`desktop/src/features/coding-sessions/ui/CodingSessionComposer.tsx:295`
selects `steer` only when `canSteer`).

**`turn_degraded` is not unreachable here, and a consumer that skips decoding
it is wrong about what this relay will hand it.** What the paragraph above
establishes is only that *this fork's desktop* never asks for a steer, so no
`turn_degraded` originates from it. A `steer` from any other client is accepted
on the wire — the envelope validates every delivery class and the absent
field (`crates/buzz-relay/src/handlers/ingest.rs:7474-7479`: `None`,
`boundary`, `steer`, `interrupt`) — and the
provider then degrades it out loud: `inject_native_steer` returns `false`
unconditionally (`crates/buzz-session-provider/src/lib.rs:2264-2276`) and the
arm behind the delivery publishes `turn_degraded` / `STEER_UNSUPPORTED` beside
the `turn_queued` (`crates/buzz-session-provider/src/lib.rs:2116-2124`). That
degrade path has unit coverage with a hand-injected capability and no
end-to-end evidence, which is why native injection is called deferred; it is
not why the receipt is called impossible, because it is not.

Native mid-turn injection needs the `buzz-acp` steer types re-exported (`mod
pool` is private at `crates/buzz-acp/src/lib.rs:13`) and is a later slice's
work. Do not read this section as "steer shipped".

An unknown `deliver` value is a malformed command, rejected by the relay's
envelope validation. Defaulting an unreadable class to `boundary` would take a
turn the sender asked to interrupt with and quietly park it behind an hour of
work.

### Fork amendment: the relay is the mailbox

A `thread.turn.start` is durable on the relay from the moment it is accepted,
and the provider's in-memory queue is a cache of it, never the record. Three
consequences bind providers:

1. **A command is consumed when its turn *starts*, not when it is received.**
   A command accepted into the mailbox and not yet started is still
   unconsumed, so a provider that dies between the two loses nothing.
2. **On restart, a provider replays every unconsumed command from its
   watermark, in `(created_at, id)` order**, and delivers them in that order.
   A command whose turn already started is ignored as already-consumed, so a
   replay can never run a turn twice.
3. **The guarantee is "never silently lost", not "eventually run".** A replayed
   command addressed to an execution with no live process is answered with a
   terminal `turn_dropped` (`NO_LIVE_EXECUTION`) and recorded as refused.
   Resuming that session does not deliver it: `session.resume` mints a new
   generation, and the replayed command still names the old one. The sender
   sees a signed receipt saying so and sends it again.

A client therefore does not need to hold an unsent turn in memory to deliver
it later: publishing it with `deliver: "boundary"` is strictly safer, because
the relay survives the client, the provider, and the machine either runs on.
The consequence for a person is that a published turn cannot be recalled — the
`turn_dropped`/`turn_refused` receipts are what say a turn will not run, and
there is no client-side unsend.

## Authority

The relay requires `messages:write`, a valid `h` channel scope, and an actual
active channel membership row. **Open-channel visibility does not authorize a
coding-session command.** Permission to read a room is not authority to steer
an agent that runs commands against someone's checkout, so the membership gate
here is strictly stronger than the one ordinary chat writes take. A client-side
membership check is fail-closed UI preflight; the relay is authoritative.

`kind:44220` is never global — it requires the `h` tag, which is what makes it
inherit the channel ACL (including private-project access) on the read path.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constant | `crates/buzz-core/src/kind.rs` |
| Payload + target key | `crates/buzz-core/src/coding_session_command.rs` |
| Desktop builder | `desktop/src/features/coding-sessions/lib/codingSessionCommand.ts` |
| Envelope validation | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builder | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |

## Deploying the `deliver` key

**Relay before desktop.** The relay validates `kind:44220` content with
`deny_unknown_fields` (`crates/buzz-relay/src/handlers/ingest.rs`,
`crates/buzz-core/src/coding_session_command.rs`), and this fork's desktop
builder always writes `deliver` explicitly. An upgraded desktop against a relay
that predates the key therefore has **every** turn rejected — not degraded,
rejected — and the kind-9 fallback that would have hidden it does not exist
here by design (see the fork amendment above). Ship the relay first, then the
desktop. The reverse order is a total outage of turn sending for everyone on
that community.
