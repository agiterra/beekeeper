# Authority-chain conformance vectors

`fixtures/chain-vectors.json` is the one canonical statement of what a
kind:44228 authority transition and its relay-signed kind:40099 acceptance
receipt may contain, per transition type. Every strict reader of that pair
runs it.

It exists because of ledger 204. Lane 186 added the `grant-project-actions` /
`revoke-project-actions` types with a `projectRef` key and taught the relay to
echo `projectRef` onto the acceptance receipt, but one strict reader — the
CLI's `deny_unknown_fields` receipt struct — was never taught the key. Since
the desktop signs that delegation at launch for every team session a project
owner founds with "Use roles" on, every such session's chain became
unreadable: `bee sessions hire`, `seat-repair`, `report`, `operation get` and
`pulse digest` all failed with *invalid accepted authority chain: malformed
authority acceptance receipt: unknown field `projectRef`*, and no seat could
publish a canonical report. That is the closed-envelope rule of
`docs/UNIFIED_WORK_PLAN.md` § 2 decision 2: additive change is allowed only
when every strict reader ships in the same landing. These vectors make the
rule mechanical — a lane that adds a receipt key adds a vector, and every
reader's test fails until that reader is updated.

## Shape

Each vector carries a transition content object and the receipt content
object that names it, with three independent expectations:

| field | meaning |
| --- | --- |
| `transitionValid` | the signed 44228 content decodes |
| `receiptValid` | the 40099 receipt content decodes and its key set matches its `transitionType` |
| `binds` | the two describe one link, so a reader may fold them together |

`binds` is separate because a receipt and a link can each be well formed and
still disagree — the `project-actions-receipt-naming-a-different-project`
vector is exactly that, and a reader that skipped the equality check would
fold a delegation of a project the signer never named.

`acceptedEventId` is a placeholder: a loader that signs real events replaces
it with the transition's own id.

## Readers that run these vectors

| Reader | Test |
| --- | --- |
| `beekeeper-core`'s transition decoder (shared by the relay's ingest and `beekeeper-db`) | `crates/beekeeper-core/src/coding_session_authority_transition_project_action_tests.rs` |
| `beekeeper-cli`'s receipt-backed chain projection (`bee sessions report`/`hire`/`operation get`) | `crates/beekeeper-cli/src/commands/sessions/operations_receipt_tests.rs` |
| `beekeeper-session-provider`'s acceptance-receipt fence | `crates/beekeeper-session-provider/src/authority.rs` |
| The desktop TypeScript timeline decoder | `desktop/src/features/coding-sessions/lib/codingSessionAuthorityConformance.test.mjs` |

The provider deliberately tolerates *unknown* receipt keys (see
`ReceiptContent`), so the vectors vary only keys the wire defines; what they
pin everywhere is the per-type presence rule and the receipt↔link binding.

The relay's emission (`crates/beekeeper-relay/src/handlers/side_effects.rs`) is the
writer these vectors describe. It is not a reader and does not load them; the
CLI and provider fixtures mirror it field for field instead.
