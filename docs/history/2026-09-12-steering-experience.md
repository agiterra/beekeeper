# 2026-09-12 — the steering experience: an explicit delivery choice, honest downgrade reasons, and recovery for an unknown delivery

**Status:** implementation candidate on `work/native-steering-fable`, not landed,
not installed. Continues Fable's native-steering candidate
([`2026-09-11-native-steering.md`](2026-09-11-native-steering.md)) under
Astra's brief, [`2026-09-12-opus-steering-handoff.md`](2026-09-12-opus-steering-handoff.md).
The runtime design in [`../NATIVE_STEERING_IMPL.md`](../NATIVE_STEERING_IMPL.md)
is unchanged; its §3.5 client section is extended, not replaced.

Base for this work: `0963723b4` (Fable's ledger commit) on `main` `9aebb1262`.
Astra owns combined integration and installed acceptance. Nothing was pushed,
rebased, installed, or integrated with Astra's project-team branch.

## Gap assessment against the existing candidate

Recorded before editing, against the brief's three items.

| Item | Already true | Missing or wrong |
| --- | --- | --- |
| 1. Explicit delivery choice | Primary steers when working and capable; interrupt is a separate control; idle sends normally | No way to ask for the boundary on an execution that *can* steer — the composer chose for the person. The boundary label read "Send next". No visible explanation of what either class does. |
| 2. Pending and recovery | `sending`/`queued`/`degraded`/`injected`/`unknown` already distinct; the text-match fallback already reports itself as a guess and never as proof | No recovery for a delivery-unknown input. The degrade caption asserted "**Delivered** at the next turn boundary — **this provider cannot steer**" for every downgrade. The row could not do better: `resolveProgress` discarded the receipt's `code`/`message` and the row stored a boolean. No attachment count, so recovery could not disclose what it was not bringing back. |
| 3. Runtime guarantees | Durable intent, attempt correlation, idle guard, late reconciliation, authority and generation checks all intact | Nothing owed. The degrade reason was already on the wire; only the client threw it away. |

## What changed, and why

### The downgrade reason is carried instead of guessed

`turn_degraded` has always carried `{code, message}`. The client dropped it at
`codingSessionTurnReceiptIndex.ts` (`{stage: "degraded"}`), stored
`degradedByProvider: true`, and rendered one fixed sentence. Two claims in
that sentence could be false at once:

- **"Delivered at the next turn boundary."** It has not been delivered.
  `turn_degraded` is published beside the `turn_queued` that follows it; the
  turn is in the provider's mailbox. The row now says *queued for the next
  turn boundary*.
- **"This provider cannot steer."** True only for `STEER_UNSUPPORTED`. For
  `STEER_TURN_ENDED` — the turn ended before the input reached it — the
  runtime steers perfectly well, and the reader was sent looking for a
  capability defect that does not exist. The row now renders the provider's
  own reason, and an unrecognized code repeats the provider's message rather
  than inventing one.

The stage type is now `{stage: "degraded"; code; message}`, the pending row
stores `degradedByProvider: {code, message}` (required, so no caller can
reintroduce the fixed sentence), and `describeCodingSessionTurnDegrade` maps
the documented codes.

### A second delivery choice, named and explained

`getCodingSessionComposerState` gains `secondaryLabel` and `deliveryHint`.

- Idle: **Send**. No hint — there is no turn to steer or to wait for.
- Working, steering advertised: primary **Steer**, secondary **Queue next**,
  hint *"Steer joins the turn that is running. Queue next runs after it."*
- Working, no steering: **Queue next** alone, hint *"Runs after the current
  turn — this execution cannot steer."* No second button, because the primary
  already queues and two buttons would be one act under two names.

The label is "Queue next", never "Send next": the command *is* sent — signed,
published, irrevocable — and then waits. Interrupt remains its own control and
is untouched. The hint is on screen rather than in a `title`, for the reason
the codebase already argues about the irrevocability sentence: a tooltip is
invisible on touch and unannounced by most screen readers.

`submit` now takes an explicit intent. `"primary"` resolves to steer only when
this render's execution is working and advertised steering; `"boundary"` stays
a boundary send even where steering is available. The class is resolved
against the render's own `target` and capability, so a late capability update
or a target change re-creates the handler and cannot leak the previous choice.

### Copy to draft, which claims nothing

A delivery-unknown row gains **Copy to draft** beside **Dismiss**, and a line
above them: *"Copying leaves this message where it is. It may already have
reached the running turn."* — plus *"Its 2 images are not copied."* when the
turn carried attachments (the row now records `attachmentCount`).

It appends the person's own words to the composer using the same
`restoreCodingSessionDraft` the refusal path uses, so a half-written message
survives; it publishes nothing; and it leaves the row exactly as it was,
still saying delivery is unknown. Dismiss remains local dismissal only. No
Edit or Cancel control was added: a published command cannot be recalled, and
a control that only hid it locally would say otherwise.

The request travels through a small store in `codingSessionPendingTurns.ts`
rather than a callback threaded through two surfaces, and
`resetPendingCodingSessionTurns` clears it on a community switch with the rows
it refers to.

### One regression found and fixed: the dock reserve was a constant

The conversation reserved `pb-44` for the composer dock, which is absolutely
positioned over it. The dock's height is a function of its contents, so adding
the delivery hint pushed the last pending row's controls under the dock, where
they could not be clicked — the smoke spec caught it immediately. Fable's
report had already recorded the same brittleness as the reason the dismiss
control sits inside the bubble.

The reserve is now measured from the dock with a `ResizeObserver`, with
`pb-44` as the first-paint fallback and the task rail keeping its own larger
reserve. That removes the class of bug rather than moving the cliff. The hook
lives in `hooks/useCodingSessionWorkspaceLayout.ts` beside the existing
narrow-width hook, which moved out of `CodingSessionWorkspace.tsx` so that file
stays under the repository's 1,000-line ratchet (it was at 997 before this
change).

