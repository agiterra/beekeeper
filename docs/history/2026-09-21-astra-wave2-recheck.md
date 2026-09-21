# Wave 2 re-check — 2026-09-21

**Hold. Seven original findings are closed; five are partially closed.** The remaining failures concern approval identity, seat custody and deferred delivery, and positive evidence admission. Lane 210's settlement change has no blocker established by this review. Reader agreement remains incomplete, including reproducible disagreements beyond the known web copy.

Review target: `ce6df5633`. The checkout was `064b04076`; its only difference from that target was `docs/CURRENT_STATE.md`, so the reviewed implementation and cited lines match the requested revision. I read the plan and ledger 210–216 as context, then checked implementation. The plan's §8 ends at **A5**, not A6, at this revision; A6's reason-string/candidate-artifact rulings are recorded in ledger 214 and reflected in the project-work README. That location discrepancy is not itself a blocker.

## Verdict on each original finding

| # | Original finding | Verdict | Code evidence and remaining scope |
|---|---|---|---|
| 1 | Approval command differs from bound definition | **Partially closed** | Missing/mismatched reads now disable grants and argv is structured, but body and hash still come from independent reads: `desktop/src/features/project-actions/lib/resolveBoundDefinition.ts:77`, `:89`, `:53`. R1 below remains a blocker. |
| 2 | Preparation moves a running seat's tree | **Partially closed** | Guarded assignment turns hold custody through `run_turn` (`crates/buzz-session-provider/src/session.rs:2520`), but ordinary turns take no guard (`:2526`; `assignment_custody.rs:408`). The preparation-to-dequeue handoff also remains fallible without a terminal answer. R2. |
| 3 | Slow live checkout replayed / stale terminal writer | **Partially closed** | Live attempts are joined (`assignment_custody.rs:209`); ordinary terminal commits compare owners (`assignment_inputs.rs:1557`). The dequeue-refusal writer bypasses that comparison and can overwrite a new attempt (`assignment_custody.rs:435`, `:443`, `:459`). R3. |
| 4 | Deferred wake has no durable retry owner | **Partially closed** | Tick release and file-backed watermark exist (`lib.rs:585`, `:6143`), but release fabricates Start rather than rerunning admission (`:8210`). Expiry/authority/generation checks are skipped; an undecidable oldest wake can remain indefinitely (`:8228`). R4. |
| 5 | Completion bypass on empty criteria | **Closed** | `crates/buzz-cli/src/commands/sessions/operations_completion.rs:283` gates incomplete head/stale/conflict declarations independently of rows; read errors propagate at `:222`. The explicit override remains separate at `:228`. |
| 6 | Excluded reports accepted as coverage | **Partially closed** | The canonical team fold is called (`crates/buzz-core/src/project_work_inputs.rs:438`), but empty-projection and empty-assignment exceptions waive its requirements (`project_work_fold_project.rs:525`, `:594`, `:602`). R5. |
| 7 | Action definition loses plan provenance | **Closed** | Assembly retains repository/commit/name (`project_work_inputs.rs:296`); evaluation looks up the declaration's complete key (`project_work_fold_project.rs:664`). Both commit-sort-order fixtures are loaded (`project_work_fold_tests.rs:67`). |
| 8 | Initial lost-response retry double-declares | **Closed** | Stable initial identity at `crates/buzz-cli/src/commands/sessions/work.rs:123`; existing declaration reused at `:986`; whole-body comparison at `:1061`; sequential lost-response test at `work_tests.rs:1001`. Concurrent initial publications can still fork, which the chosen design explicitly represents; I am not reopening that design. |
| 9 | Publisher mistaken for approver | **Closed** | Creator/roster Owner checks at `desktop/src/features/project-actions/lib/hostStepApproval.ts:383`, `:396` match relay `crates/buzz-relay/src/handlers/command_executor.rs:1334`, `:1344`. Both cards use the shared authority hook; publisher is not authority. |
| 10 | Pre-approval checkout dropped | **Closed** | `desktop/src/shared/api/tauriWorkflows.ts:167` preserves absent/null/SHA and `:268` maps it; inbox reads run checkout at `HostStepApprovalInboxCard.tsx:90`, Actions at `ProjectActionRunRow.tsx:93`. Unknown checkout blocks granting (`hostStepApproval.ts:300`). |
| 11 | Actions render writes | **Closed** | `useProjectAgentsRepo.ts:110` reads; `:115` defines a separate mutation, invoked by the explicit Prepare click (`ProjectActionsScreen.tsx:83`). Native status reads the recorded store at `desktop/src-tauri/src/commands/project_work.rs:446`; no synchronization remains on this render path. |
| 12 | Failed intent save permits checkout | **Closed** | `assignment_inputs.rs:1509` must persist before returning Started; errors return Unstarted at `:1514`/`:1519`, before Git at `:1526`. Post-effect write failure is distinguished at `:1572`; retry writes the record through its owner check (`assignment_custody.rs:347`). This closes the original store bug; the new deferred-wake store still has separate error-handling weaknesses noted under R4. |

