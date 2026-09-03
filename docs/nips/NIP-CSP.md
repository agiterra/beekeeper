# NIP-CSP — Coding-session policy

`draft` `optional` `client` `relay`

`kind:44245` is one signed record saying *how a mission is meant to be run*:
its posture, what it may spend, how much a person wants to be told, what a
lane owes before its work counts, who may be benched against whom, which acts
stay the founder's, and when to stop.

Before it, each of those was either a sentence in a brief no machine reads or
nothing at all. Budgets lived in a launch dialog, "red first" lived in
`AGENTS.md`, and "ask before you push" lived in a person's memory of having
said it once.

This kind is additive. Clients that do not implement NIP-CSP continue to run
coding sessions exactly as they do today and may ignore kind 44245. **Enforced: budget.turns at the provider's turn gate, gates.verifierRequired at the fold's completion check and at the relay's verdict-gated push, and gates.requiredGates at that push. Every other field is read and shown, never counted.**
Two fields, named, and nothing else counted — see "Validation boundary" and
"What v1 deliberately does not have" below, which together are the one thing a
surface rendering a 44245 must not get wrong. `gates.verifierRequired` joined
the enforced list on 2026-09-02 (batch 3, item G).

## Allocation

`44245` is the lowest unused and unreserved value available to this protocol at
the time of writing. The allocation scan checked both this fork and
`vanilla/main` registries and documentation:

- `44231` is reserved for session checkpoints and `44232` for native snapshots.
- `44233` and `44234` are proposed for git transitions and checks.
- `44235` through `44239` are explicitly reserved as coding-session headroom.
- `44240` through `44243` are used or reserved by Project Pulse extensions.
- `44244` is the team transaction (NIP-CSTX).
- `44245` had no code or documentation match in either tree before this NIP.

`crates/buzz-core/src/kind.rs` carries the allocation note and a compile-time
assertion that nothing sits between 44244 and 44245. This is a fork-local
allocation, not a claim of global Nostr registry ownership.

## Envelope

The event is a **regular** stored event. It has exactly four ordered,
two-field tags:

```json
[
  ["h", "<canonical channel UUID>"],
  ["d", "<canonical sessionRef UUID>"],
  ["csp-v", "buzz-coding-session-policy/v1"],
  ["csp-genesis", "<genesis event id, lowercase 64-hex>"]
]
```

`d` and `csp-genesis` MUST exactly equal the corresponding content fields, so a
policy cannot be filed under one umbrella while claiming another. Extra,
missing, repeated, reordered, or non-two-field tags are invalid. `h` and `d`
use lowercase canonical hyphenated UUIDs. The event kind MUST be 44245.

### Regular, not replaceable

The `d` tag addresses an umbrella so a consumer can fold the newest **accepted**
revision. It does **not** opt this kind into NIP-33 replacement, and a
compile-time assertion in `kind.rs` says so. A replaceable policy would let one
author's write erase the revision a decision was actually made under — and the
first question anyone asks about a mission that went wrong is *what was the
policy at the time*.

## Content

Content is public JSON. Exactly three keys are required:

```json
{
  "schema": "buzz-coding-session-policy/v1",
  "sessionRef": "<canonical UUID>",
  "genesisRef": "<genesis event id>"
}
```

Seven further keys are optional, in this wire order: `posture`, `budget`,
`attention`, `gates`, `bench`, `irreversible`, `stop`. An unset optional key is
**omitted**, never written as an explicit `null`.

### `posture`

Closed vocabulary: `spike`, `ship`, `investigate`, `overnight`. Consumed by the
router; it selects the risk tier a class is routed at.

### `budget`

```json
{
  "turns": 240,
  "tokensPerSeat": 4000000,
  "tokensPerSession": 40000000,
  "costUsdPerSession": 120.5,
  "contextTier": "long"
}
```

Every field is optional and omitted when unset. `turns` is a `u32` ≥ 1;
`tokensPerSeat` and `tokensPerSession` are `u64` ≥ 1; `costUsdPerSession` is a
finite number > 0; `contextTier` is exactly `standard` or `long`. A
`tokensPerSeat` above `tokensPerSession` is refused: a ceiling that cannot bind
is not a ceiling.

