# NIP-CSAT — Coding-session authority transitions

`draft` `optional` `relay`

`kind:44228` is one append-only link in one coding session's **authority
chain** — the sequence of decisions about who may steer, read, or hold a
signed team role seat in a session
after its genesis (`kind:44226`, [NIP-CSG](NIP-CSG.md)) founded it. The chain
is rooted at the genesis event **id** (never a `sessionRef` label, for the
same resolution-by-id reason genesis itself pins), and every accepted link is
confirmed by a relay-signed acceptance receipt.

## Transition event (`kind:44228`)

Legacy grant content is public JSON with exactly five fields — nothing
between, nothing beyond:

```json
{
  "genesisRef": "<genesis event id, 64-hex>",
  "prevAccepted": "<previous accepted transition id, 64-hex, or null>",
  "seq": 1,
  "type": "grant-operator",
  "granteePubkey": "<target pubkey, 64-hex>"
}
```

- `prevAccepted` is a **required key whose value is nullable** — never simply
  absent. Omitting it would be ambiguous between "the chain's first link" and
  "a malformed submission", and decoders reject that ambiguity.
- `seq` starts at 1 and increments by exactly one per accepted transition;
  `seq == 1` if and only if `prevAccepted` is `null`.
- `granteePubkey` is the transition's target: the grantee for `grant-*`, the
  pubkey losing its grant for `revoke`.
- `grant-seat` and `revoke-seat` use an exact six-key shape: the five keys
  above plus required `"role":"<normalized slug>"`. Legacy transition shapes
  remain exactly five keys and reject `role`. A role is exactly
  `[a-z0-9-]{1,64}`.

The event has exactly these three ordered, two-field tags — an envelope
mirroring genesis's own:

```json
[
  ["h", "<channel UUID>"],
  ["csat-v", "csat1-1"],
  ["csat-genesis", "<genesis event id>"]
]
```

`csat-genesis` is re-derived from the decoded content rather than trusted: the
storage transaction's chain lookup matches on the tag, so a disagreement
between tag and content would let a transition be stored under one genesis
while filed under another.

## Transition types

The type is a string enum precisely so further types (`transfer`, `takeover`)
are additive later; a relay that only understands the current set rejects any
other value as unknown rather than guessing at its meaning.

- **`grant-operator`** — grants `granteePubkey` standing to steer the session
  as an operator, without moving ownership.
- **`grant-viewer`** — grants `granteePubkey` read access to the session's
  events (its transport channel) without any steering authority. This is how
  a session owner shares a session with someone outside the project.
- **`revoke`** — removes whatever grant (operator or viewer) `granteePubkey`
  currently holds. A revoke naming a pubkey with **no live grant is refused**
  (`NoSuchGrant`): a no-op link would burn a `seq` for nothing.
- **`grant-seat`** — makes `granteePubkey` the active holder of exact `role`.
  It replaces that actor's prior seat, if any. Self-nomination is refused.
- **`revoke-seat`** — removes the actor's exact active `role`. A missing seat
  or role mismatch is refused rather than burning a sequence number.

Re-granting an already-granted pubkey with a different tier is a regrade, not
an error: the later accepted link wins.

## Relay processing

Envelope validation is pure (content shape, tag grammar); **chain linkage and
signer standing are validated atomically with storage**, because both require
the chain's current accepted head, which only the storage transaction can
answer:

1. `genesisRef` must name a stored `kind:44226` genesis, published in the
   same channel as this transition (`GenesisNotFound` / `WrongChannel`
   otherwise).
2. `prevAccepted` must equal the chain's current accepted head (or be `null`
   on an empty chain), and `seq` must be exactly head + 1 (`StaleHead` /
   `SeqMismatch`). A transition that lost a race is refused with the expected
   linkage named, so the publisher can refetch and resubmit.
3. Legacy `grant-operator`, `grant-viewer`, and `revoke` links remain founder
   signed. Seat links may be signed by the founder, an active operator grant
   with `may_steer`, or an active `lead` seat. A lead may manage only non-lead
   seats and cannot mint or revoke lead authority. Self-nomination is refused.
   The payload never restates the signer's identity; the signature settles it.
4. A `revoke` must name a pubkey with a live grant (`NoSuchGrant`).

On acceptance the relay publishes a relay-signed **acceptance receipt** — a
`kind:40099` system message in the same channel — naming exactly the facts a
consumer needs to establish the new canonical head:

```json
{
  "type": "coding_session_authority_transition_accepted",
  "genesisRef": "<genesis event id>",
  "acceptedEventId": "<the accepted 44228's event id>",
  "seq": 3,
  "transitionType": "grant-viewer",
  "granteePubkey": "<pubkey>"
}
```

For `grant-seat` and `revoke-seat`, the receipt adds required `"role"` echoing
the accepted link. Legacy receipts retain their original exact facts. An older
consumer may ignore the new seat transition semantics, but it must still
advance its accepted chain head/sequence so a later legacy grant does not
stall behind an accepted seat link.

A resubmission of an already-accepted transition is answered `duplicate:` and
mints no second receipt. Consumers reconstruct the live grant set by folding
receipts in `seq` order; providers additionally verify each receipt is signed
by the relay's own key before trusting it (lighter clients may match by
`acceptedEventId` and `content.type` against the relay they authenticated to).

## Capabilities

| Capability | Founder | Operator grantee | Viewer grantee | Lead seat |
|------------|---------|------------------|----------------|-----------|
| Steer session commands | ✓ | ✓ | — | ✓ |
| Read transport channel | ✓ | ✓ | ✓ | via membership/grant |
| Manage any role seat | ✓ | ✓ | — | — |
| Manage non-lead seats | ✓ | ✓ | — | ✓ |

**Steer** = founder or live operator grantee. **Read** = project member (via
the session's project, per [NIP-MP](NIP-MP.md)) or **any** live grantee —
a viewer grant is precisely transport-channel read access for someone outside
the project. Legacy grant transitions remain founder-only. Seat transitions
use the accepted-chain authority matrix above; no unsigned registry or
kind:44221/kind:44223 metadata can create role authority.

## Deterministic seat projection

Clients derive `actor -> role` only from accepted `grant-seat`/`revoke-seat`
links in the contiguous relay-receipted chain. They verify transition and
receipt signatures, exact channel/session genesis scope, `seq`, and
`prevAccepted`, then apply links in order. A later seat grant changes that
actor's role; a matching revoke removes it. Stale, revoked, wrong-channel, or
wrong-genesis links confer no authority. If no accepted seat link exists, the
actor is unauthorized; clients never infer a role from lifecycle metadata.

## Viewer read scope — a documented tradeoff

A viewer grant's read scope is **the session's transport channel**, not a
per-event allowlist. When several sessions share one transport channel, an
invited viewer of one session can therefore read the sibling sessions'
events in that channel too — grants are per-chain, but the read surface is
per-channel, and the channel is the unit the relay's access machinery scopes.

This is an accepted tradeoff, stated plainly rather than hidden: the
alternative (per-event filtering inside a channel) would put an authority
fold on every read path. Where isolation between sessions matters, **run one
transport channel per session** — the recommended deployment shape — and the
viewer scope collapses to exactly the granted session.

## Agent grantees

`granteePubkey` is just a pubkey — the chain draws no distinction between a
human's and an agent's. An agent seated on a sibling execution (`actor` on its
`session.create`, [NIP-CSL](NIP-CSL.md)) is granted `grant-operator` the same
way any collaborator is: a 44228 transition naming its pubkey. Grant tooling
(`bee sessions grant`/`revoke`) accepts that pubkey as 64-char lowercase hex or
an `npub1…` bech32 key — resolved locally to hex before the transition is
built, so the signed 44228 content and this chain's fold never see anything
but hex. `bee sessions roster` additionally marks a folded grant `agent: true`
when the channel's `kind:44223` metadata has ever named that pubkey as a
seated actor (`agentRef`) — a fact read back from the channel's own record,
not asserted by the roster reader.

Key custody for an agent grantee is never part of this chain. Resolving
*which* private key answers for an `actor` pubkey, injecting it into that
seat's process, and adding the actor to the channel so its signed 44220s are
even eligible are all host-local steps performed before the grant — see
[NIP-CSL § actor custody is host-local](NIP-CSL.md#fork-amendment-actor-custody-is-host-local-never-on-the-wire).
A `grant-operator` receipt on this chain is authority to steer, never a claim
about how the grantee holds its key.

## Relation to other NIPs

- **NIP-CSG** supplies the genesis (`kind:44226`) the chain roots at and the
  founder identity the relay checks signers against.
- **NIP-CSC / NIP-CSL** supply the command surfaces whose authorization reads
  the folded chain (founder or operator may steer). NIP-CSL also supplies the
  `actor`/`role` seat and its host-local custody, above.
- **NIP-MP** supplies project membership, the other route to transport-channel
  read access.
