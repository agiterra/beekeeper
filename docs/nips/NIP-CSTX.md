# NIP-CSTX — Coding-session team transactions

`draft` `optional` `client` `relay`

`kind:44244` is one signed, append-only semantic transaction inside a coding
session. It lets every client distinguish assignments, reports, verdicts,
acknowledgements, and terminal mission dispositions without interpreting chat
prose or provider transcripts.

The event signature is the author. Content MUST NOT carry `author`,
`authorPubkey`, `actor`, or another field that restates authorship. An
assignment's `assigneeActor` is the target of the assignment, not its author.

This kind is additive. Clients that do not implement NIP-CSTX continue to use
the existing conversation and coding-session events and may ignore kind 44244.

## Allocation

`44244` is the lowest unused and unreserved value available to this protocol at
the program base (`0f37c1d4`). The allocation scan checked both this fork and
`vanilla/main` registries and documentation:

- `44231` is reserved for session checkpoints and `44232` for native snapshots.
- `44233` and `44234` are proposed for git transitions and checks.
- `44235` through `44239` are explicitly reserved as coding-session headroom.
- `44240` through `44243` are used or reserved by Project Pulse extensions.
- `44244` had no code or documentation match in either tree before this NIP.

This is a fork-local allocation, not a claim of global Nostr registry
ownership.

## Envelope

The event is a regular stored event. It has exactly five ordered, two-field
tags:

```json
[
  ["h", "<canonical channel UUID>"],
  ["d", "<canonical sessionRef UUID>"],
  ["cstx-v", "buzz-coding-session-team-transaction/v1"],
  ["cstx-genesis", "<genesis event id, lowercase 64-hex>"],
  ["cstx-type", "<closed operation token>"]
]
```

`d`, `cstx-genesis`, and `cstx-type` MUST exactly equal the corresponding
content fields. Extra, missing, repeated, reordered, or non-two-field tags are
invalid. `h` and `d` use lowercase canonical hyphenated UUIDs. The event kind
MUST be 44244.

## Common content

Content is public JSON with exactly seven top-level keys:

```json
{
  "schema": "buzz-coding-session-team-transaction/v1",
  "sessionRef": "<canonical UUID>",
  "genesisRef": "<genesis event id>",
  "type": "assignment",
  "supersedes": null,
  "deliveryCommandId": "wake-builder-1",
  "body": {}
}
```

`supersedes` is a required nullable key. It means **same-author correction of
the same operation**, not workflow causality. A correction MUST name another
kind 44244 event with the same signed author, channel, session, genesis, and
operation type. A relay or fold with stored-event access enforces those rules
and rejects a dangling or cross-session correction. Operation bodies carry the
explicit causal references described below.

`deliveryCommandId` is also a required nullable key. It may correlate **any**
operation with the kind 44220 turn that woke or delivered it; it is not limited
to assignment. A non-null value obeys the exact 44220 `commandId` grammar:
non-blank UTF-8, at most 256 bytes, with no control characters. It is
correlation, not evidence of provider delivery; receipts remain authoritative.

The closed v1 operation tokens are:

```text
assignment
report
verdict
acknowledgement
mission.completed
mission.blocked
```

Unknown operation tokens and a `type` whose body has another operation's shape
are invalid.

## Operation bodies

Nullable fields shown below are required keys whose value is either the named
type or JSON `null`. Collections are required even when empty unless stated
otherwise.

### `assignment`

```json
{
  "assigneeActor": "<assignee pubkey, lowercase 64-hex>",
  "assigneeRole": "builder",
  "objective": "Implement the signed transaction protocol",
  "brief": "<complete bounded brief>",
  "branch": "team-transactions",
  "baseSha": "<40- or 64-hex git object id>",
  "fileOwnership": ["crates/buzz-core/src/coding_session_team_transaction.rs"],
  "acceptanceSteps": ["cargo test -p buzz-core"]
}
```

`branch` and `baseSha` may be null. `assigneeRole` exactly matches the seat
grammar: `[a-z0-9-]+`, 1 through 64 bytes. The typed assignment remains
`Authored, delivery unconfirmed` until the existing command/receipt path proves
delivery.

