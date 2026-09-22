# Unified work plan — execution

2026-09-20. Direction: Brian. Orchestrator: Fable. Design source: Astra's
[unified plan](history/2026-09-20-astra-unified-plan.md) (the *what*), built on
Andy's [project teams spec](PROJECT_TEAMS_AND_ACTIONS_SPEC.md) and the landed
batch 180–191 (ledger). This file is the *how*: waves, gates, ownership rules,
review and proof protocol, and the decisions already taken. It defines work; it
claims nothing is built. Status lives in `CURRENT_STATE.md`, findings in
`SESSION_STATE.md`. Where this file and Astra's plan disagree, this file wins
and says why.

## 1. The outcome, in one sentence

Brian states a goal once; the lead adopts a committed plan file as the
contract; software prepares, briefs, verifies the exact artifact, observes
delivery and settles completion; and the project can say what remains, who owes
it and what proves it — with **zero unplanned human repairs** on a
kettle-sized task and under 60 minutes goal to terminal record (today: 5 h 34 m
goal→terminal, ~35 min of team work, six operator-counted repairs — 7
founder-signed non-host-answer commands on the wire; ledger 178(o), 178(r)).

## 2. Decisions taken (do not reopen without new evidence)

1. **The join.** A work declaration is a signed reference to
   `plans/<file>.md` in the agents repository at a full commit, schema
   `beekeeper-plan/v1`, criteria with stable slug ids and one `proof` kind
   (`review` | `action` | `git-ref`). Criteria text is never copied onto the
   wire. Answers Andy's open questions 8 and 9 (no grant split in v1).
2. **One sibling kind** for work records (declaration, assignment binding,
   evidence binding). 44244 stays closed and unchanged. Elsewhere additive
   change is allowed when every strict reader ships in the same landing.
3. **Completion** consumes lane 183's shape: the signed `mission.completed`
   settles by re-fold. Work coverage is a second, separate question.
4. **Eight identities stay installed; eight is not a staffing default.** Role
   text changes ship as new template versions (1.1.0); nothing staged mutates.
5. **Changed role text** is adopted by explicit restart; a **changed action
   definition** never runs under an old approval. Different objects.
6. **We do not wait on Andy** (Brian, 2026-09-20; equal owners, no human
   gates). He gets a drafted note, sent on Brian's word.
7. **Landing method is ours, not Astra's §4 preamble:** lanes commit in their
   own worktrees; a finalizer agent stacks, gates bare, lands with `just push`,
   edits the map and installs. **Astra is the adversarial reviewer** of each
   integrated wave, read-only — the cross-provider review the vision asks for.
8. **W3 is three sequential lanes** (W3a preparation in the provider, W3b work
   brief, W3c replacement and fencing), because they share the provider's
   riskiest files. W3c waits for the control run to pass.
9. **W4 (approval survives a definition change) goes first and alone.** It is
   the only item that is a safety defect in code that runs scripts on an
   operator's machine (Astra §7; static counterexample, to be reproduced).

## 3. What was missing, and is added here

- **W0 — the frozen contract.** Before any parallel lane: one small lane writes
  `conformance/project-work/` — JSON examples of each record, the plan-file
  schema with the kettle example, the fold's output type, the CLI command
  surface (`sessions work validate|adopt|bind|status`) and the brief's field
  list — plus the kind number allocation in `kind.rs`. Fable and Astra review
  it; after that it changes only by an explicit amendment recorded in § 8.
- **Preflight.** An agent verifies, against current `origin/main`, every path
  in the ownership table: exists or is declared new; no path in two lanes;
  ratchet headroom on each desktop file; the next free migration number; the
  kind number unused. Its corrections amend § 5 before Wave 1 starts.
- **M — the measurement tool.** `scripts/measure-session.mjs` (or
  `bee sessions measure`): reads a session's relay events and prints the
  scorecard and the per-seat cost table (turns, tokens by category, reported
  cost coverage, orientation and schema-refusal counts, ACK turns, waits by
  cause, goal→adoption→…→terminal timestamps, human actions). Today's audits
  were done by hand by a model; the next ones are a command. Built in Wave 1.
