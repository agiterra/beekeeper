> Written by Astra (GPT-6), 2026-09-21, read-only adversarial review of Wave 2 at `220a29195`; copied verbatim by Fable from /tmp/astra-wave2-review.md. Rulings: [UNIFIED_WORK_PLAN.md](../UNIFIED_WORK_PLAN.md) § 8 A5.

# Wave 2 adversarial review — 2026-09-21

**Verdict: hold the control run.** Preparation is not fenced at the execution boundary, work coverage can accept the wrong evidence or bypass its completion gate, and the approval card does not reliably show what the owner is authorizing. These are concrete source-level counterexamples, not reproduced live incidents.

Reviewed HEAD and origin/main `220a29195e68c2bc9a1865c3ba68453a11d7b1b4`, the governing plan including A1–A4, and ledger 201–207. **Item 208 is absent from this checkout's ledger**; I cannot verify its claimed fix here. Paths below are repository-relative. No repository edits, builds, pushes, or live session operations were performed.

## Findings ranked by severity

### 1. P1 — Approval displays a command from a different definition

**Blocks control.** `desktop/src/features/project-actions/ui/HostStepApprovalInboxCard.tsx:54` fetches the run and current workflow separately: `:84` shows the run's bound hash, while `:86` takes the command from the current definition without comparing hashes. `ProjectActionRunRow.tsx:81` repeats it.
**Failure:** A waits for approval; B is published; the card shows hash A and command B; A is republished before the click. Relay and host correctly execute A, but the owner saw B. Even a failed definition read leaves Approve enabled: `ProjectActionApprovalCard.tsx:162` gates only on identity, and `:166` disables only during mutation. Require a resolved definition matching the run hash before granting, with an unambiguous argv display; `lib/actionDefinition.ts:149` currently joins arguments with spaces. Keep Deny available when facts cannot be established.

### 2. P1 — Preparing the next assignment can change a running seat's tree

**Blocks control.** `crates/buzz-session-provider/src/lib.rs:4671` performs preparation and the tree check before delivering to the actor's mailbox at `:5002`. Preparation executes `git checkout -B` (`crates/buzz-session-provider/src/assignment_inputs.rs:1002`). There is no wait here for the previous turn to end. Actor dequeue checks its refusal fence, then runs the prompt without repeating the tree check (`crates/buzz-session-provider/src/session.rs:2475`, `:2495`).
**Failure:** verifier is testing A in a clean tree; an assignment for B arrives. The provider switches to B while A's turn still runs, then queues B. A second queued assignment C can switch again; B later starts on C although B passed its earlier check. Establish/check at an actor-controlled idle boundary and retain checkout custody through that turn. Test two assignments queued behind a running verifier, not only an idle-seat preparation call.

### 3. P1 — A live slow checkout is mistaken for an interrupted attempt

**Blocks control.** `crates/buzz-session-provider/src/lib.rs:8101` times out a `spawn_blocking` join after 120 seconds; that Git task continues. `assignment_inputs.rs:202` treats `establishing` as pending, and `:1356` starts another attempt for any pending record without distinguishing a live owner from a crashed one. Terminal writes at `:1395` do not compare attempt identities.
**Failure:** fetch A exceeds 120 seconds; a replay starts attempt 2 while attempt 1 still runs; another replay can mark it abandoned while either task can subsequently overwrite that terminal outcome. A different assignment can meanwhile move the same tree, followed by the detached old task moving it back. The JSON lock serializes record writes, not these Git effects. Add per-seat live-attempt ownership, restart reconciliation, and conditional terminal writes; test timeout plus redelivery and overlapping assignments with a barrier-controlled fetch.

### 4. P1 — Deferred assignment wakes have no durable retry owner

**Blocks control.** Deferred preparation returns `TurnDisposition::Undecided` without retaining the command (`crates/buzz-session-provider/src/lib.rs:4696`). The live event path merely returns without advancing its watermark (`:1788`). No task-completion callback re-delivers it. Worse, replay delivery handles only `Err`, not `Ok(Undecided)`, then advances the watermark (`:1683`, `:1707`); the watermark ceiling contains only in-flight and replay-held commands (`:6065`).
**Failure:** a normal CLI assignment wake times out; fetch finishes at 130 seconds; an otherwise healthy connection never replays that wake. A newer channel event can advance the watermark past it, defeating reconnect recovery too. Preserve the exact deferred command durably, clamp the watermark to it, and schedule its release when establishment settles. Test success after the timeout with no reconnect or second human/lead command. The separate team-report wake queue does not retain this ordinary 44220 delivery.