### Provider-owned durable wake

A provider MAY maintain a durable wake intent for an operation or for a
provider-signed terminal turn that still owes a required report. This is not a
second authority system:

- the provider signer MUST be the founder or hold an active receipt-backed
  `grant-operator` on the exact genesis;
- the founder-held create/hire client SHOULD ensure that provider grant only
  after the expected provider's verified create receipt (and signed metadata
  where the create flow waits for metadata). It MUST extend the canonical
  accepted chain, confirm the relay-signed acceptance receipt, and treat an
  already-active operator grant as an idempotent success before granting the
  seat actor. The provider MUST NOT self-grant and no client-side projection
  bypasses relay admission;
- a report intent belongs only to the provider that owns the reporting actor's
  exact active receipt-backed generation. Other providers seeing the same
  channel record MUST remain inert. On startup that owning provider MUST
  exhaust the stored kind-44244 partition before relying on the live replay
  window, so an offline interval cannot erase the durable push obligation;
- the destination MUST be the one active accepted `lead` seat, joined to an
  exact provider target through its authorized create, provider-signed create
  receipt, and provider-signed metadata; zero, multiple, or unverifiable lead
  targets remain pending and emit nothing;
- a report wake carries only `{operationId,type}` and uses a new deterministic
  command id derived from the report event and exact lead target. It MUST NOT
  reuse the report's `deliveryCommandId`, which may name an earlier command or
  another target;
- a terminal diagnostic is allowed only when the completed turn's exact
  `commandId` is a verified `{operationId,type:"assignment"}` pointer to an
  active canonical assignment whose `deliveryCommandId`, assignee actor, and
  assignee role all match that turn. Ordinary hire/READY and prose turns never
  imply a missing report;
- a canonical signed report by that actor, referencing that exact assignment,
  inside the assignment turn suppresses the diagnostic. A parallel report for
  another assignment, or an unauthorized, excluded, or malformed record, does
  not suppress it;
- relay acceptance alone does not retire the intent. A verified target-provider
  `turn_queued`, `turn_started`, `turn_refused`, `turn_dropped`, or exact prompt
  echo does. A crash replays the same signed attempt; once its freshness horizon
  expires, a fresh target-bound command id and signature are required.
  `turn_degraded` and `interrupt_delivered` are not settlement: neither proves
  that this boundary prompt was queued, started, refused, or dropped.

The terminal diagnostic pointer is bounded public JSON:

```json
{
  "schema": "buzz-team-wake/v1",
  "type": "turn_ended_without_required_operation",
  "terminalEventId": "<provider-signed 44225 event id>",
  "seatRole": "builder",
  "causedByCommandId": "<exact assignment turn command id>"
}
```

### `report`

```json
{
  "assignmentRef": "<assignment event id>",
  "summary": "Implemented and verified",
  "branch": "team-transactions",
  "baseSha": "<git object id or null>",
  "headSha": "<git object id or null>",
  "files": ["crates/buzz-core/src/coding_session_team_transaction.rs"],
  "tests": [
    {
      "name": "buzz-core",
      "command": "cargo test -p buzz-core",
      "outcome": "passed",
      "evidence": "42 passed; exit 0"
    }
  ],
  "redBeforeGreen": true,
  "deviations": [],
  "residuals": [],
  "anomalies": []
}
```

`branch`, `baseSha`, `headSha`, and `redBeforeGreen` may be null. Test outcome
is exactly `passed`, `failed`, or `not-run`; `evidence` may be null. Only this
valid signed `tests` collection is structured test evidence. A prose message
that says tests passed does not populate it.

### `verdict`

Verdicts have two closed subtypes with non-overlapping decision vocabularies.
A verifier cannot sign an approval and a lead cannot relabel a disposition as
verification evidence.

#### `refutation`

```json
{
  "subtype": "refutation",
  "assignmentRef": "<assignment event id>",
  "reportRef": "<report event id>",
  "decision": "not-refuted",
  "summary": "No refutation found",
  "findings": [],
  "requiredAction": null
}
```

The closed refutation decision vocabulary is `confirmed`, `not-refuted`, and
`blocked`.

#### `disposition`