- **Deploy coupling.** hive deploys itself from a green pipeline in ~25 min
  (push → CI 14 min → timer ≤5 → build). Any lane that changes relay ingest
  lands **relay-first**: the finalizer waits for hive's NIP-11 `build_time` to
  pass the landing time before installing a desktop that depends on it. Old
  desktops must ignore the new kind (W1 proves it with a reader test).

## 4. Waves and exit gates

| Wave | Lanes (parallel unless noted) | Exit gate |
| --- | --- | --- |
| 0 | W4 approval/definition binding · W0 contract · preflight · land 192 | W4's Postgres+host test shows a changed command executes zero times under an old approval; W0 reviewed by Fable and Astra; ownership table corrected |
| 1 | W1 core+sdk+relay for the sibling kind · W6 role templates 1.1.0 · M measurement | Same events in any order fold to the same coverage; old reader ignores the kind; hive serves it (build_time check); `measure` reproduces the numbers in Astra's kettle audit within tolerance |
| 2 | W2 CLI · W3a provider-owned preparation and wake ordering · W5 surface | CLI adopts/binds/status against fixtures; a verifier assignment starts on its exact input with the Mission panel never opened and a wake raced ahead of checkout; UI and CLI agree |
| 3 | W3b work brief → integrate → Astra adversarial review → **control run** | Control run on a fresh project: every criterion at the adopted plan and delivered SHA, terminal record, zero unplanned repairs, <60 min gross, measured by M |
| 4 | W3c replacement and fencing → **disruption run** | Astra's experiment: lose one worker, keep its sibling, amend one criterion mid-run, restart at two boundaries, suppress one subscription delivery; completes with no repair prompt |

A wave does not start until the previous gate is met or Fable records why a
gate item moved. Findings from a run go to the ledger the same day and back to
the owning lane; the finalizer never edits a lane's files.

## 5. Ownership

Astra's §4 table is the starting table. Rules that bind every brief:
strict file ownership, one reserved ledger number per lane (193 onward, in
wave order), gates listed per lane and run bare after committing, NUL check,
no ratchet bumps, no `unwrap()`/`expect()` in production paths, doc comments
on new public items, `$BEE`/bundled `bee` only, commit `-s`, never push.
Amendments from preflight are recorded in § 8.

Known corrections already: W3 splits as in § 2.8 (same files, sequential).
W5's `Run` must carry `--checkout` (lane 184 left the desktop button
without it). W7 is not a lane: it is the finalizer plus the two runs.

## 6. Review and proof protocol

- **Per lane:** the lane reports ≤400 words; Fable rules accept / rework.
- **Per wave:** finalizer stacks → all gates bare on the tip → `just push` →
  wait for CI green and, if the relay changed, hive's `build_time` → install.
  Then Astra reviews the integrated diff read-only against this file's gates
  and returns findings with file:line; Fable triages; owning lanes fix.
- **Runs:** fresh project each (`kettle-control`, `kettle-disruption`), never
  the audited sessions. Brian types the goal and performs only the
  *scheduled* actions (first host approval; the injected steer and faults).
  Every unscheduled human action is a finding. M produces the numbers;
  an investigator agent reads transcripts only to explain an anomaly.
- **Honesty rule:** a green suite is not a live proof; "unknown" is a valid
  result; a claim cites file:line, an event id, or M's output.

## 7. What only Brian decides

1. Availability for the two runs (keyboard needed: keychain prompt, one host
   approval, the scheduled faults). Each run is under two hours.