## Remaining counterexamples

### R1 — P1: the new hash comparison authenticates a different HTTP response

`resolveBoundDefinition.ts:77` independently fetches the workflow body and autorun state; `:89` borrows the latter's hash. `:53` compares that hash to the run hash, then `:62` renders the separately fetched body. Actions does the same through `useProjectActions.ts:136`, `:160` and `ProjectActionRunRow.tsx:84`.

**Interleaving:** run A waits; B is current; body read returns B; A is republished; autorun read returns hash A. The comparison passes, command B is shown, and Approve is enabled (`hostStepApproval.ts:323`; `ProjectActionApprovalCard.tsx:229`). The relay correctly grants A and the host executes A. Both once and future-run grants are affected.

**Minimum correction:** return definition and hash from one workflow-row read, or hash the actual fetched body with the canonical compiler. Test a publication between those reads. The present test injects the hash/body pair independently (`approvalTruth.test.mjs:288`), assuming the missing association. The corrected argv display, disabled unread states and Deny availability are sound; they do not establish this association. **Blocks control.**

### R2 — P1: custody is conditional on the prompt having an assignment requirement

Non-pointer turns return Open before registering any requirement (`crates/buzz-session-provider/src/lib.rs:7897`). `assignment_custody.rs:408` then returns NotRequired **before** taking the seat lock; `session.rs:2526` runs with no guard.

**Interleaving:** an ordinary follow-up to a verifier is examining clean commit A. Assignment B arrives. `establish_for_turn` sees the seat as idle (`assignment_custody.rs:224`), acquires custody at `:261`, and moves its tree while that turn is running. The same omission is possible through unmanaged preparation paths (`lib.rs:8292`, `:8298`). Checkout custody must protect every running turn that uses the seat, even if no assignment-specific SHA check is required.

A second hole remains even for guarded turns: preparation releases its guard before delivering the prompt (`assignment_custody.rs:275`). If C establishes after B was prepared but before B acquires its actor guard, B's dequeue check refuses it. That protects against testing C as B, but `session.rs:2533` merely logs and `:2543` continues: it emits neither a refusal nor a terminal event to discharge the provider's in-flight delivery. B can remain accepted-but-never-started. Transfer custody/reservation coherently to dequeue, or durably requeue/refuse this case with a provider-visible answer.

The advertised two-assignment test manually acquires a token for the running turn (`assignment_custody_tests.rs:139`); it does not prove all real actor paths acquire it. **Blocks control.**

### R3 — P2: a non-owner dequeue writer can destroy a newer attempt's ownership

The ordinary `commit_terminal_record` comparison is sound. However, `verify_for_turn` drops custody at `assignment_custody.rs:435`, then reloads the current row at `:443` and unconditionally writes `tree_moved`, clearing `attempt_owner` at `:459`.

**Interleaving:** dequeue detects a mismatch; pause after guard release; the operator requeues that assignment (`desktop/src-tauri/src/coding_sessions/assignment_establishment.rs:264`; shared reset at `assignment_inputs.rs:542`); a new attempt records owner O; the old dequeue writer overwrites O with its stale observation. O's legitimate terminal write then fails its ownership comparison at `assignment_inputs.rs:1557`. This defeats the new fence without breaking the fence function itself.

