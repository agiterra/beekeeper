# Project Pulse digest fold — v2 conformance contract

This directory is the byte-exact source of truth for the Project Pulse fold.
The Rust fold in `buzz-cli`, the TypeScript fold in Desktop, and a relay-side
kind 39011 projection must bind to the same vectors. A rule implemented in only
one fold is a defect.

The fold reports facts. Rendering and the `wait | consult | proceed` decision
consume those facts but are not part of this contract.

## Inputs and scope

For one normalized kind-30621 project coordinate, a client-composed fold takes:

- kind 44240 Pulse entries selected by their `a` tag;
- project-channel facts of kinds 24223 (lease), 44221 (lifecycle command),
  44223 (metadata), 44224 (lifecycle receipt), 44227 (goal), 44229 (name), and
  44230 (closure);
- `now`, read once after the last source query returns; and
- `sourceErrors`, one `{scope, message}` per failed or truncated query.

Session reach is exactly `sessionsScope: "project channels"`. A community-wide
session scan is forbidden and an `a` filter cannot discover session facts.
Therefore an empty reachable set is not proof that no work exists outside that
scope.

## Envelope and member shapes

Keys occur in the following order. Every nullable member is emitted as `null`,
never omitted.

```json
{
  "schema": "buzz-project-pulse-digest/v2",
  "source": "client-composed",
  "project": "30621:<owner>:<dtag>",
  "asOf": 1785600000,
  "complete": true,
  "sessionsScope": "project channels",
  "sessions": [],
  "providerReachableSessions": [],
  "openUnverifiedSessions": [],
  "closedSessions": [],
  "entries": [],
  "errors": []
}
```

`source` is `client-composed` in this corpus. A relay projection substitutes
`relay-digest`; it does not change the remaining shape. Client-composed output
never invents relay receipt time: `leaseAcceptedAt` is always null.

```text
sessions[] = {
  sessionKey, sessionRef|null, name|null, goal|null,
  lifecycle, coordinationState,
  latestObservationAt|null, observedAgeSeconds|null,
  generations[], sourceEventIds[]
}

generations[] = {
  targetKey, executionKey, providerAuthorityPubkey,
  current, reachability,
  status|null, statusAt|null, branch|null,
  observedCommit|null, dirty|null, relayReachable|null, verifiedAt|null,
  commitConfirmation,
  leaseState|null, leaseIssuedAt|null, leaseAcceptedAt|null,
  leaseExpiresAt|null, leaseSigner|null, leaseSourceEventId|null,
  leaseSequence|null,
  lifecycleCommandEventId, lifecycleReceiptEventId,
  sourceEventIds[]
}

entries[] = {
  eventId, pubkey, createdAt, type, text, claimedAreas[],
  branch|null, sessionRef|null, supersedes|null,
  supersededBy[], active
}

supersededBy[] = { eventId, pubkey|null, honored, reason|null }
errors[]       = { scope, message }
```

The three outer index arrays contain `sessionKey` strings in the same order as
their rows in `sessions[]`; they do not duplicate session objects.

## Lifecycle authority and generation identity

A session generation exists in the digest only when exactly one immutable
kind-44221 lifecycle command and exactly one kind-44224 receipt share a
`commandId`, the receipt status successfully mints a generation, and the
receipt signer equals the command action's `providerAuthorityPubkey`.
Metadata authorship never establishes or changes authority.

Proof uniqueness is also exact-target scoped. After identical command and
receipt event IDs are deduplicated, exactly one successful proof pair may mint
`(h,targetKey)`. Two distinct successful pairs for that same generation make
the generation inadmissible, independent of input order; the fold never picks
the first, newest, or lexicographically smallest authority proof.

Every authority join is channel-bound. Command identity is `(h, commandId)`
and generation identity for joins is `(h, targetKey)`. A command and receipt
must carry the same exact nonempty `h`; metadata and lease evidence must carry
the accepted generation's `h`. Same-named facts from another queried channel
are unrelated and cannot mint or modify a generation. Creates mint generation
1 only; a successful create receipt naming any other generation is rejected.

The receipt envelope is the SDK's exact, ordered four-tag sequence:

```text
h=<nonempty channel UUID>
cslr-v=cslr1-1
csl-command=<content.commandId>
csl-key=coding-session-lifecycle-receipt/v1|<byte-length>:<commandId>
```