2. The note to Andy (drafted by Fable when Wave 0 lands; sent on Brian's word).
3. Any widening of scope beyond § 4, and any authority change (who may
   approve host steps stays with the project owner; no lane may move it).

Everything else Fable decides and reports.

## 8. Amendments

**A1 — preflight against `origin/main` `e26de4933`, 2026-09-20.** Binding on
every Wave 1+ brief.
1. `crates/buzz-core/src/kind.rs` belongs to W0 alone. W1 uses the constant and
   never edits the file. Expected allocation: **44249**, the lowest number
   neither used nor reserved (44231–44239 and 44241–44243 are reserved).
2. `desktop/src-tauri/src/handlers.rs` (the `generate_handler!` list; it is not
   in `lib.rs`) was owned by nobody: it is **W5's**. W3 keeps lane 185's
   existing Tauri commands as adapters and adds no registration; if it must,
   the line goes through W5.
3. `desktop/src-tauri/src/coding_sessions/mod.rs` is **W3's**.
4. `crates/buzz-persona/src/lib.rs` (one `mod` line for the new test) is
   **W6's**, not the finalizer's.
5. W3's test file is `crates/buzz-session-provider/src/tests/verification_input_tests.rs`.
6. Ratchet: `coding_sessions/workdir_store.rs` is at **999/1000**. W3a's first
   commit splits it into a sibling module before adding a line. Also over 850:
   `CodingSessionMissionInspector.tsx` 951 (W5 adds a child component, not
   lines), `coding_session_team_fold_tests.rs` 946 (W5 adds a new test file),
   `assignment_input.rs` 865 and its tests 867 (W3a moves code out, net
   negative).
7. Sizing, for briefs: provider `lib.rs` is 21,149 lines, `session.rs` 6,708,
   CLI `lib.rs` 6,820. W2 and W3 briefs name the functions they touch, not the
   file, and lanes add new modules rather than growing these.
8. Migration `0046` is free and is W4's.

**A2 — Astra's adversarial review of Wave 0, 2026-09-20**
([review](history/2026-09-20-astra-wave0-review.md)). All eight findings
accepted; the Wave 0 gate reopens until 198–200 land.
1. Lane 199 (approval races): an action-scope grant binds to the approved
   run's stored hash, never a later workflow read; run creation admits the
   caller's expected hash atomically and the executor checks the hash of the
   definition it is about to run. Each race is reproduced before it is fixed.
2. Lane 198 (contract): the fold's inputs carry verified evidence facts and
   caller-compiled action definitions, with named predicates per proof kind;
   coverage is complete only over **one candidate artifact commit**
   (`mixed_artifacts` otherwise); authority is the existing 44244 `may_lead`
   predicate including active steer grantees; `goalRef` names only the goal
   and a new nullable `decisionRef` names a decision; conflict is defined over
   maximal declarations per `workId`, two roots included.
3. Lane 200: the desktop's tag-refusal fallback matches the refusal exactly.
4. Process: the implementer never writes its own oracle. W1 builds the parts
   A2 does not touch and takes the amended fixtures from lane 198's branch.

**A3 — Wave 2 landing and the Kettle Smoke run, 2026-09-20** (ledger 201–205).
1. **One assembler.** `buzz_core::project_work_inputs::assemble_fold_inputs`
   is the only path from fetched events to the fold's inputs. The CLI, the
   desktop and the provider call it; no surface decodes 44244 reports, pairs
   a host result with its echo and request, or picks a goal on its own.
2. **The CLI surface in `conformance/project-work/README.md` § (d) was
   under-specified.** It omitted the channel a 44249 needs for its `h` gate
   and the genesis and project its envelope carries, and named no
   `--responsible`. The landed CLI's spelling is normative; the README is
   corrected by the next contract touch.
3. **Every additive key on a closed record lands with a shared conformance
   fixture loaded by every strict reader** — the 204 rule, after a `projectRef`
   echoed by the relay but unknown to the CLI's `deny_unknown_fields` reader
   made every owner-founded team session's authority chain unreadable. A lane
   that adds such a key names every reader in its report.
4. **Findings from a run are triaged by the orchestrator the same day** into
   exactly three dispositions: a lane now, already in flight, or a recorded
   observation with no lane.

**A4 — decisions from the Kettle Smoke run (ledger 205), 2026-09-21.**
1. **What the measurement says costs tokens.** A seat's input per turn is its
   actual context times the number of tool calls in the turn (every call
   re-reads the context); the window size is free until it fills. Closing the
   protocol on Kettle Smoke cost 11 turns and 8.2M cache-inclusive input
   against 5 turns and 8.5M for the work. The levers, in order: fewer wakes,
   fewer tool calls per turn (orientation), smaller contexts (a brief instead
   of rediscovery), cheaper worker models (routing, proven live). A
   "context size in the registry" lane is **not** built: it would move nothing.
2. **An approving disposition that asks the assignee for nothing settles the
   assignment without an acknowledgement** (lane 210). The provider cannot
   sign a receipt for a seat (its key is not kept; ledger 183(f)), so the
   honest fix is to stop requiring a receipt that carries no information. A
   disposition that is not approving, or that carries an ask, still needs an
   explicit answer. The settlement projection says which rule settled it.
   Old sessions are read by the same rule and nothing already settled changes.
   Conformance fixtures for the team-transaction settlement land with it, in
   the 204 style, loaded by every reader.
3. **W3b, the host-assembled work brief, carries every identifier a seat
   needs** (lane 209): channel, session, genesis, assignment and report
   references as ready-to-run commands, the criterion excerpts with their
   provenance, the exact base, effective permissions and the model's source.
   Tonight's lead typed those ids into its hire brief by hand; seats then
   spent a quarter to a third of their tool calls on orientation.
4. **Wave 3, revised:** 209 work brief · 210 acknowledgement contract · land
   208 (mobile flake, named failures) · Astra's adversarial review of Wave 2
   · the control run on a fresh project `kettle-control`. The human-action
   count for a run is `person_actions` from `bee sessions measure` plus the
   operator's written list of acts that sign nothing (ticks, relaunches).

**A5 — Astra's adversarial review of Wave 2, 2026-09-21**
([review](history/2026-09-21-astra-wave2-review.md)). Twelve findings, all
accepted; seven block the control run. Each went back to the lane that built
the code, reproduced as a failing test before its fix; the contract lane
writes new fixtures first (the implementer never writes its own oracle).
1. **Lane 211, approval truth** (findings 1, 9, 10, 11): what the card shows
   is what is approved or Approve is unavailable — definition resolved by the
   run's bound hash, argv displayed unambiguously, Deny always available; the
   approver is the project's creator or a roster Owner, never the publisher;
   the run's pre-approval bound commit is carried (absent / null / sha kept
   distinct); rendering never writes.