Keep the observation update under appropriate custody and condition it on the record/attempt revision it observed. **Required to close finding 3 and before testing requeue/recovery; not independently necessary for a fault-free control that never requeues.**

### R4 — P1: deferred delivery bypasses admission and can hold its floor indefinitely

`release_deferred_turns` constructs `TurnDecision::Start` directly (`lib.rs:8210`) and calls `apply_turn_decision`, bypassing the consumed/in-flight, horizon, generation and authority checks in the normal decision path (`commands.rs:911`, `:932`, `:953`, `:963`, `:1007`).

**Safety case:** a generation-N wake is deferred; that session resumes as N+1; release retries the old target. `verification_input_outcome` deliberately returns Open for a stale target, assuming ordinary admission rejected it (`lib.rs:7903`). Delivery selects the current actor by session ID (`:4959`, `:5075`), so the old wake can reach the new generation. Revoked sender authority is likewise not rechecked by this release path.

**Liveness case:** after initial deferral, a required relay input stays unavailable. Each tick returns Undecided, the record remains (`:8228`), and its age is never compared with the command horizon. It indefinitely clamps the file-backed floor (`deferred_turns.rs:221`) and, because only one wake per seat is attempted, prevents later held work for that seat being considered (`lib.rs:8202`).

Replay through the same admission function as the original command, preserving identity; discharge expired/stale/refused work with an observable disposition. Also make deferred-store writes fallible: `deferred_turns.rs:170` currently logs a failed save but updates the mirror, contrary to its durable-custody claim. Do not silently evict old owed wakes at capacity (`:167`). **The admission bypass and missing terminal policy block control.**

### R5 — P1: empty canonical facts waive the coverage proof

The shared assembler calls the real team fold, but retains raw report/disposition facts (`project_work_inputs.rs:485`). The review evaluator checks canonical disposition inclusion only when the included set is nonempty (`project_work_fold_project.rs:525`), applies the same exception to reports (`:594`), and checks assignment binding only when bindings already exist (`:602`).

**Counterexample 1:** report and lead approval refer to an absent assignment. The canonical fold excludes both, leaving no included events; raw facts remain. Bind them as work evidence: both inclusion checks disappear and matching lead/SHA can reach Covered at `:558`.
**Counterexample 2:** use a valid report/approval for assignment X, but bind its evidence to criterion Y without any assignment binding for Y. The empty-list exception waives the required relationship and covers Y.

Require positive canonical inclusion and a positive criterion-to-assignment-to-assignee relationship. Empty means unproved, never permission to omit the predicate. Add independently authored negative sequences for both cases. **Blocks control.**

## Are the 12 reasons and 18 sequences an adequate oracle?

**Independent, but insufficient.** Expected folds are loaded rather than generated by the implementation (`project_work_fold_tests.rs:137`), and repository/commit action lookup is now correctly fixed. But the two R5 cases are absent; the implementer supplied permissive semantics where the oracle supplied no negative example. Those semantics contradict the unconditional canonical-evidence requirement.

The “every reason string” test only checks reasons produced by those sequences (`project_work_fold_tests.rs:613`) and explicitly permits four unexercised codes (`:635`). Concrete remaining A6 violations: `wrong_run_or_hash` omits the declaration commit on wrong-echo-signer (`project_work_fold_project.rs:698`), wrong-step (`:726`) and no-host-result (`:777`) paths. The dirty-after diagnostic at `:765` also differs from the table's dirty-before template. These diagnostic mismatches are not additional false-success paths, but the test name overstates coverage. Define valid branch-specific templates and fixture every branch; do not make code infer its oracle or force distinct failures into misleading identical prose.

The scoped lookup and CLI/native/provider reuse of the same assembler are sound (`project_work_inputs.rs:296`; `desktop/src-tauri/src/commands/project_work.rs:399`; `crates/buzz-session-provider/src/work_brief_collect.rs:487`). I found no second TypeScript assembler.

## Lane 210: no blocker found in the settlement rule

