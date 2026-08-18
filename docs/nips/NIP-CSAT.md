# NIP-CSAT — Coding-session authority transitions

`draft` `optional` `relay`

`kind:44228` is one append-only link in one coding session's **authority
chain** — the sequence of decisions about who may steer or read a session
after its genesis (`kind:44226`, [NIP-CSG](NIP-CSG.md)) founded it. The chain
is rooted at the genesis event **id** (never a `sessionRef` label, for the
same resolution-by-id reason genesis itself pins), and every accepted link is
confirmed by a relay-signed acceptance receipt.

## Transition event (`kind:44228`)

Content is public JSON with exactly five fields — nothing between, nothing
beyond:

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
3. The signer must be the session's **owner** — today the genesis signer
   (`SignerNotOwner`). The payload never restates the signer's identity; the
   signature settles it.
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

A resubmission of an already-accepted transition is answered `duplicate:` and
mints no second receipt. Consumers reconstruct the live grant set by folding
receipts in `seq` order; providers additionally verify each receipt is signed
by the relay's own key before trusting it (lighter clients may match by
`acceptedEventId` and `content.type` against the relay they authenticated to).

## Capabilities

| Capability | Founder (genesis signer) | Operator grantee | Viewer grantee |
|------------|--------------------------|------------------|----------------|
| Steer the session (commands, goal edits, transitions) | ✓ | ✓ (commands; not transitions) | — |
| Read the session's transport channel | ✓ | ✓ | ✓ |

**Steer** = founder or live operator grantee. **Read** = project member (via
the session's project, per [NIP-MP](NIP-MP.md)) or **any** live grantee —
a viewer grant is precisely transport-channel read access for someone outside
the project. Extending the chain itself (publishing 44228s) remains
owner-only in this revision.

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

## Relation to other NIPs

- **NIP-CSG** supplies the genesis (`kind:44226`) the chain roots at and the
  founder identity the relay checks signers against.
- **NIP-CSC / NIP-CSL** supply the command surfaces whose authorization reads
  the folded chain (founder or operator may steer).
- **NIP-MP** supplies project membership, the other route to transport-channel
  read access.
