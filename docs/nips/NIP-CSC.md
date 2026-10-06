# NIP-CSC: Coding-Session Commands

`kind:44220` is a durable, channel-scoped command from a Beekeeper operator to a
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

`driver` is an open capability-driven slug, not a Beekeeper enum. `commandId` and
all target identifiers are nonempty after trimming and at most 256 UTF-8 bytes.
Start-action text is nonempty after trimming and at most 12 KiB (12,288 UTF-8
bytes). `deliver` is an **optional** key on `thread.turn.start` — absent means
`"boundary"` — whose value is one of exactly `"boundary"`, `"steer"`, or
`"interrupt"`; any other value makes the command malformed, never a silent
default. `thread.turn.interrupt` carries no `deliver` key: the action is the
class. Senders on this fork **omit** `deliver` when it would be
`"boundary"` — the wire default — so a relay that predates the field still
accepts the ordinary turn path; `"steer"` and `"interrupt"` are written
explicitly and require a relay that validates the field. `generation` is a positive JavaScript-safe integer
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
`turn_injected`, `turn_degraded`, `turn_delivery_unknown`, `turn_dropped`,
`turn_refused`; and `interrupt_delivered` for a `thread.turn.interrupt`) and
the [NIP-CST](NIP-CST.md) `kind:44225`
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
  execution's runtime advertised native steering at initialize **and** the
  runtime's descriptor declares the `promptRequired` idle guard (below).
  Steering capability is a fact about the execution, not about the driver
  slug, and it is published per execution as `capabilities.threadSteer` in
  the `kind:44223` metadata. A successful injection is answered
  `turn_injected` (carrying the running turn's `turnId`), preceded by a
  `user_prompt{steered: true}` transcript item on that turn; nothing is
  cancelled, no second turn begins, and no new accounting is charged.
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
wrong. The same downgrade answers a native attempt that provably did not
inject — the running turn had already ended (`STEER_TURN_ENDED`), the runtime
answered the request with an explicit error (`STEER_REJECTED`), or the input
carried attachments the native path does not take
(`STEER_ATTACHMENTS_UNSUPPORTED`). One admission failure is **not** a
downgrade: when the execution's native-steer admission is full, nothing is
written and the command is answered `turn_dropped` / `STEER_SATURATED`,
terminal like `QUEUE_FULL` — the sender sends it again once it drains.

**The idle guard.** A `_session/steering` request that finds no running turn
is the dangerous case: an adapter with no guard starts a *detached* turn with
the input, one whose updates and completion the provider does not observe, so
the words are delivered and their output is lost. Whether an adapter honours
the request `_meta.steering.idleBehavior: "promptRequired"` — answering
`{outcome: "promptRequired"}` and leaving the content with the caller — is
not advertised on the wire. It is therefore a **declared runtime fact**:
`RuntimeDescriptor.steerIdleGuard` (`crates/beekeeper-core/src/coding_session_runtime.rs`),
set by the component that installs and pins the adapter, never inferred from
the driver slug. Native injection is offered only when the descriptor declares
`promptRequired` **and** the process advertised steering at `initialize`; a
runtime that declares no guard keeps every `steer` at the boundary.

**Delivery unknown.** A native attempt is one write into a running turn, and
its acknowledgement can be lost: the write fails part way, the prompt ends or
the runtime exits before the answer arrives, the bounded wait for it expires,
or the answer names no recognized outcome. The provider then publishes
`turn_delivery_unknown` with a `STEER_*` code (see the [NIP-CSL](NIP-CSL.md)
code table) and records the command as **refused** — a terminal answer, never
replayed automatically, because a replay of an input the runtime may already
hold is the double delivery the classes exist to prevent. The sender decides
whether to send again. An adapter that reports it started a separate,
unobserved turn with the input (`startedNewTurn`) is answered the same way
(`STEER_UNOBSERVED_NEW_TURN`): delivered, never resent, output unobserved. A
late acknowledgement may reconcile an unknown attempt under the same
`commandId`: `turn_injected` if it did land, `turn_dropped` /
`STEER_NOT_DELIVERED` if it provably did not.

**Implementation status, 2026-09-11: native injection is live for executions
whose runtime advertised steering at `initialize` AND whose descriptor declares
the `promptRequired` idle guard; every other runtime stays at the boundary.**
The 2026-08-26 status ("boundary-only; deferred until an adapter advertises
it") is superseded by the native steering plan
(`docs/NATIVE_STEERING_IMPL.md`). What was verified against the installed
adapters on 2026-09-11:

- `claude-agent-acp` 0.70.0 advertises `_meta.steering.supported` and honours
  `_meta.steering.idleBehavior: "promptRequired"` (`dist/acp-agent.js:1146-1150`
  answers `promptRequired` without touching the session; `:1160-1184` pushes a
  mid-turn input into the same SDK stream and answers `injected`). The desktop
  host declares `steerIdleGuard: "promptRequired"` for this runtime only
  (`desktop/src-tauri/src/session_provider/runtimes.rs`), so its executions
  publish `capabilities.threadSteer: true` and a `steer` is a native attempt.
- `codex-acp` 1.6.2 advertises steering but has **no idle guard**
  (`dist/index.js:31387-31400` starts a detached turn when no turn is
  steerable). Its descriptor declares no guard, so codex executions stay
  boundary-only: `threadSteer` is `false`, the composer never asks for
  `steer`, and a `steer` from any other client is degraded out loud.
- `goose acp` is not installed here; nothing is declared and it stays at the
  boundary.

`turn_degraded` remains reachable from any client for any execution that does
not qualify, exactly as before: the envelope validates every delivery class
(`crates/beekeeper-relay/src/handlers/ingest.rs`) and the provider answers with the
downgrade beside the `turn_queued`. The desktop composer selects `steer` only
when `isWorking && canSteer`
(`desktop/src/features/coding-sessions/ui/CodingSessionComposer.tsx`), and
`canSteer` is the execution's published `threadSteer`.

What this section does **not** claim: that every `steer` lands. The native
path has the unknown-delivery outcomes above, and the provider's evidence for
them is composition fixtures against scripted agents plus adapter validation
against the installed claude-agent-acp — see `plans/SESSION_STATE.md` for what
was run.

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
| Kind constant | `crates/beekeeper-core/src/kind.rs` |
| Payload + target key | `crates/beekeeper-core/src/coding_session_command.rs` |
| Desktop builder | `desktop/src/features/coding-sessions/lib/codingSessionCommand.ts` |
| Envelope validation | `crates/beekeeper-relay/src/handlers/ingest.rs` |
| Builder | `crates/beekeeper-sdk/src/builders.rs` |
| Semantic keys | `crates/beekeeper-sdk/src/coding_session.rs` |

## Deploying the `deliver` key

**Relay before desktop, for the escalated classes only.** The relay validates
`kind:44220` content with `deny_unknown_fields`
(`crates/beekeeper-relay/src/handlers/ingest.rs`,
`crates/beekeeper-core/src/coding_session_command.rs`), so any payload carrying a
key it does not know is rejected — not degraded, rejected — and the kind-9
fallback that would have hidden it does not exist here by design (see the fork
amendment above). Because the desktop builder omits `deliver` at its default,
boundary turns keep working against a relay that predates the key; `"steer"`
and `"interrupt"` do not, and their commands are refused until the relay
carrying the field is deployed. Ship the relay first, then the desktop; sending
an escalated class before that is a refusal, and writing the default out
explicitly would have been a total outage of turn sending for everyone on that
community.