### 5. P1 — The completion gate fails open for unreadable plans and conflicts

**Blocks control.** `crates/buzz-cli/src/commands/sessions/operations_completion.rs:271` selects incomplete declarations only when `criteria` is nonempty; `:225` otherwise permits completion. An unresolved plan before any bindings yields no criterion rows (`crates/buzz-core/src/project_work_fold_project.rs:216`); conflicted declarations deliberately carry none (`:114`).
**Failure:** adopt, then complete without `--agents-repo` before binding evidence: coverage is unknown/incomplete, yet no explicit `--without-coverage` is needed. Two conflicted heads likewise escape. Gate on declaration state, plan resolution, conflicts and `coverage_complete`, not the availability of rendered criterion rows. Preserve the intentional legacy/no-declaration and explicit-override behavior separately.

### 6. P1 — Work coverage accepts reports excluded by the team contract

**Blocks control.** `crates/buzz-core/src/project_work_inputs.rs:370` extracts raw reports/dispositions without canonical 44244 admission. `project_work_fold_project.rs:468` ignores the report's signer and assignment reference and checks its SHA. In contrast, the team fold requires report signer == assignee (`coding_session_team_transaction_fold.rs:739`); relay ingest validates structure, not that authorship relationship (`crates/buzz-relay/src/handlers/ingest.rs:4164`).
**Failure:** B owns an assignment; channel peer C publishes a well-formed report claiming its SHA; the lead mistakenly approves and binds it. The team fold excludes the report/disposition chain, but work coverage accepts it. Raw historical approvals also remain available after replacement. Derive report and disposition facts from the existing canonical team projection inside the sole assembler; add wrong-assignee and superseded-disposition fixtures.

### 7. P1 — Expected action definitions lose the plan commit they belong to

**Blocks trustworthy control evidence; also blocks disruption.** `crates/buzz-core/src/project_work_inputs.rs:268` collapses `(repository, commit, action name)` to name alone. CLI loads definitions for every declaration, including superseded ones (`crates/buzz-cli/src/commands/sessions/work.rs:1508`); action evaluation looks up only the name (`project_work_fold_project.rs:514`).
**Failure:** old plan at `ff…` defines verify hash A; amended head at `aa…` defines hash B. Map collection retains the lexicographically later old definition for both. Correct B evidence fails; old A evidence re-bound under the new declaration can pass. Keep repository/commit/name provenance through evaluation. Test two declarations with the same action name and different hashes, in both commit-sort orders. This is shared by desktop, not a CLI-only issue.

### 8. P2 — Initial adoption retries create another work identity

**Blocks the promised retry-safe control flow.** `crates/buzz-cli/src/commands/sessions/work.rs:918` creates a fresh UUID for each initial adoption; `:1000` requires the same UUID to deduplicate. `:870` disallows supplying a stable initial `--work-id` without supersedes.
**Failure:** publication succeeds but its response is lost; the same command creates a second declaration under a second workId, leaving duplicate obligations. The retry test uses an amendment with an explicit UUID (`work_tests.rs:581`), so misses this. Provide a stable initial operation identity, and test response loss after the first accepted publish. The existing dedupe also ignores goal, decision and responsible actor despite calling itself exact-body matching (`work.rs:995`).

### 9. P2 — Delegated publication gives the inbox the wrong approver

**Blocks the intended owner-inbox approval flow; Actions offers a creator-only workaround.** Publication records the publisher as workflow owner (`crates/buzz-relay/src/handlers/command_executor.rs:998`); the approval's `p` tag names that owner (`crates/buzz-workflow/src/suspend.rs:161`; `crates/buzz-relay/src/workflow_sink.rs:225`). Inbox treats it as project approval authority (`desktop/src/features/project-actions/ui/HostStepApprovalInboxCard.tsx:92`). Actual admission is project creator **or roster Owner** (`command_executor.rs:1334`).
**Failure:** lead publishes through its delegation. Brian's inbox card withholds Approve; the lead's card offers a grant the relay refuses. Actions uses the project creator instead (`ProjectActionsScreen.tsx:35`), still excluding legitimate coowners. Resolve authority from the project named by `approverSpec`, including current roster ownership; never infer it from the action publisher.

