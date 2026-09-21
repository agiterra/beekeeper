# kind:44223 — coding-session per-generation metadata

`schema` exactly `buzz-coding-session-metadata/v1`. Twelve base keys plus nine
independent additive amendments, each present or absent on its own.

## Closed key set

**Base (all twelve, always):** `schema, session, projectRef, repoRef, title,
agentRef, provider, runtime, model, status, branch, capabilities`.

**Amendments, each independent:** `sessionRef`, `role`, `turnBudget`,
`routing`, `beeStamp`, `packRef`, `handover`, `composeRef`, and the four B1
coordinate facts `observedCommit, dirty, relayReachable, verifiedAt` **taken
together** — all four or none.

That is 512 accepted shapes. Every one of the eight single keys is *omitted*
when absent, never written as `null`: `buzz-core` refuses an explicit null
naming the key, because a producer with nothing to say omits it.
`composeRef` additionally requires `packRef` — a composition of nothing is not
a fact.

`status` is a closed ten-variant enum. `capabilities` is deliberately an **open**
set (finding 34): a newer host's `promptImage` must not drop the session.

## Where the readers disagree today

Four readers, and three of the four are missing a key the others have —
`handover` (desktop ingress), `composeRef` (the Pulse gate), and all four of
`beeStamp`/`packRef`/`handover`/`composeRef` (mobile). The gate refusing
`composeRef` while the ingress decoder accepts it is the exact direction
`sessionCoordinationStrictJsonParity.test.mjs` says must never happen. See the
`accepts` block on each vector.

`conformance/project-pack-source/fixtures/pack-source-vectors.json` covers the
`packRef` half of this record in depth and stays where it is; this file does not
duplicate it. Its TypeScript side still compares two local readers rather than
loading those vectors — that gap is recorded, not closed, by lane 215.
