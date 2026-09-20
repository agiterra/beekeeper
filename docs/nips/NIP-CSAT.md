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
- `grant-project-actions` and `revoke-project-actions` use their own exact
  six-key shape: the five keys above plus required
  `"projectRef":"30621:<64-hex owner>:<d>"`, at most 256 bytes. They reject
  `role` and `bodyPubkey`, and every other type rejects `projectRef`. The
  signed content ceiling is 768 bytes (raised from 512 when this shape
  landed, so a long project `d` cannot make a grant unsignable).

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
- **`grant-project-actions`** — delegates the actions of the one project named
  by `projectRef` to `granteePubkey`, and nothing else. See
  [Project-action delegation](#project-action-delegation).
- **`revoke-project-actions`** — withdraws that delegation for the exact same
  `(granteePubkey, projectRef)` pair. A revoke naming a pair with no live
  delegation is refused (`NoSuchGrant`), the same rule as `revoke`.

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
4. A `revoke` must name a pubkey with a live grant (`NoSuchGrant`), and a
   `revoke-project-actions` must name a live delegation of the same project
   (also `NoSuchGrant`).
5. `grant-project-actions` and `revoke-project-actions` are **owner-signed**:
   they fall under rule 3's legacy arm, so only the session's current owner
   may extend the chain with one. For a project team session that owner is the
   founder, which is exactly the "signed by a project owner" requirement the
   delegation needs — the relay checks the granter's *current* project
   standing again at admission time (below).

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
the accepted link; for `grant-project-actions` and `revoke-project-actions` it
adds `"projectRef"`, because a receipt that said only "a delegation was
accepted for this key" would leave its scope to be guessed at. Legacy receipts retain their original exact facts. An older
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
| Publish/trigger a project's actions | via project standing | via project standing | — | only with a live `grant-project-actions` |

**Steer** = founder or live operator grantee. **Read** = project member (via
the session's project, per [NIP-MP](NIP-MP.md)) or **any** live grantee —
a viewer grant is precisely transport-channel read access for someone outside
the project. Legacy grant transitions remain founder-only. Seat transitions
use the accepted-chain authority matrix above; no unsigned registry or
kind:44221/kind:44223 metadata can create role authority.

## Project-action delegation

`grant-project-actions` exists because a team session was founded on the goal
"build kettle with tests and a verify action, land it on main" and could not
finish it (ledger 186, finding 178(f)). Publishing a project action
(`kind:30620`) and starting a manual run of one (`kind:46020`) are admitted
for the channel owner or admin plus the project's creator, a roster owner or
an endorsed repository's founder — and a lead seat is none of those. The lead
was refused, opened a ruling on the founder, and the session stopped. The
system had accepted a routine goal it lacked the standing authority to finish.

**Exact shape.** Six keys, the five legacy ones plus `projectRef`:

```json
{
  "genesisRef": "<genesis event id, 64-hex>",
  "prevAccepted": "<previous accepted transition id, 64-hex, or null>",
  "seq": 4,
  "type": "grant-project-actions",
  "granteePubkey": "<lead seat pubkey, 64-hex>",
  "projectRef": "30621:<64-hex project owner>:<project d>"
}
```

The tags are the unchanged three (`h`, `csat-v`, `csat-genesis`): the
delegation is a link in one session's chain, not a free-standing record, so it
is revoked, ordered and raced exactly like every other link and it dies with
nothing else.

**What it delegates.**

| Act | Kind | Delegable |
|-----|------|-----------|
| Save or update this project's action definition, including one with `run_on_host` steps | 30620 | ✓ |
| Start a manual run of one of this project's actions | 46020 | ✓ |
| Approve a host step so a command runs on an operator's machine | 46030 | **✗** |
| Anything about another project, or any steering, hiring or read authority | — | **✗** |

Approval is deliberately outside it. Approval is the separate act of letting a
command run on somebody's machine, and a seat that could both write the
command and approve it would be no boundary at all. The synthetic approval
gate before the first `run_on_host` step
(`PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.4) is untouched by any delegation.

**What the relay checks at admission.** The standing rules are unchanged and
are asked first; the delegation is consulted only after they have refused, and
it admits the act only when all of the following hold:

1. the capability is one a delegation may carry at all (the table above);
2. the caller holds a live `grant-project-actions` for this exact
   `projectRef`, folded from a **whole** contiguous chain in the channel the
   action is filed in — a chain that cannot be read whole decides nothing
   rather than deciding from a prefix;
3. the pubkey that signed the delegation is admitted to write this project
   *now*, by the same creator/roster-owner/repository-founder rule, so a
   granter who has since lost ownership leaves no capability behind;
4. the grantee still holds that session's active `lead` seat, because what
   was delegated is a lead's ability to carry out the project's work, not a
   permanent capability attached to an identity.

Every refusal names the missing fact — no live delegation, a granter who no
longer writes, a holder who is not the lead, a chain that could not be read —
because a bare "forbidden" is the refusal that made a lead ask a person.

**Who issues it, and when.** Beekeeper's founding form signs it for the
session's lead at Start, in the same step that signs the lead's `grant-seat`,
when the founder is one of the project's owners; the readiness panel discloses
either that ("This session's lead may publish and trigger this project's
actions") or, when the founder is not an owner, that it cannot be signed and
which keys can sign it.

## Deterministic seat projection

Clients derive `actor -> role` only from accepted `grant-seat`/`revoke-seat`
links in the contiguous relay-receipted chain. They verify transition and
receipt signatures, exact channel/session genesis scope, `seq`, and
`prevAccepted`, then apply links in order. A later seat grant changes that
actor's role; a matching revoke removes it. Stale, revoked, wrong-channel, or
wrong-genesis links confer no authority. If no accepted seat link exists, the
actor is unauthorized; clients never infer a role from lifecycle metadata.

The relay uses the same accepted-chain projection when admitting
`session.hire`: founder and active `may_steer` operator may hire any role; an
active `lead` seat may hire only a non-lead role. It resolves the hire's exact
`genesisRef` before consulting seats, so a stale or wrong-genesis request
cannot borrow lead standing from another session with the same label.

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