```json
{
  "subtype": "disposition",
  "assignmentRef": "<assignment event id>",
  "reportRef": "<report event id>",
  "refutationRef": "<prior refutation event id or null>",
  "decision": "approve",
  "summary": "Accepted",
  "findings": [],
  "requiredAction": null
}
```

The closed disposition decision vocabulary is `approve`,
`approve-with-notes`, `changes-requested`, `reject`, and `blocked`.
`refutationRef` may be null; when non-null it MUST name a `refutation` for the
same assignment/report pair. `requiredAction` may be null. `assignmentRef` and
`reportRef` MUST differ for both subtypes.

### `acknowledgement`

```json
{
  "acknowledgedEventRef": "<event id>",
  "status": "received",
  "note": null
}
```

`received` is the only v1 status. For a governing disposition the signer MUST
be the actor assigned by its `assignmentRef`; receipt MUST NOT be inferred from
provider queue/start receipts. `note` may be null.

### `mission.completed`

```json
{
  "assignmentRefs": ["<assignment event id>"],
  "landedShas": ["<40- or 64-hex git object id>"],
  "summary": "Mission landed and closed",
  "followUps": []
}
```

`assignmentRefs` is non-empty and duplicate-free. `landedShas` may be empty for
non-code missions. A relay/fold validates that each referenced assignment has a
report explicitly governed by disposition `approve` or `approve-with-notes`,
followed by the assigned actor's acknowledgement of that exact disposition. No
other verdict decision means approval. This single-event decoder cannot prove
graph existence or authority.

### `mission.blocked`

```json
{
  "assignmentRefs": [],
  "summary": "Signing is held",
  "blockers": ["Keychain is locked"],
  "heldOn": "founder",
  "requiredAction": "Unlock the signing keychain"
}
```

`assignmentRefs` may be empty when a mission blocks before dispatch. `blockers`
is non-empty. `heldOn` may be null; `requiredAction` may not.

## Bounds and reference grammar

- Complete content: at most 131,072 UTF-8 bytes.
- Long brief: at most 32,768 bytes.
- Other prose item: at most 8,192 bytes; test names at most 512 bytes.
- File path: at most 1,024 bytes; branch: at most 255 bytes.
- General collections: at most 256 entries; report tests: at most 128.
- Nostr references and assignee pubkeys: lowercase 64-hex.
- Git object ids: lowercase 40- or 64-hex.
- UUIDs: lowercase canonical hyphenated form.
- `deliveryCommandId`: non-blank UTF-8, at most 256 bytes, no control
  characters; it is not a UUID field.
- Required prose is non-blank and no prose/path may contain NUL.
- File, assignment-reference, and landed-SHA collections are duplicate-free.
- An event cannot causally reference or supersede its own id.

Decoding rejects missing or unsupported keys, duplicate JSON keys, malformed
references, out-of-bounds values, unknown enum values, and tag/content
disagreement.

## Authority matrix

The fold receives a verified authority/session context rather than deriving
roles or grants from transaction prose. The exact v1 matrix is:

| Operation | Authorized signer |
|---|---|
| `assignment` | founder, active `lead`, or active operator grant with `may_steer` |
| `report` | the assignment's `assigneeActor` |
| `verdict/refutation` | active `verifier` |
| `verdict/disposition` | founder, active `lead`, or active operator grant with `may_steer` |
| `acknowledgement` | the assigned actor of the acknowledged governing disposition |
| `mission.completed`, `mission.blocked` | founder, active `lead`, or active operator grant with `may_steer` |

An operator grant qualifies only when the supplied active accepted NIP-CSAT
grant includes `may_steer`; viewer or otherwise non-steering grants do not
qualify. Role seats must be active in the supplied accepted NIP-CSAT
`grant-seat`/`revoke-seat` projection. Kind 44221/44223 lifecycle metadata is
not authority and an absent seat grant is unauthorized, never inferred. The
event signature is always the signer; the body never claims authority.

## Validation boundary and deterministic fold

The core single-event validator proves structural validity only. The canonical
fold, with a supplied verified authority/session context, additionally applies:

1. Every event has a valid id/signature and matches the supplied channel,
   session, and genesis exactly.
