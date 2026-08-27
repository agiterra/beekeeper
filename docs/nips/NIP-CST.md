# NIP-CST: Coding-Session Transcript Facts

`kind:44225` is a provider-authored, immutable, provider-neutral transcript
fact for one exact coding-session generation. It lets Claude Code and future
runtime adapters feed the same Buzz coding workspace without exposing raw
provider protocol events.

This contract complements [NIP-CSC](NIP-CSC.md) and [NIP-CSL](NIP-CSL.md):
those contracts create and actuate sessions; NIP-CST renders their signed
shared transcript.

> **Fork amendments.**
>
> 1. **Native kinds only.** No kind-9 compatibility copy is published, ever.
>    One fact, one event, one kind.
> 2. **`reasoning` items are allowed and on by default.** The donor forbade
>    them. Here they are a first-class item kind, gated by
>    `BUZZ_CSP_INCLUDE_THOUGHTS` (default on). See below.

## Wire contract

Content has exactly this JSON shape:

```json
{
  "schema": "buzz-coding-session-transcript/v1",
  "session": {
    "driver": "claude-agent-acp",
    "instanceId": "provider-instance",
    "sessionId": "provider-session",
    "generation": 1
  },
  "eventSeq": 42,
  "timestamp": 1785512977000,
  "turnId": "provider-turn-id",
  "item": {
    "kind": "assistant_text",
    "text": "The tests pass."
  }
}
```

`eventSeq` is a positive safe integer, monotonic within the exact
`driver` / `instanceId` / `sessionId` / `generation` target. It is reserved and
persisted **before** publish, which is what makes a producer crash lose a
sequence number rather than reuse one: **gaps are permitted, duplicates are
not.** `timestamp` is finite epoch milliseconds. `turnId` is a bounded nonempty
string or `null`. `item` is an explicitly selected, deeply redacted Buzz
transcript item; raw provider events, credentials, environment, and unbounded
protocol objects are forbidden.

Tags are exactly these five two-field tags in this order:

1. `h`: channel UUID
2. `cst-v`: `cst1-1`
3. `cs-target`: the NIP-CSC structured exact-target key
4. `cst-seq`: decimal `eventSeq`
5. `cst-key`: the structured transcript semantic key for target + sequence:
   `coding-session-transcript/v1|<driver><instanceId><sessionId><generation><eventSeq>`,
   length-prefixed as in NIP-CSC

Content is bounded to 32 KiB and nested item depth to 24. Identifiers are
bounded to 512 UTF-8 bytes. Output that would exceed the content cap is elided
with a marker and a `sha256` of the full text, so a truncation is always
visible as a truncation.

## Fork amendment: reasoning items

The donor forbade reasoning/thought items outright. This fork allows them, as a
deliberate reversal:

```json
{ "item": { "kind": "reasoning", "text": "Checking whether the fixture is stale…" } }
```

The purpose of durably storing sessions here is analysis — "which tools are we
missing", "what was the chain of reasoning that produced this feature" — and a
transcript with the reasoning stripped cannot answer the second question at
all. Reasoning items are produced from the adapter's `thought` updates, take
the same redaction, depth, and size bounds as every other item kind, and are
controlled by `BUZZ_CSP_INCLUDE_THOUGHTS` on the producer (default on). A
deployment that does not want them recorded turns the flag off; nothing
downstream requires them to be present.

## Fork amendment: `user_prompt` correlation fields

A `user_prompt` item is additionally, optionally, attributed to the exact
command that opened its turn:

```json
{
  "item": {
    "kind": "user_prompt",
    "content": "Add a test for the empty case.",
    "steered": false,
    "operatorPubkey": "64-lowercase-hex-signer",
    "commandId": "the-thread.turn.start-commandId"
  }
}
```

`operatorPubkey`, `commandId` and `senderRole` are all additive and optional,
following the rule that governs every item field here: present only when the
provider actually witnessed the fact, omitted entirely rather than sent as
`null` when it did not, so an item published before any of them existed stays
valid forever.

`commandId` is the `commandId` of the [NIP-CSC](NIP-CSC.md) signed command that
started this turn — the same identifier the `turn_started`
[NIP-CSL](NIP-CSL.md) `kind:44224` receipt for that command carries, so a
reader can join a transcript line to the wire command and to its receipts
without a text match.

It is present whenever an operator command started the turn, and it names one
of **two** kinds:

- for an ordinary turn, the `kind:44220` `thread.turn.start` that started it;
- for the initial turn embedded in a `kind:44221` `session.create`'s
  `initialTurn`, the **create's own** `commandId` — that turn has no
  `thread.turn.start` of its own, so the create is the command that caused it.