### `attention`

Closed vocabulary: `decisions`, `decisions-and-milestones`, `everything`.
Consumed by the **UI**, never by the router — attention changes what a person is
shown, never what the machine does.

### `gates`

```json
{
  "redFirst": true,
  "reviewEveryLane": true,
  "requiredGates": ["just ci", "just test"],
  "verifierRequired": false
}
```

`requiredGates` holds at most 32 unique names of at most 64 bytes each.

### `bench`

```json
{
  "identities": ["<lowercase 64-hex pubkey>"],
  "providers": ["claude-primary", "codex-primary"],
  "challengerSampleRate": 0.25
}
```

`identities` holds at most 64 unique lowercase 64-hex pubkeys; `providers` at
most 16 unique provider **aliases** of at most 256 bytes each;
`challengerSampleRate` is a finite number in `0.0..=1.0`.

`providers` names provider **aliases** (`claude-primary`), never instance ids
(`1958c6c448e05eed`). They are different names for different things, and
`crates/buzz-core/src/coding_session_identity.rs` makes confusing them a
compile error.

### `irreversible`

A non-empty, duplicate-free subset of `push`, `deploy`, `delete`,
`external-message`. Consumed by the fence: these are the acts that need the
founder's word. The list is closed in v1 for the same reason the fence is — an
act nobody named is an act nobody agreed to.

### `stop`

```json
{
  "timeBoxSecs": 28800,
  "onMilestone": "the lane lands and CI is green"
}
```

`timeBoxSecs` is a `u64` ≥ 1; `onMilestone` is 1 to 8,192 bytes with no control
characters.

## Bounds and decoding rules

- Complete content: at most 32,768 UTF-8 bytes, checked before parsing.
- Nostr references: lowercase 64-hex. UUIDs: lowercase canonical hyphenated.
- Prose carries no NUL and no control characters.
- Duplicate JSON keys are rejected at the top level and in every nested object.

Four rules a reader MUST implement, each with a reason a later reader will
otherwise relitigate:

1. **Unknown fields are rejected, not ignored.** A consumer that tolerated an
   unknown key would be claiming to enforce a policy it cannot read — the
   founder writes `noPushWithoutReview`, every gate accepts the record, and
   nothing enforces anything. The refusal names the offending key. The cost is
   that a v2 field cannot arrive inside a v1 record; that is the intended cost.
2. **Absent is not null.** An explicit `null` is refused, by name, at the top
   level and inside every nested object. A key-set check sees a null as
   *present* while serde decodes it to `None`, so without this rule the same
   signed bytes mean "unset" to one reader and "set to nothing" to another.
3. **A record that sets nothing is the withdrawal, and is valid.**
   `{schema, sessionRef, genesisRef}` decodes and `sets_any_policy()` answers
   `false`. Under a newest-wins fold that is the only way to *take a policy
   back*, so it is a legal record rather than an error. A consumer MUST render
   it as "no policy", never as "policy unknown": those are different facts and
   the first is a decision somebody made. The asymmetry that follows is
   deliberate — an **empty sub-object** (`"budget": {}`) and an **empty array**
   (`"gates": {"requiredGates": []}`) are both refused, because otherwise a
   record whose every collection was empty would pass the "at least one field"
   guard and answer `sets_any_policy() == true`: a record that set nothing
   claiming to set something, and a withdrawal any writer could silently
   impersonate. `sets_any_policy()` asks whether a **value** is set, never
   whether a key is present.
4. **A limit of zero is refused, not stored.** `turns`, `tokensPerSeat`,
   `tokensPerSession`, `costUsdPerSession` and `timeBoxSecs` all refuse zero.
   Zero and "no limit" would otherwise be the same record read two ways, and a
   budget a reader can misread as unlimited is worse than no budget at all.

## Validation boundary

The relay validates **structure**: schema, tags, closed vocabularies, bounds,
tag-to-content parity — `crates/buzz-relay/src/handlers/ingest.rs`. Whether the
signer held the standing to set policy is the **consuming fold's** question,
answered against the accepted NIP-CSAT chain, exactly the division kind 44244
draws. A relay that adjudicated policy authority would be asserting standing it
cannot verify at ingest time.