2. **Lane 212, preparation fences** (2, 3, 4, 12): establish and verify at the
   actor's idle boundary with custody held through the turn; a live slow
   checkout is not an interrupted attempt (attempt identity, conditional
   terminal writes); a deferred wake is durable, clamps the watermark and is
   released when establishment settles; a failed intent save stops the git
   work.
3. **Lanes 214 then 213, coverage** (5, 6, 7, 8): the completion gate fails
   closed on declaration state, not on rendered rows; evidence comes from the
   canonical team projection, never raw events; action definitions keep
   (repository, commit, name) through evaluation; the initial `workId` is
   derived from stable inputs so a lost response cannot double-declare.
4. **Lane 210 acceptance requirements**: no-ask is `approving &&
   requiredAction == null`, never prose; settled is not acknowledged and
   gates no resource; an already acknowledged chain keeps governing; fixtures
   for every non-approving and corrected case; the strict frontend reader
   ships in the same commit.
5. **Lane 215, preventive**: shared vectors for the four other closed records
   with independent Rust and TypeScript readers (44221, 44224, 44226, 44230)
   and 44223's unloaded ones; reader disagreements are pinned and reported,
   not silently fixed.
6. The control run waits on 209–215 landed and Astra's re-check of 211–213.

### A6 — two contract rulings made during lane 213 (2026-09-21; recorded late)

Ruled while 213 was building and until now recorded only in ledger 214 and `conformance/project-work/README.md`: `wrong_run_or_hash` always names the declaration's plan commit, and the README carries the table of exact reason strings; `candidateArtifact` is null while a git-ref criterion is uncovered.

### A7 — rulings on Astra's Wave 2 re-check (2026-09-21)

Review: `docs/history/2026-09-21-astra-wave2-recheck.md` at `ce6df5633`. Seven findings closed, five partial. Every counterexample is accepted; none is disputed. Lane 210 (settlement) stands.