The helper treats trimmed blank as no ask (`coding_session_team_transaction_fold_settlement.rs:82`), but a **signed** disposition with blank `requiredAction` is rejected before settlement (`coding_session_team_transaction.rs:849`; `coding_session_team_transaction_validators.rs:137`). Thus blank is not a wire-level escape; null is the reachable no-ask form.

Canonical admission precedes settlement (`coding_session_team_transaction_fold.rs:435`). Inactive causal parents and replaced records are removed by `coding_session_team_transaction_fold_projection.rs:29`, `:68`, `:169`; settlement then requires an approving disposition and active report (`…fold_settlement.rs:297`, `:304`). A changes-requested chain does not become approving merely because its ask is blank/null. Acknowledged chains retain priority (`:366`) as ruled. I found no path through this rule that resurrects an excluded approval.

`SeatWorktreeFacts.session_settled` remains a session closure/deletion fact, not assignment acceptance (`worktree_lifecycle.rs:63`; CLI `commands/sessions/worktree.rs:363`, `:569`; desktop `coding_sessions/worktree_close.rs:314`). A live execution independently blocks pruning (`worktree_lifecycle.rs:166`). No-ask settlement therefore does not newly authorize deleting a working seat. Shared settlement projection decoding passed the targeted desktop conformance check.

## Lanes 215/216: reader agreement is only partially closed

The known web copy still disagrees, and the disagreement is executable, not inferred. I imported its decoders with the existing TypeScript loader and fed the shared vectors: **10 metadata and 5 receipt disagreements**. Valid metadata carrying beeStamp/packRef/composeRef/handover is rejected by its old optional-key set (`web/src/features/coding-sessions/domain/ingressPayloads.ts:453`); capabilities with promptImage fail its six-key shape (`:603`). It accepts invalid routing:null (`:523`). Its receipt status set omits turn_injected, turn_delivery_unknown and continuation_registered (`:142`), and its error bounds are too wide (`:39`, `:398`). The real trust reader drops rejected metadata (`web/src/features/coding-sessions/domain/trust.ts:235`).

**Additional gap:** desktop ingress still accepts duplicate JSON keys that core and the desktop coordination gate reject. Reproduced with a shared valid metadata vector containing `"status":"failed","status":"running"`; ingress accepted, gate rejected. A duplicate receipt status has the same disagreement. Desktop `codingSessionWireDecode.ts:14` uses JSON.parse, losing duplicates before validation; core parses original typed input (`coding_session_payload.rs:775`, `:1813`), and the coordination gate rejects duplicates (`sessionCoordinationStrictJson.ts:228`). Mobile's `coding_session_wire.dart:13` similarly uses jsonDecode; its duplicate behavior was inspected, not executed in this review.

The shared vectors cannot expose that class: they store parsed content objects and readers serialize them again (`crates/buzz-core/tests/coding_session_record_conformance.rs:72`; `desktop/src/shared/coordination/codingSessionRecordConformance.test.mjs:91`). Add raw-content vectors, including duplicate keys, and load the web reader too. Shared Rust consumers improve CLI/provider/relay consistency; passing parsed-object vectors does not establish byte-level agreement with independent readers. Relay ingress deliberately applies scope/membership and content caps rather than strict envelope validation to these provider records (`crates/buzz-relay/src/handlers/ingest.rs:815`, `:873`); do not assume malformed duplicate-key records are relay-inaccessible. **These reader defects do not independently block an ordinary valid-provider desktop control; they do block any claim that all strict readers now agree.**

## Verification and decision

Executed targeted desktop record/genesis/settlement conformance tests: **12 passed**. Executed the read-only web-vector and duplicate-key probes described above. No Rust/mobile suite, live approval, actor race, restart or control session was executed; R1–R5 timing scenarios are source-derived counterexamples. Only this requested report was written. Code fixes, pushes and session messages were not performed.

Minimum blocking set: **R1** bind the displayed body to its hash; **R2** protect all running turns and resolve failed dequeue custody; **R4** re-admit deferred commands and terminate stale/expired holds; **R5** require positive canonical coverage proof. R3 must also close before the requeue/disruption proof; reader parity and reason-template completeness remain explicit follow-ups.

Hold