There is no `cs-target` receipt tag. The target is strictly decoded from the
receipt content. Lifecycle commands likewise require ordered `h`, `csl-v`,
`csl-command` tags, and leases require ordered `h`, `cslease-v`, `cs-target`,
`csl-command`, `cslease-seq` tags. Reordering any of these envelopes rejects
that fact rather than broadening the DB authority proof during a client fold.
Lifecycle command, receipt, and lease content also use their protocol-defined
closed field sets. Missing or unknown fields and duplicate JSON object keys at
any nesting depth are rejected before authority or reachability is folded.

A successful create participates only when its content `projectRef`, normalized
with `normalize_project_coordinate`, equals the requested project. Its verified
`sessionRef` becomes `sessionKey`; without one, the fallback is
`implicit:<executionKey>`.

A resume is accepted only when:

- its command targets an already accepted exact generation;
- receipt target driver, instance id, and session id equal the predecessor;
- receipt generation is exactly predecessor generation + 1; and
- the command/receipt authority proof above succeeds.

The new generation inherits the predecessor's umbrella `sessionRef`. Multiple
executions deliberately created with the same verified `sessionRef`, and all
accepted resumes, collapse into one outer session row.

`targetKey` is `coding-session/v1|` plus UTF-8 byte-length-prefixed driver,
instance id, session id, and generation. `executionKey` uses
`coding-execution/v1|` and the same first three fields, omitting generation.
Within each execution, only the greatest accepted generation is `current:true`.

## Durable observations and liveness evidence

For each accepted exact generation, retain the newest authority-signed kind
44223 metadata by `(created_at, event id)`. It participates only when its
content-derived target matches the accepted generation, its normalized
`projectRef` matches the project, and its signer equals the lifecycle-bound
provider authority. A `sessionRef` claimed by metadata does not establish
authority or umbrella grouping.

`statusAt` is the metadata event's `created_at`. `latestObservationAt` is the
greatest non-null `statusAt` across the umbrella, and
`observedAgeSeconds = now - latestObservationAt`. Observation fields
`observedCommit`, `dirty`, `relayReachable`, and `verifiedAt` preserve null.

`commitConfirmation` is fixed:

| `relayReachable` | value |
|---|---|
| `true` | `Commit confirmed on relay` |
| `false` | `Commit not found on relay` |
| `null` | `Commit not checked` |

A kind-24223 lease is eligible only when its content target and `cs-target` tag
name an accepted exact generation, `csl-command` names that generation's
accepted create/resume command, `cslease-seq` equals content `leaseSequence`,
and its signer equals that command's `providerAuthorityPubkey`.

Per exact generation:

1. Retain eligible leases at the greatest `leaseSequence`.
2. Duplicate rows with the same event id collapse.
3. Two different event ids at the same greatest sequence are conflicting
   evidence: `leaseSequence` preserves that sequence, the selected lease fields
   are null, and both ids remain in `sourceEventIds`.
4. A unique `released` winner is preserved but does not prove reachability.
5. A unique `live` winner expires conservatively at
   `event.created_at + 150`. This conservative signed-time lifetime subtracts
   30 seconds from Redis's authoritative 180-second accepted-time TTL so a cold
   client cannot extend reachability beyond the snapshot it cannot observe.
   `now < leaseExpiresAt` is live; exact equality is expired.
   `leaseAcceptedAt` remains null in a client-composed digest. A future
   relay-composed digest may use validated `acceptedAt + 180` instead.

Generation `reachability` is:

- `terminal` when newest metadata says `stopped` or `disconnected`, regardless
  of lease evidence;
- `provider_reachable` only for a `current:true` generation with a unique,
  unexpired, authority-valid `live` lease; otherwise
- `unverified`.

A valid lease on an old generation never transfers to its resumed successor
and cannot keep the old generation provider-reachable once it is non-current.
Other event streams may be positive activity evidence to a product, but never
extend or manufacture a lease in this fold.

Lifecycle commands, receipts, metadata, closures, and leases use the strict
buzz-core decoders: exact accepted field sets, duplicate-key rejection at every
depth, bounded nonempty identifiers, exact enums and null coupling, and
receipt status/session/error consistency. A malformed signed fact is not a
partial source read and cannot mint, observe, close, or make a generation live.

## Umbrella lifecycle and coordination state

The newest kind-44230 by `(created_at, event id)` joined through verified
`sessionRef` determines umbrella `lifecycle`: content action `closed` yields
`closed`; absence or `open` yields `open`.

Goal, name, and closure joins are `(h, sessionRef)` joins. When one verified
umbrella `sessionRef` has accepted generations in more than one channel, those
umbrella facts fail closed rather than selecting one channel or allowing a
fact from either channel to govern the other.

`coordinationState` and the outer indexes are tri-state:

- `closed`: umbrella lifecycle is closed; closure outranks every lease;
- `provider_reachable`: lifecycle is open and at least one current generation
  is `provider_reachable`;
- `open_unverified`: lifecycle is open without such evidence.

An unclosed session without a valid lease is not dead and is not ordinary
history. No age threshold changes this state. The v1 30-minute freshness law
is removed: durable observation recency and ephemeral reachability are
independent axes.

## Pulse entries and supersession

A kind-44240 event participates when it belongs to this project and passes the
strict Pulse entry envelope validator. Invalid in-scope entries are excluded
and add `invalid-entry` detail to `errors[]` without making the read partial.
Foreign-project events are ignored without error. `claimedAreas` remain author
claims. `pu-session` is echoed but is not verified attribution.

Supersession is single-pass marking, never traversal. Entry `E` becomes
`active:false` iff an entry `S` in the same result set satisfies all of:

- `S.supersedes == E.id`;
- `S.pubkey == E.pubkey`;
- `S` is newer by `(created_at, event id)`, with greater id winning a timestamp
  tie; and
- `S.id != E.id`.

Cross-author claims never retire the target and use reason `cross-author`.
Missing targets use `unresolved` and add an `unresolved-supersedes` error.
Claims losing the total order use `out-of-order`. Superseded entries remain in
the digest with `active:false`.

Entry-to-session placement is a consumer authorization law. Because
`pu-session` is author-controlled, a surface may nest an entry only after
resolving founder/grant authority from the sanctioned session authority chain.
The v2 digest does not encode that result, so the corpus cannot assert it.

## Ordering

- `entries[]`: `createdAt` descending, greater event id first on ties.
- `sessions[]`: `latestObservationAt` descending, null last, then
  `sessionKey` byte-ascending.
- `generations[]`: current first, then `executionKey` byte-ascending, then
  `targetKey` byte-ascending.
- the three session index arrays follow `sessions[]` order.
- `supersededBy[]` and every `sourceEventIds[]`: event id ascending.
- `errors[]`: scope ascending, then message ascending.
- `claimedAreas[]`: author order after decoder normalization.

Branch filters are exact and case-sensitive. A null branch stays distinct;
`--branch -` selects null and omitting the filter selects all. Filtering must
recompute the outer session index arrays so they never name hidden rows.

## Completeness

`complete` is false exactly when `sourceErrors` is nonempty. Those errors are
returned even with partial entries or sessions. A confirmed-empty project is
`complete:true`; a failed read must never masquerade as one. Fold observations
such as invalid entries or unresolved supersession do not by themselves make a
read incomplete.

## Vector format and coverage

`fixtures/fold-vectors.json` has schema
`buzz-project-pulse-fold-vectors/v2`, digest schema
`buzz-project-pulse-digest/v2`, entry schema `buzz-pulse-entry/v1`, and an array
of `{name, description, input, expected}`. Inputs use signature-stripped Nostr
rows (`id`, `pubkey`, `created_at`, `kind`, `tags`, `content`). Synthetic ids
are lowercase 64-hex ordering fixtures, not hashes of the event body.

The corpus preserves every v1 entry/supersession case and adds:

- `idle-hours-old-with-live-authorized-lease`: old idle metadata plus a live
  authority-bound lease is provider-reachable using the real receipt tags;
- `unverified-lease-outcomes`: no lease, release, exact-boundary expiry,
  equal-sequence conflict, wrong signer, and terminal precedence collapse into
  one open/unverified umbrella while preserving evidence;
- `resume-generation-isolation-and-continuity`: a +1 resume stays in one
  umbrella, predecessor reachability does not transfer, and generation +2 is
  rejected; and
- `closure-outranks-live-generation`: durable closure wins over live generation
  evidence and indexes the umbrella as closed;
- `cross-channel-authority-proof-splicing-rejected`: session facts cannot join
  across channel `h` values;
- `create-receipt-generation-two-rejected`: create can mint generation 1 only;
  and
- `strict-lifecycle-and-lease-json-rejected`: closed field sets and duplicate
  JSON-key rejection plus lifecycle/receipt/metadata/closure value validation
  stay byte-parity behavior across folds.
- `competing-generation-proofs-rejected-forward` and `-reverse`: two distinct
  successful proofs for one exact target reject the generation in both orders.

`implementation.test.mjs` binds the production TypeScript fold. The Rust test
in `buzz-cli` loads the same JSON with `include_str!` and compares both decoded
objects and serialized field order. A relay-side fold must bind the same
vectors with only the documented `source` substitution.
