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