## Checks

Run in this worktree with the repo's hermit toolchain. Code commit
`ecd4336f4`; evidence logs are under
`../review-2026-09-11-native-steering/` (this machine only).

| Check | Result |
| --- | --- |
| `just ci` on `ecd4336f4` (clean tree, 23:38–23:51 UTC) | **exit 0**. Desktop unit 9285/9285; Rust 10610 passed, 0 failed; mobile 2011 passed. Log `ci-opus.log`. |
| `just smoke` alone on `ecd4336f4` (23:51–01:19 UTC) | **exit 1: 1309 passed, 9 failed, 1 skipped (1.5h)**. None attributable to this change; see below. Log `smoke-opus.log`. |
| Native-steer spec alone, `tests/e2e/coding-session-native-steer.spec.ts` | **5 passed** (58.5s) |
| Desktop typecheck, biome, `pnpm check:px-text`, file-size ratchet | clean; biome reports only the 3 warnings and 6 infos that predate this branch |

The nine smoke failures, each checked against evidence rather than assumed:

| Spec | Classification | Evidence |
| --- | --- | --- |
| `coding-session-observations.spec.ts:33`, `:114`, `:144` | pre-existing | three of the six ledger item 111 found failing on untouched base `77b792de9` |
| `coding-sessions.spec.ts:458` | pre-existing | ledger item 111 |
| `project-packs.spec.ts:40` | pre-existing | ledger item 111 |
| `coding-session-mission-lens.spec.ts:1121` | predates this change | fails identically (inspector reads *Goal not read yet*) on `0963723b4`, the commit before this change; passed in Fable's pre-rebase run, ~~so it arrived with the rebase onto `9aebb1262`~~. The differing runs establish an observed failure, not its introducing commit; the integration trace identifies delayed goal reads |
| `project-pr-review.spec.ts:1213` | predates this change | fails on `0963723b4`; ~~its error is *navigation invoked commands outside the read-only set: get_relay_self*~~. Corrected by Astra 2026-09-12: the original log fails the branch-created toast assertion at line 1234. See [the integration correction](2026-09-12-steering-integration.md). |
| `project-pulse-declared-work.spec.ts:232` | predates this change | fails on `0963723b4` with the same `get_relay_self` guard error |
| `mentions.spec.ts:326` | flake | passed on `0963723b4`, and 5/5 with `--repeat-each=5` on `ecd4336f4` |

The sixth item-111 spec, `coding-session-founder-acts.spec.ts:380`, passed in
this run. Base-commit reruns: `smoke-new4-on-0963723b4.log`,
`mentions326-repeat-on-ecd4336f4.log`.

Why the run took 1.5 hours: the suite is 1,319 tests on one Playwright worker
(the config has used one worker since the harness landed), and per-test
durations matched Fable's run. `TESTING.md`'s *about 47 minutes* dates from a
suite of about 1,149 tests and is stale.

Focused unit runs during development, all green: the pending-turn store (31),
the composer model, the pending-row captions (10), the refusal hook (15), and
the composer's optimistic/behaviour suite (9).