### 10. P2 — Desktop drops the pre-approval bound commit added by 206

**Blocks approval-card acceptance.** Relay now emits `checkout` (`crates/buzz-relay/src/api/workflows.rs:428`), but desktop's raw type and mapper omit it (`desktop/src/shared/api/tauriWorkflows.ts:45`, `:232`). The two cards still seek it only in a later host result (`HostStepApprovalInboxCard.tsx:78`; `ProjectActionRunRow.tsx:66`).
**Failure:** a checkout-required run is awaiting its first approval; the card says no record names the commit even though the run API supplies it. That host result cannot exist yet. Carry the run checkout through, preserving absent versus null versus a bound SHA; test an awaiting-approval run, not a completed one.

### 11. P2 — Viewing Actions starts a write, including forced cache checkout/clean

**Does not independently block a fault-free control run; violates the requested render boundary.** `desktop/src/features/project-actions/ui/ProjectActionsScreen.tsx:26` mounts `useRecordProjectAgentsRepo`; its effect invokes recording (`lib/useRecordProjectAgentsRepo.ts:18`). The native call synchronizes then writes the workdir store (`desktop/src-tauri/src/managed_agents/agents_repo.rs:529`); synchronization uses `checkout --detach --force` and `clean -x -d --force` (`managed_agents/packs_cache.rs:543`).
**Failure:** merely opening the tab, including as a nonowner, mutates the managed pack cache and execution configuration. This is a managed cache, **not a seat worktree**. Move establishment to an explicit preparation action or already-authorized host operation; rendering should read its result.

### 12. P2 — Failed intent persistence still permits checkout

**Blocks the claimed crash-recovery guarantee; not alone a fault-free control blocker.** `crates/buzz-session-provider/src/assignment_inputs.rs:1299` logs and swallows every save error; `:1314` returns success. `:1367` relies on that persistence before starting Git.
**Failure:** disk-full or rename failure prevents the started-attempt record from reaching disk, yet the checkout proceeds; a crash loses the attempt count, defeating the bounded replay guarantee. Return persistence errors before side effects and distinguish failures recording an already-completed effect. Test failed pre-checkout persistence and failed terminal persistence separately.

## Lane 204: remaining cross-reader exposure

**Preventive P2, not a present control blocker:** these closed records have independent strict readers, but I found no complete shared vectors loaded by both languages. An additive key accepted by one reader and rejected by the other reproduces 204's failure class.

| Record | Rust reader | Independent desktop reader |
|---|---|---|
| 44221 lifecycle command | `crates/buzz-core/src/coding_session_lifecycle_command.rs:104`, `:360` | `desktop/src/shared/coordination/sessionCoordinationStrictJson.ts:105` |
| 44224 lifecycle receipt | `crates/buzz-core/src/coding_session_payload.rs:752` | `desktop/src/shared/coordination/sessionCoordinationStrictJson.ts:222` |
| 44226 genesis | `crates/buzz-core/src/coding_session_genesis.rs:128` | `desktop/src/features/coding-sessions/lib/codingSessionCreateObservations.ts:359` |
| 44230 closure | `crates/buzz-core/src/coding_session_closure.rs:49` | `desktop/src/shared/coordination/sessionCoordinationStrictJson.ts:855` |

44223 has partial shared pack-source vectors on the Rust side (`coding_session_payload.rs:3407`), but its TypeScript parity tests compare local readers rather than loading those vectors (`sessionCoordinationStrictJsonParity.test.mjs:1`). Do not call it wholly untested. 44244 already has common schema fixtures across core, SDK and desktop (`coding_session_team_transaction_tests.rs:446`; SDK `coding_session_team_transaction.rs:98`; `codingSessionMissionTransactionWire.test.mjs:46`). The next additive change must name and exercise every strict reader, including native-to-frontend projection decoders.

## A4: accept the direction, constrain what settlement means