The v1 signer rule is: **the founder, or a seat holding an operator grant** the
relay had already accepted when the record was published. A `lead` role slug is
not enough and is not an input — a consumer can prove an operator grant from the
accepted NIP-CSAT chain and cannot prove a role slug it did not mint.

Every consumer applies that one rule through the same function,
`buzz_core::coding_session_policy::fold_coding_session_policies` with
`signer_may_steer_at`: the provider's context projection and turn gate
(`crates/buzz-session-provider/src/context_projector.rs`,
`select_session_policy`), and `bee sessions policy set|get|clear`
(`crates/buzz-cli/src/commands/sessions/policy.rs`). The grant is evaluated **at
the record's own `created_at`**, so a later revoke does not retroactively
invalidate a policy signed while the grant stood, and a later grant does not
retroactively bless one signed before it existed.

A record that fails the check is **listed with its reason**, never silently
dropped and never promoted: a stranger's competing ceiling and "nobody set a
policy" are different facts, and a reader that could not tell them apart would
be the defect this rule exists to prevent.

## What v1 deliberately does not have

- **No general enforcement.** Exactly one field binds anything:
  **`budget.turns`**, which overrides the provider's `BUZZ_CSP_TURN_BUDGET`
  ceiling for that umbrella and, when it is what refuses a turn, says
  "published session policy" in the `BUDGET_EXHAUSTED` message so a reader can
  tell which ceiling bound
  (`crates/buzz-session-provider/src/commands.rs`, `exhausted_umbrella_budget`).
  A policy published mid-session does not bind until that umbrella's next
  create or resume.

- **`gates.verifierRequired` is enforced** at the 44244 fold's completion check
  (`crates/buzz-core/src/coding_session_completion_verification.rs`): with it
  set, a `mission.completed` whose settled assignments carry no active
  verifier's ruling is excluded `CompletionNotVerified`, and
  `bee sessions complete` refuses to sign one. A surface that has not read the
  policy set folds with the flag `false`, which means *this fold enforces
  nothing extra* — never *no verifier is required*.

  **Every other field is read and shown, never counted** — `posture`,
  `budget.tokensPerSeat`, `budget.tokensPerSession`, `budget.costUsdPerSession`,
  `budget.contextTier`, `attention`, `gates.redFirst`, `gates.reviewEveryLane`,
  `gates.requiredGates`, all three `bench.*`, `irreversible`, and both
  `stop.*`. For those, **a published policy is a stated intention, not an
  enforced limit** — and any surface that displays one MUST say so rather than
  showing a budget bar nothing is counting.
- **No UI.** The launch form is later work. `bee sessions policy set|get|clear`
  writes and reads the record (batch 2 lane B2). `get` folds the signer rule
  above: it prints the newest record whose signer held standing, `null` when
  none did, and every refused record under `excluded` with its author and a
  reason. A withdrawal prints as a real record with `setsAnyPolicy: false` —
  "nobody set one", "somebody took it back", and "a stranger published one" are
  three different facts and the output keeps them apart.

## Implementation

| Layer | Path |
|---|---|
| Types, decoder, validator, envelope | `crates/buzz-core/src/coding_session_policy.rs` |
| Kind allocation and assertions | `crates/buzz-core/src/kind.rs` |
| Signed builder | `crates/buzz-sdk/src/coding_session_policy.rs` |
| Relay structural validation | `crates/buzz-relay/src/handlers/ingest.rs` |
| Writer and reader (`bee sessions policy set\|get\|clear`) | `crates/buzz-cli/src/commands/sessions/policy.rs` |
| Newest-accepted fold, in the context package | `crates/buzz-session-provider/src/context_projector.rs` |
| `budget.turns`, enforced at the turn gate | `crates/buzz-session-provider/src/commands.rs` |
| `gates.verifierRequired`, enforced at the fold's completion check | `crates/buzz-core/src/coding_session_completion_verification.rs` |
| Field-by-field consumer map | `docs/design/portable-team-loop/POLICY.md` |

`docs/design/portable-team-loop/POLICY.md` is the companion design note: it
names each field's future consumer so the record and the thing that reads it
cannot drift apart before the reader is written.
