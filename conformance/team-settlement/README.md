# Team-settlement conformance vectors

`fixtures/settlement-vectors.json` is the one canonical statement of **when a
kind:44244 assignment is settled, by which rule, and what it is waiting for
when it is not**. Every reader of that projection runs it.

It exists because of ledger 210. An approving disposition that asks the
assignee for nothing now settles its assignment without an acknowledgement,
and the settlement projection gained `settledBy` to say which rule settled it.
That is a change to what a closed record *means*, and
`docs/UNIFIED_WORK_PLAN.md` § 8 A3.3 — the rule ledger 204 was written for —
requires every strict reader to ship in the same landing. These vectors make
that mechanical: a lane that changes the rule adds a vector, and every
reader's test fails until that reader is updated.

## The rule the vectors pin

An assignment settles when a canonical **approving** disposition
(`approve` or `approve-with-notes`) governs a canonical report for it, and
either

- the assignee has acknowledged that disposition — `settledBy:
  "acknowledgement"`, the rule that has always applied; or
- the disposition **asks the assignee for nothing** — `settledBy:
  "approving_disposition_without_ask"`.

"Asks for nothing" is mechanical and reads exactly two things: the `decision`,
and whether `requiredAction` is absent or blank. Prose is never read —
`summary` and `findings` are the ruling's own reasoning, addressed to every
reader of the mission. A lead who wants something from the assignee writes it
in `requiredAction`, or opens a new assignment.

**Selection order.** An acknowledged chain outranks every unacknowledged one,
whatever their order on the wire; among chains of the same kind the newest
disposition wins. So an assignment already settled by an acknowledged chain
keeps that chain as its governing evidence, and the new rule only settles
assignments that have no acknowledged chain at all.

**Settled is not acknowledged.** `approving_disposition_without_ask` says a
ruling asked for nothing. It does not say the assignee received it, agreed
with it, stopped working, or that its worktree may be removed. No surface may
render it as "acknowledged", "delivered" or "idle", and no resource or
lifecycle gate keys off it: worktree disposal reads the session's own kind
44230 closure (`crates/buzz-core/src/worktree_lifecycle.rs`), never an
assignment's settlement.

## Shape

Each vector carries `records` — symbolic ids, in arrival order — and an
`expected` settlement projection written in those same symbolic ids. A loader
signs the records with fixed keys (`founder` `0x11…`, `assignee` `0x22…`,
`stranger` `0x33…`, the assignee seated as `builder`), folds them, and
resolves the symbols.

| field | meaning |
| --- | --- |
| `records[].type` | `assignment`, `report`, `disposition`, `acknowledgement`, `completion` |
| `records[].supersedes` | the symbolic id this record corrects |
| `records[].settlementFact` | what `buzz-session-provider`'s `settlement_fact` must answer for this record, or `null` |
| `expected.assignments[]` | `settled`, `settledBy`, `governedReport`, `disposition`, `acknowledgement`, `awaiting` |
| `expected.terminal` | the symbolic id of the canonical terminal, or `null` |
| `expected.pendingCompletion` | a held completion, its wire `code` and the assignments it names that are unsettled |
| `expected.excluded` | every record the fold excludes, held completions included |

## Readers that run these vectors

| Reader | Test |
| --- | --- |
| `buzz-core`'s fold — the rule itself | `crates/buzz-core/src/coding_session_team_transaction_fold_settlement_conformance_tests.rs` |
| `buzz-cli`'s `fold` object (`bee sessions operation list\|get`, `sessions complete`) | `crates/buzz-cli/src/commands/sessions/operations_completion_tests.rs` |
| `buzz-session-provider`'s settlement-fact classifier | `crates/buzz-session-provider/src/pending_completion_tests.rs` |
| The desktop's strict native-fold decoder and the Mission panel's Settlement section | `desktop/src/features/coding-sessions/lib/codingSessionTeamSettlementConformance.test.mjs` |

The desktop reader also asserts the two failures ledger 204 is about: an
assignment row carrying a key the decoder does not know is refused, and a row
*missing* `settledBy` is refused rather than read as acknowledged.
