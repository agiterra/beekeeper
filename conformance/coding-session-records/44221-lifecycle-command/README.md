# kind:44221 — coding-session lifecycle command

`{schema, commandId, action}`, `schema` exactly
`buzz-coding-session-lifecycle-command/v1`. The action is a closed tagged union
on `type`.

## Closed key set per action

| Action | Keys |
| --- | --- |
| `session.create` | `type, projectRef, repoRef, [sessionRef], [genesisRef], providerInstanceRef, providerAuthorityPubkey, model, title, initialTurn, [actor, role], [hireRef], [routing]` |
| `session.hire` | `type, sessionRef, genesisRef, role, providerInstanceRef, model, brief, [requestedBy], [routing]` |
| `session.resume` / `session.restart` / `session.stop` | `type, session, providerAuthorityPubkey` |

Three rules make the bracketed keys exact rather than approximate:

- **A nullable key is structurally required.** `projectRef`, `repoRef`, `model`,
  `title`, `initialTurn` are written as explicit `null` when absent, so a
  truncated payload can never be mistaken for a deliberate choice.
- **An additive key is omitted, never written as `null`.** `genesisRef`,
  `actor`/`role`, `hireRef`, `routing`, `requestedBy` postdate the v1 shape, so
  events without them exist and must stay valid forever. An explicit `null` is
  *present* to a key-set check and would mean a different thing to a strict
  reader than to a lenient one — that pun is refused.
- **Three historical create forms** (8, 9 and 10 keys) × seated × attributed ×
  routed = the twenty-four accepted create shapes `beekeeper-core` builds rather
  than lists.

`genesisRef` never appears without `sessionRef`. `actor` and `role` travel
together or not at all. A create carries the routing **record** (the answer); a
hire carries the routing **request** (the question), and each refuses the
other's shape by the name of the key that does not belong (ledger draft 97).

## What is not guarded here

The hire refusal codes, the brief's byte ceiling, and the `requestedBy`-versus-
signer comparison are behaviour, not key sets. `session.restart` and
`session.hire` are covered here only as *variants*; the mobile reader is a
create decoder and its sibling `decodeCodingSessionResume` is not exercised by
these vectors.