1. **R1, lane 218 — what is shown is what is hashed.** The approval surfaces take the definition and its hash from one native read, and the hash is computed from the returned bytes by the relay's own canonical function. Two reads joined by a comparison is refused as a design, whatever the test says.
2. **R2 + R3, lane 219 — custody belongs to the turn, not to the assignment.** Every turn that runs on a seat holds that seat's custody from dequeue, requirement or not. A prepared turn refused at dequeue gets a provider-visible terminal answer that discharges the in-flight delivery. The `tree_moved` observation is written under custody and only against the attempt revision it observed.
3. **R4, lane 220 — a deferred wake is re-admitted, never re-started.** Release replays the original command through the same admission function. Expired, stale-generation and refused wakes are discharged with an observable disposition; an undecidable wake past the command horizon is expired, not held. Deferred-store writes are fallible and nothing owed is evicted silently.
4. **R5, lane 221 then 222 — empty means unproved.** Coverage requires positive canonical inclusion and a positive criterion→assignment→assignee relationship. The oracle lands first (221: both negative sequences, branch-specific reason templates, a fixture for every branch and every reason code); the fold follows it (222). The implementer does not author either.
5. **Readers, lane 223 — not a control blocker, built now.** Raw-content vectors including duplicate keys; `web/` loads the shared vectors; desktop ingress and mobile reject duplicates as core does. Until it lands nobody claims the strict readers agree.
6. **The control run waits on 218–222 landed, deployed and installed.** Astra is asked for a narrow third look at R1–R5 only. R3 is included now rather than deferred, because Wave 4's disruption run needs it and the code is already open.

### A8 — rulings on Astra's third look (2026-09-21)

Review: `docs/history/2026-09-21-astra-wave2-third-look.md` at `aa055b233`. R1, R3, R5 closed; readers no blocker. R2 and R4 partially closed; both counterexamples accepted. The R2 hole was introduced by lane 219's own unplanned fix for a restart deadlock (`supersede_seat`), which is the case the review was asked to attack.