**Lane 210 is not reviewed as landed code. These are acceptance requirements before control, not claims about its unfinished implementation.** An ACK legitimately proves that the **assignee signed receipt of the exact disposition**, plus an optional note (`crates/buzz-core/src/coding_session_team_transaction.rs:358`; assignee authorization in `coding_session_team_transaction_fold.rs:746`). It never proved implementation correctness or that an ask was performed. Removing a no-ask ACK sacrifices witnessed delivery and a possible concern, which need not block acceptance.

- Define no-ask mechanically: approving decision and `requiredAction == null` (`coding_session_team_transaction.rs:295`, `:343`, `:847`). **Failure:** lead says “approved; please push” in summary but leaves requiredAction null; settlement loses the obligation. Teach roles/CLI to put required work in that field or a new assignment; do not infer it from prose.
- Auto-settled must not mean **acknowledged**, delivered, idle, or safe to delete the worktree. A worker may still be running or have an objection. Retain asynchronous receipt/objection handling and existing execution/resource gates; never synthesize its signature.
- Pin historical selection: current settlement chooses among qualifying report/disposition/ACK chains (`coding_session_team_transaction_fold_settlement.rs:178`). A newer unacknowledged no-ask approval can become the winner under A4 and change an already-settled assignment's governed report. Test what “nothing already settled changes” preserves: boolean settlement alone is weaker than preserving its evidence chain.
- Require fixtures for nonnull requiredAction, all nonapproving decisions, excluded/missing reports, unauthorized or superseded dispositions, report corrections and old ACK replay. Include the strict frontend projection reader: `desktop/src/features/coding-sessions/lib/invokeCodingSessionTeamFold.ts:278` currently allows exactly six assignment fields; adding the promised settlement-rule field without updating it repeats 204.

## Checked and found sound; verification limits

- The two processes use the same OS lock implementation (`desktop/src-tauri/src/coding_sessions/workdir_store_lock.rs:15`); provider preserves foreign JSON keys and atomically renames store writes (`assignment_inputs.rs:1295`, `:1322`). The defect is effect ownership and error handling, not two incompatible locks.
- Stable dirty trees are refused before writes (`assignment_inputs.rs:909`). Missing/malformed pointers do not invent a path (`:1139`); provider falls back to its actual tree fence (`lib.rs:8032`, `:7816`, `:7841`). A refused establishment returns Refused rather than opening a turn (`:7811`). These protect ordinary sequential starts; they do not protect the later boundary or a concurrently moving tree. Snapshot intent discovery picks a seat by umbrella alone (`:7984`), so its recorded path may describe another seat; actual preparation uses the target's cwd, preventing that display error from directly selecting Git's target.
- Adoption resolves the committed plan, actions and goal before its **single** publication (`crates/buzz-cli/src/commands/sessions/work.rs:884`); reads use `git show commit:path` (`:389`) and the publication compiler (`:482`). I found no partial multi-publish adoption. I found no second production coverage assembler: CLI (`:1644`) and native desktop (`desktop/src-tauri/src/commands/project_work.rs:386`) call core; TypeScript forwards events/projection. Provider coverage assembly is still W3b work.
- Approve/Deny publish only from click handlers. Action scope reaches the native grant unchanged (`ProjectActionApprovalCard.tsx:83`; `desktop/src/shared/api/tauriWorkflows.ts:472`), and relay binds future-run permission to the approved run hash (`command_executor.rs:1497`). That authorizes the same definition on future runs, not only the displayed run/commit. Ordinary nonowners on Actions have no grant controls; the delegated-publisher/coowner exceptions are finding 9.
- 204's projectRef presence and receipt/transition binding are covered by common authority vectors in core (`coding_session_authority_transition_project_action_tests.rs:176`), CLI (`operations_receipt_tests.rs:598`), provider (`authority.rs:1396`) and desktop (`codingSessionAuthorityConformance.test.mjs:40`). This is the right corrective pattern.

This was static adversarial review, not a fresh live run or test execution. I did not reproduce slow-fetch timing, crashes, approval races, UI mounting, or the Kettle Smoke measurements against a running system. Ledger 205 is historical evidence, not proof that Wave 2 works: it explicitly used the Wave 0–1 bundle. Before control, test the concrete counterexamples above through the composed paths, especially actor dequeue, lost-response retries, canonical evidence admission and the first delegated approval.