2. Every causal reference exists in the supplied set, is kind 44244, has the
   required type, and remains in that context. Reports name assignments;
   verdicts name a report for that same assignment; disposition refutations
   name a refutation for that exact pair; acknowledgements name dispositions;
   terminal assignment references name assignments.
3. The combined causal/correction graph is acyclic.
4. `supersedes`, when non-null, names an existing event with the same signed
   author, channel, session, genesis, operation type, verdict subtype, and
   logical subject. It is never a workflow edge. Logical subjects are:
   assignment actor+role; report assignment; verdict subtype+assignment+report;
   acknowledgement disposition; or the session mission for a terminal type.
5. Authority is closed over the canonical causal graph, not evaluated as an
   isolated signer claim. A report requires its active authorized assignment;
   a refutation or disposition requires its exact active authorized assignment
   and report; a disposition naming a refutation requires that exact active
   authorized refutation; an acknowledgement requires its active authorized
   disposition and that disposition's active assignment/assignee. Terminal
   events require every named assignment to be active. A self-appointed
   unauthorized assignment therefore grants no authority to its report or any
   descendant.
6. Unauthorized events remain append-only history but are explicitly excluded.
   Any causal or correction child of an unauthorized, superseded, losing-fork,
   or otherwise excluded parent is also excluded with a stable dependency code.
   Projection runs in causal operation order, so a child can never make an
   invalid parent active. A later correction whose correction parent is invalid
   is excluded rather than skipping over that broken correction link.
7. Each eligible correction component chooses among its unsuperseded heads by ascending
   `(created_at, event id)`, with the greatest tuple winning. Superseded events,
   losing fork heads, the winner, and every contender are disclosed. Separate
   reports that do not use `supersedes` remain parallel facts; report recency
   alone never erases another report.
8. An assignment settles only when one active report is explicitly governed by
   an active approving disposition and the assigned actor acknowledges that
   exact disposition. If several complete governance chains exist, the
   disposition with greatest `(created_at, event id)` governs, and the conflict
   is disclosed. When the assigned actor signs several acknowledgements of the
   same disposition, the greatest tuple supplies the projected acknowledgement
   id and that conflict is also disclosed. Other reports and acknowledgements
   remain facts.
9. `mission.completed` is ineligible unless every named active assignment is
   settled under rule 8. `mission.blocked` needs no inferred predecessor.
10. The canonical terminal is the eligible authorized `mission.completed` or
   `mission.blocked` with greatest `(created_at, event id)`. All competing
   terminal facts and the winner are disclosed. No terminal is ever inferred
   from silence, timeout, provider state, or absence of later events.

The semantic graph normally progresses:

```text
assignment -> report -> refutation? -> disposition -> acknowledgement -> mission.completed
```

`mission.blocked` may settle a mission from any stage. A refutation is evidence,
not approval; only the disposition governs. Corrections replace a record only
in the semantic projection; all signed events remain append-only history.

## Shared conformance vectors

`conformance/coding-session-team-transaction/fixtures/schema-vectors.json`
contains transport-neutral valid and invalid payload vectors. Core runs the
fixture through the strict decoder; SDK, desktop, mobile, and relay adapters
SHOULD run the same file rather than transcribing its examples.

## Why observation is its own kind (kind 44246, NIP-CSOB)

A seat's checkpoint reports, gate rows, findings dispositions and phase timings
are **not** operations on this kind, and deliberately so. Three properties of
the fold above decided it. Its operation vocabulary is a closed enum with no
`other` arm and `fold_coding_session_team_transactions` returns `Err` for the
**whole set** when an envelope fails to validate — not one of the two
record-class hard errors, but one of the three further whole-set failures
described above — so a build predating a new token would read a session carrying
one gate row as a broken mission, on the stream a team writes most often. Its
projections are bounded for a handful of assignments, and hundreds of
observations would evict the records mission state depends on. And an
observation carries no authority, no supersession and no causal reference, so
this kind's correction validator and twelve exclusion codes buy it nothing while
letting its defects become governance disclosures. Kind 44246 therefore has no
exclusion codes at all and a fold that cannot fail. See `docs/nips/NIP-CSOB.md`.