1. **R2, lane 227 (built as lane 224) — one custody identity per seat, for the life of the provider.** The seat's lock is never replaced. A restart deadlock is cured by ending the holder, not by minting a second lock: quiescence of the predecessor is proven (actor task joined, agent child exited) and never inferred from the successor's existence; a live establishment is respected until it ends, and the blocking Git helper gets a process timeout so it does end. A successor that cannot obtain custody answers `seat_busy`; it does not run.
2. **R4, lane 228 (built as lane 225) — the answer is staged before the door is shut.** Every refusal reached through admission (stale generation, authority, verification input, conflicting command) stages its terminal decision and signed answer through the existing durable terminal mechanism before the refusal is recorded; a retry that finds a refusal with an unpublished answer publishes that original answer. An unreadable deferred store is not an empty one: the file is kept, deferral refuses with `VERIFICATION_INPUT_UNHELD`, and the condition is visible. The test named for the provider's failure branch must exercise that branch.
3. **The control run waits on 227–228 landed, deployed, installed, and a fourth look limited to those two.** The goal-membership waiver at `project_work_fold.rs:419` stays as documented; it is recorded as an observation, not a lane.
4. **Lanes 224/225 were renumbered 227/228 on landing:** `origin/main` had claimed 224–226 (Andy's agents-repository batch) before ours was pushed, and ledger numbers are frozen once pushed.

### A9 — rulings on Astra's fourth look (2026-09-21)

Review: `docs/history/2026-09-21-astra-wave2-fourth-look.md` at `d209432aa`. All five counterexamples accepted; two were executed. Three rounds have each closed the named case and opened its neighbour, so this round states the invariants rather than the cases, and the lane proves the invariants.

1. **Seat availability invariant (lane 229).** A seat may be acquired only when, under its one mutex, the fence is clear; the fence is checked *after* acquisition, never only before waiting. The fence clears only when every process group ever registered against the seat is proven gone (`killpg` then poll to `ESRCH`, bounded). Every process the seat spawns — the agent child and every Git invocation — runs in its own process group, and that group id is recorded in the seat's durable custody record before the process is used. Retirement is one routine used on clean exit, timeout, panic and provider Drop; a task is joined, never inferred complete. Where a proof cannot be awaited (Drop), the seat is fenced durably with the reason, and the next provider start attempts the proof from the recorded group ids before unfencing. Pipes are drained concurrently with waiting. A fenced seat says why and that it will not clear by itself.
2. **Answer custody invariant (lane 230).** Responsibility for a wake moves in one direction: the held record is released only after the signed answer is durably staged; a projection rejected or timed out by the outbox is never retired as delivered — the intent stays, parked durably and visibly, until a recovery publication (fresh signature, same command id) or an operator disposes of it.
3. **In-house refutation before landing.** Astra's executed probes (`/tmp/astra-fourth-fence-probe.rs`, `/tmp/astra-fourth-git-probe.rs`) become tests in the lane. A separate refuter agent, briefed with A9 and the four reviews, attacks the lane before the finalizer; its findings are fixed in the lane. The control run waits on 229–230 landed and Astra's fifth look at those two.

### A10 — plan drift is disclosed, never enforced (2026-09-21)

Lane 233 tested the claim that a plan committed out from under a live declaration makes it stale and blocks completion. The claim is false: `Stale` means `goal_changed` only; the fold reads the plan at the pinned commit; completion, `status` and the desktop surface never name the plan's current tip (ledger 233). The pinning itself is correct and stands — a declaration's contract never changes underneath a team. What is missing is disclosure.

1. **A new fold fact, `planDrift`** (lane 234 contract, lane 235 implementation): for each declaration, the agents repo's `main` tip as the assembler already receives it in relay ref state (kind 30618 names branch tips, not paths — so drift means "main moved since the declaration", and no surface may claim the plan *file* changed; lane 234 corrected this wording), compared with the declared commit; `null` when the ref state is absent (unknown is disclosed as unknown, never as "no drift"). The oracle lands first: fixtures for no-drift, drift, unknown, and superseded-then-current; every strict reader of the fold output loads them.
2. **Surfaces:** `bee sessions work status` and the desktop work row print declared vs current commit when they differ, with the re-adopt command; the completion result carries the same fact. Completion is not refused for drift. The lead's work brief line is a follow-on after lane 229 lands (provider crate ownership).
3. The note to Andy of 2026-09-21 overstated this ("our completion gate refuses a stale declaration"); a correction is owed and drafted.

### A11 — the control run is a measurement, not a safety claim (2026-09-21)

Four review rounds on R2/R4 (211→218→227→229) each closed the named case and opened its neighbour, and each paid for a landing Astra did not need to review. The "control run waits on …" rulings in A5.6, A7.6, A8.3 and A9.3 were Fable's and are lifted **for the control run only**. The disruption run (Wave 4) still waits on R2/R4 closed.

1. `kettle-control` runs on the installed build (`d209432aa` plus lanes 231/232 landed tonight). Its report lists R2/R4 as known-open and makes no safety claim; a known-open defect that fires ends the run — stop, record, no repairs — and that is a result, not a wasted run.
2. Review before landing: the refuter and Astra review branches; custody (229/230) and drift (234/235) land only after they pass, in one finalizer pass.
3. Two baselines: this run, and one after Wave 4, show the custody work's effect.

### A12 — what the control run taught (2026-09-22)

Run record: `docs/history/2026-09-22-kettle-control-runbook.md`; findings ledger 236(a)–(h); fixes lanes 237–240.

1. **A closed record is not landed until one live publish against hive has been accepted.** Lane 201's work records were landed on stub-wire and fixture evidence; the first live `work adopt` on record was refused (236(f): the NIP-OA `auth` tag decorates every seat-signed event). Every lane that adds or changes a writer of a closed kind ends with one publish against hive from a seated key, its event id in the ledger item.
2. **Strict readers are enumerated, not assumed.** Lanes 216 and 223 said "all readers agree"; the desktop had a fifth strict decoder that loaded no shared vector (236(b), lane 238). A conformance suite's README lists every decoder in the tree by file path, and the checker fails when a decoder is added without a loader. "All readers" means the list.
3. **Anything a seat waits on wakes it by mechanism.** Decision answers already did; host results did not (236(g), lane 240). The rule is general: an approval, a host result, a verdict, a hire's readiness — each has a wake, recorded by receipt, and no role text may ask an agent to poll or a person to relay.
4. **Prose that governs seats lives in the template catalog.** The "stop and ask" rule that cost 49 minutes lived in `buzz-acp/src/base_prompt.md`, outside every template version (lane 239). Base prompts are versioned and hash-pinned like templates, or their sentences move into `working-contract`.
5. **The next run is the same run.** `kettle-control` again on the landed batch, same plan, same measurement; the number to beat is 12 minutes of team work and zero hidden asks. Astra's read of this run: `docs/history/2026-09-22-astra-control-run-read.md` when filed.