A consumer therefore MUST resolve a `commandId` against both kinds, and MUST
NOT treat "no `kind:44220` carries this id" as evidence the item is malformed.
Equally, absence is **not** the marker of a create-embedded initial turn: the
only items that omit `commandId` are those published by a provider from before
this field existed, and any `user_prompt` no operator command started.
`commandId` follows the NIP-CSC identifier bounds: nonblank after trimming, at
most 256 UTF-8 bytes, no control characters.

The desktop's pending-turn row settles on this field when present; matching
prompt text remains a fallback for echoes an older provider produced with no
`commandId`, and a row that settled that way says so rather than presenting a
text match as equally strong evidence.

`senderRole` is the crew role of the identity that signed the command, when
the provider knows one:

```json
{
  "item": {
    "kind": "user_prompt",
    "content": "Add a test for the empty case.",
    "steered": false,
    "operatorPubkey": "64-lowercase-hex-signer",
    "commandId": "the-thread.turn.start-commandId",
    "senderRole": "lead"
  }
}
```

It is the [NIP-CSL](NIP-CSL.md#fork-amendment-actor-and-role-agent-seats)
`role` slug of the sibling seat whose `actor` signed this turn — same bounds
as there: 1–64 bytes, `[a-z0-9-]+`. It is present **only** when the signer is
a seated actor the provider can resolve to a role inside this umbrella; a turn
signed by the session founder, by a human collaborator, or by an agent with no
seat in this umbrella carries no `senderRole` at all. It is never inferred
from a display name, never carried for a signer the provider did not resolve,
and never sent as `null`.

`senderRole` describes the *signer*, not the delivery: a reader that wants to
know how the turn was delivered reads the [NIP-CSC](NIP-CSC.md) command's
`deliver` class and the `turn_degraded` receipt, not this field. And it is a
record of what the provider resolved at delivery time, not an authorisation:
authority is decided by the founder/grant chain in NIP-CSL, and a role slug on
a transcript item confers nothing.

The prompt text a runtime adapter receives for such a turn is prefixed with a
`[Context]` block naming that sender and role, while `content` here stays the
**original** text the sender signed — the framing is presentation for the
model, never a rewrite of the signed record.

## Authority and immutable reconciliation

The relay requires `messages:write`, an `h` channel scope, and **active channel
membership with no open-channel fallback** — visibility is not authority to
author the record of what a session did. Beyond that the relay applies a 32 KiB
size cap and stores the event. It does not parse the content: these are the
provider's own account of its own session, and relay-side validation would be
the relay asserting authority over facts it never observed.

Verification is the consumer's, at its trusted-ingress boundary: Nostr
signature, configured trusted provider signer, visible channel, exact ordered
tags, target, sequence, semantic key, and strict payload — all before the item
is displayed or suppressed from ordinary chat.

For one channel, signer, target, and sequence, divergent canonical payloads are
an immutable conflict: count them, render neither. Facts from different signers
never merge.

Transcript trust does not grant actuation. Sending a turn still requires an
independently signed NIP-CSC command from an authorized operator addressed to
the exact generation.

## Presentation

442xx kinds are never members of the channel timeline's content kinds, so
transcript items never appear as chat. The coding-session workspace consumes
them through the provider-neutral projection; the compact, immersive, and
pop-out surfaces all read the same record.

## Analysis

Storing these facts is the point: the relay's `events` table is the analysis
database, and no export step stands between a finished session and a query about
it. See [Analyzing recorded coding sessions](../coding-session-analysis.md) for
the `bee sessions` command surface and direct-SQL recipes (tool frequency,
error rate by tool, sessions per project, whole-transcript dump).

Two things a reader must get right: order by `cst-seq` **numerically** — it is a
decimal string on the wire, so a lexicographic sort puts item 10 before item 9 —
and carry explicit `kinds` on every `/query` filter, or the relay's p-gate
answers 403.

## Private continuity projection

The signed transcript is the attributable source corpus, not an instruction
stream for a later model. A provider that projects it into a private continuity
package MUST verify the full genesis/authority/lifecycle/provider chain first.
Before the package crosses into a new execution it MUST redact any credential,
opaque provider cursor, or absolute host path; repo-relative paths may remain.
Each redaction replaces the unsafe value in full with its byte count and SHA-256
digest, while the surrounding history item retains the signed source event id.
This lets a reader distinguish deliberate handoff elision from missing source
data without copying machine-private material into another model's context.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constant | `crates/buzz-core/src/kind.rs` |
| Payload structs | `crates/buzz-core/src/coding_session_payload.rs` |
| Membership, size cap | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builder | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
| Analysis CLI | `crates/buzz-cli/src/commands/sessions.rs` |
