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
  "action": { "type": "thread.turn.start", "text": "operator instruction" }
}
```

`driver` is an open capability-driven slug, not a Buzz enum. `commandId` and
all target identifiers are nonempty after trimming and at most 256 UTF-8 bytes.
Start-action text is nonempty after trimming and at most 12 KiB (12,288 UTF-8
bytes). `generation` is a positive JavaScript-safe integer
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
| Envelope validation | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builder | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