New behaviour tests:

- `codingSessionComposerModel.test.mjs` — the second choice is offered only
  where the primary steers; an idle execution says nothing about delivery
  classes; the boundary control is never labelled as a deferred send.
- `codingSessionPendingTurns.test.mjs` — a degrade keeps the provider's reason
  and the first answer wins; the attachment count is recorded; the recovery
  request is scoped, single-use and reset with the store; a receipt arriving
  out of order never demotes a later fact; identical words under different
  command ids settle one row each, reported as a text match.
- `CodingSessionPendingTurns.test.mjs` — a degrade never claims delivery and
  never invents the reason, across all four documented codes; an unrecognized
  code repeats the provider's own words.
- `CodingSessionComposer.optimistic.test.mjs` — a steering execution can be
  asked for the boundary explicitly; an execution that cannot steer offers one
  control and explains it; a capability that changes under the composer cannot
  leak the old class; a double press publishes one command; copy to draft
  recovers the words beside an existing draft, sends nothing, and leaves the
  unknown row in place (this one drives the real row and the real composer
  together).
- `coding-session-native-steer.spec.ts` — three new browser scenarios: an
  explicit boundary send signs a boundary command; a downgraded steer names
  the provider's reason and never claims delivery; copy to draft at 250% text
  recovers the words, signs nothing, and leaves the row unknown.

## Findings

**F1 — the coding-session panel cannot be used at 250% text in a short
window. Pre-existing; not introduced or fixed here.** At 250% the panel's own
chrome is about 1,000px tall: a 140px header, the founder banner, and a
composer dock measuring 635px once its controls wrap. In a 900×720 window the
conversation gets roughly 257px and the chrome covers all of it — the send
control sits under the header, and transcript controls sit under the dock, so
neither can be clicked. Measured, and independent of this slice: with the
delivery hint removed entirely, the header (y 463–603) and the send control
(y 529–619) still occupied identical coordinates. The browser scenario
therefore exercises 250% text at a normal window height, where the same
interaction passes. Worth its own numbered ledger item and an owner.

**F3 — three smoke specs broke between Fable's pre-rebase run and the rebased
candidate.** `coding-session-mission-lens.spec.ts:1121`,
`project-pr-review.spec.ts:1213` and `project-pulse-declared-work.spec.ts:232`
passed in Fable's run on the `77b792de9` base and fail on `0963723b4`, before
any change here. ~~Two report a command-guard error (`get_relay_self` outside the
read-only set).~~ Only declared-work has that failure; PR fails its success-toast assertion. ~~The rebase brought in Andy's two founded-session fixes, which
touch the projects container and the e2e bridge; that is the likely source but
is not proven here.~~ Astra's 2026-09-12 trace supersedes that attribution: the
PR creation succeeds immediately but its acknowledgement waits on unrelated
refreshes, and an older September 8 artifact already records that timeout.
Only declared-work fails the `get_relay_self` guard. See [the integration correction](2026-09-12-steering-integration.md). Owed an owner, alongside ledger item 111.

**F2 — the degrade reason was on the wire and discarded by the client.** Fixed
here. Recorded because the shape of the mistake is reusable: the receipt
carried `{code, message}`, the client's stage type had no room for it, and the
row then asserted the most common reason as if it were the only one.

## Limits

- The delivery classes are the two that exist. Atomic "interrupt and send
  *this* message", provider-side cancellation, reordering and agent-report
  batching remain deferred, as the brief scoped them.
- "Queue next" means eligible at the provider's next prompt boundary in
  provider order. It is not a promise to run ahead of another operator's
  queued work, and nothing here claims one.
- Copy to draft recovers text only. Attachments are disclosed as not copied;
  they are not re-staged.
- A published command still cannot be recalled. Dismiss is local.
- No Codex change: it has no idle guard and stays in boundary mode.
- Nothing here has been exercised in an installed build against a real
  adapter. The next installed test is Astra's: rebuild, send a unique marker
  mid-turn on a real Claude session, then confirm the row reads *Injected into
  the running turn* and settles on the steered echo; and force a
  delivery-unknown answer to confirm copy-to-draft returns the words with the
  row still standing.

## Ledger

New evidence is in this report, per the brief: ledger items 110 and 111
collide with Astra's independent project-team branch and are deliberately not
renumbered or rewritten here. Astra reconciles numbered references during
integration. The map's active-work row for native steering now links this
report beside Fable's.
