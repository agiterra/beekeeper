# Two Opus lanes on seat isolation — orchestration report, 2026-09-15 evening

Fable coordinated two Opus 5 sessions from 20:36 to 22:15 EDT on the question
"is per-seat worktree isolation the right design, or is it compensating for
how role material reaches a seat?" The design review that framed the work was
independent from Astra's (GPT-6) and the two converged: keep worktrees for
anything that mutates git, move role delivery out of the checkout, and make
verification inputs explicit commits. This file records what was built, what
was verified, and what is still owed. Findings are in ledger items 131–134.

## The candidate stack — not pushed, not installed

Eight signed commits, linear on `main` `0f87b13d4`, tip `a339e020b` on
`work/seat-bundle-core-opus` in `/Users/brian/Projects/beekeeper/review-verification-inputs-opus`:

| Commit | Slice | Ledger | Lane |
| --- | --- | --- | --- |
| `2606c2fc6`, `8af9cad4a` | Seat skills materialize to an execution-owned bundle outside the checkout; briefing names absolute paths; no `.agents/` in the seat tree | 132 | Opus A |
| `4395c4e85`, `b0775c57f` | Verifier and runner assignments must name a commit at sign time (null tolerated on reads); the host establishes it in the seat's worktree and discloses it | 131 | Opus B |
| `a8ba0e525`, `75eff4a56` | A seat's bundle is removed when the host removes its worktree; orphan list and remove commands | 134 | Opus B |
| `33f5789b6` | The provider refuses to open a verifier or runner turn whose input the seat does not hold; transport unknowns leave the turn undecided rather than consuming it | 133 | Opus A |
| `a339e020b` | Bundle constants and the session-id sanitizer live once, in `buzz-core` | closes 134(a) | Opus B |

Every commit was gated bare from its worktree by the lane that made it, and the
tip was gated again after each rebase. Final lines on `a339e020b`: workspace
and Tauri fmt clean; workspace and Tauri clippy clean; `buzz-core` 1124,
`buzz-session-provider` 827 (1 ignored), Tauri 3394 (18 ignored) passed;
`just desktop-check` and `just current-state-check` clean. Nothing has run
against a live seat or relay.

## What Brian does next

1. Install the tip: `scripts/app-from.sh a339e020b`. Each rebuild prompts for
   the login keychain.
2. Run the contrasting-pack experiment in
   [`2026-09-15-seat-bundles-experiment.md`](2026-09-15-seat-bundles-experiment.md)
   and fill in its result tables. Bundles land under
   `~/Library/Application Support/io.agiterra.beekeeper.app.dev/agents/seats/<session id>/`.
3. Land the stack in order, rebased onto `main` (`git rebase --signoff main`).
   The ledger auto-merges with no conflict markers but out of numeric order;
   check that 130–134 read in order after every rebase.

## Decisions taken during the run

- Verification inputs are an **assignment** fact, not a hire fact. Hires still
  cut from trunk (`source: null`); the assignment's `baseSha` names the commit.
- The fence lives in the **provider**, not the CLI or the relay. The desktop
  wakes only the lead; the assignee wake is minted by `buzz-cli`
  (`crew_cmds.rs` `send_team_operation_wake`); the relay validates the
  envelope through the same tolerant reader it must use for replays. Only the
  provider both holds the seat's cwd and opens the turn.
- **Unknown is not false.** A relay or fold failure at wake time leaves the
  turn undecided and re-read on the next inbox pass, bounded by the existing
  one-day command horizon. Fact refusals are durable and name "re-issue the
  assignment" as the remedy, proven not to be a hang by
  `a_re_issued_assignment_is_admitted_after_a_refusal`.
- A held worktree keeps its bundle. Bundles go with the tree, not with build
  output.
- Tolerant of `"baseSha": null`, not of a missing key: the payloads deny
  unknown fields with no default, so an absent key fails to decode before the
  role rule is reached.

## Still owed

- Bind the verified commit into the turn receipt (receipt key sets are closed
  in `buzz-core/src/coding_session_payload.rs`); today it is logged and reaches
  the wire through the next observed-commit refresh (133).
- No surface consumes the orphan-bundle commands or the assignment-input
  record outside the mission inspector (131, 134).
- The establish step is not ordered against the lead's wake; the fence is what
  makes the order irrelevant, and that fence is unproven live.
- Later slices, in order: the Claude write fence moves to `_meta` settings via
  a new `AcpClient` setter; a shared cargo target directory per repository.

## Process notes

- Both lanes were briefed with strict file ownership and disjoint crates; the
  one collision point (`session.rs`) was sequenced, never shared.
- Fable's briefs contained three errors the lanes caught with evidence: a
  rebase onto a stale local `main`; the desktop team-wake path named as the
  place to establish inputs; and a "pinned by test" precedent that was only a
  comment. Each correction was verified before it was accepted.
- Add `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --all --check`
  to every lane brief: the root workspace excludes the desktop crate.
