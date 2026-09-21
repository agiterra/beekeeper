# Wave 2 third look — R1–R5

**Hold. R1, R3 and R5 are closed; R2 and R4 are partially closed.** Two blocking counterexamples remain: actor startup replaces custody that can still be held by live work, and re-admission can durably refuse a wake while losing its visible answer.

Target: `aa055b233`. Reviewed checkout `e10f379144` differs only in `docs/CURRENT_STATE.md`; implementation citations therefore match the requested revision. A7 governs this review. Ledger entries were context, not proof. Only this report was added to the repository; no fixes, pushes or session messages.

## R2 / 219 — partially closed; P1 blocker: startup creates a second custody lock

The ordinary-turn hole is fixed: `crates/buzz-session-provider/src/assignment_custody.rs:464` takes custody **before** looking for an assignment requirement; even Unmanaged returns that guard at `:466`. The real actor acquires it at `session.rs:2557`, retains it at `:2590`, and calls `run_turn` at `:2592`. Ordinary follow-ups and unmanaged preparation therefore use the guarded path.

**The restart correction breaks that guarantee.** Every new actor calls `supersede_seat` (`session.rs:2409`), which replaces the registry's mutex unconditionally (`assignment_custody.rs:149`). Finding the previous mutex still locked merely logs a warning (`:151–156`); it does not stop the replacement or its holder.

A concrete supported interleaving, without dropping the provider:

1. An idle verifier starts preparing assignment B. The detached establishment holds the old mutex across Git (`assignment_custody.rs:291–307`). A slow fetch outlasts the preparation wait, leaving the attempt running and the wake deferred (`lib.rs:8538–8541`); the blocking Git helper has no process timeout (`assignment_inputs.rs:768–782`).
2. Restart is admitted because the session has no open model turn (`lib.rs:4480`). It asks the old actor to shut down and resumes the session (`:4495`, `:4505`).
3. The new actor replaces the mutex. An ordinary follow-up acquires the new mutex and runs while B's old establishment still owns the old mutex. B finishes fetching and checks out its commit underneath that turn.

The in-process provider-restart case is unsafe too: actor tasks are detached (`session.rs:1028`); shutdown sends a request without joining (`:1093–1096`). An old running actor can still be awaiting cancellation (`:2952–2956`, with the 30-second grace at `:64`). Cancellation sends a notification and awaits a response; it is not proof that the child has exited (`crates/buzz-acp/src/acp.rs:2467–2486`). A new actor must not infer quiescence from its own existence.

**Executed primitive counterexample:** using the current compiled provider and public custody functions, retaining the old guard gave `busy=true`; calling `supersede_seat` changed that to `busy=false`; a second guard was acquired while the first remained alive. Probe: `/tmp/astra-custody-reset-probe.rs`. The full slow-fetch/restart/model-turn interleaving is source-derived, not executed live.

**Minimum correction:** keep one serialization identity for the seat across actor generations. Establish predecessor quiescence before handing off execution, and respect any still-running establishment; replacing a locked mutex is not a handoff. Test restart during both cancellation and a preparation that exceeded the wait bound. **Blocks control under A7.2.**

The other R2 correction is sound in the checked path: a dequeue mismatch emits `TurnDropped::InputMoved` (`session.rs:2578`); the provider removes in-flight delivery (`lib.rs:9245`) and stages the exact terminal answer (`:9291`) before recording refusal (`:9296`). The corresponding real-actor test passed. This is recoverable terminal staging, unlike R4's branches below.

## R4 / 220 — partially closed; P1 blocker: re-admission can lose the refusal receipt

The original admission bypass is fixed. Release passes the original content, identity and signer to `on_turn` (`crates/buzz-session-provider/src/lib.rs:8280–8286`), which calls `commands::decide_turn` (`:4601`). That covers consumed/refused/in-flight commands (`commands.rs:911–933`), horizon (`:953`), generation (`:963`) and authority (`:995–1007`).

**Remaining failing sequence:** a held wake becomes stale-generation. Re-admission records its refusal (`lib.rs:4866`), then separately enqueues its receipt (`:4867`). Crash between those writes, or fail that outbox append. On the next release, `commands.rs:914` returns AlreadyRefused, which has no receipt (`:131`); `lib.rs:4870` returns Silent and release deletes the held record (`:8298–8307`). The lead never receives an answer. Authority refusal (`:4885`, `:4900`) and verification-input refusal (`:4800–4801`) have the same ordering.

This is not protected by the new durable terminal mechanism: plain `enqueue_receipt` takes the direct outbox path (`lib.rs:7516–7522`, `:7586`), and no terminal intent was saved before the crash. Expiry cannot reconstruct it either: staging skips IDs already in the refusal ledger (`state.rs:878–881`). This counterexample is source-derived; I did not inject a live crash or outbox failure.

**Minimum correction:** stage the terminal decision and its signed answer atomically through the existing mechanism (`lib.rs:7526–7555`) before closing admission. Preserve the special conflicting-command receipt key. Add a failure between durable refusal and outbox projection and prove retry still publishes the original answer. **Blocks A7.3's observable-discharge requirement.**

The wait-bound correction itself is sound: expiry precedes the busy-seat/one-seat-per-pass checks (`lib.rs:8263–8277`), stages an answer (`:8360`) and releases the record (`:8380`). With functioning persistence, no still-undecidable wake can retain the floor indefinitely merely because its input never arrives. Persistent storage failures can still retain the floor; removal failures are explicitly logged (`:8385–8391`) rather than silently forgotten.

Store writes now propagate errors without advancing the mirror (`deferred_turns.rs:192–193`), capacity refuses the newcomer (`:218`), and inability to hold a wake uses a recoverable `VERIFICATION_INPUT_UNHELD` answer (`lib.rs:4693–4713`). **Test limitation:** the test named `a_wake_that_cannot_be_held_is_refused_rather_than_lost` tests a failing standalone store, then calls the provider with a *writable* store and checks that the wake is held (`tests/deferred_admission_tests.rs:323–356`); it does not prove the provider's failure-answer branch. Also, startup file-read errors still become an empty held set (`deferred_turns.rs:130–134`), so “every deferred-store failure is surfaced” would overstate the implementation.

## R1 / 218 — closed

Both surfaces now consume one definition/hash answer: inbox reads it at `desktop/src/features/project-actions/ui/HostStepApprovalInboxCard.tsx:66` and matches it at `:83`; Actions stores it at `lib/useProjectActions.ts:170` and matches it at `ui/ProjectActionRunRow.tsx:86`. The matcher compares that object's hash (`lib/resolveBoundDefinition.ts:69`) and extracts its own definition (`:79`). The separate autorun status read does not supply approval bytes or hash. I found no remaining split-read grant path.

Native `desktop/src-tauri/src/commands/workflows.rs:264` reads one event; `:314–332` parses it once, returns the canonical definition value and hashes that same definition. `crates/buzz-workflow/src/hash.rs:27–41` connects `definition_hash_hex` to the same canonical-value function the relay uses (`crates/buzz-relay/src/handlers/command_executor.rs:963`). This hashes the returned canonical definition, not unrelated autorun state or raw JSON formatting.

Withholding webhook hashes is sound: the relay adds an unpublished secret before hashing (`command_executor.rs:939–964`), and webhook-triggered host steps are prohibited (`crates/buzz-workflow/src/schema.rs:620`). Native withholds the hash at `workflows.rs:322`; a missing hash prevents resolution (`resolveBoundDefinition.ts:52`). It does not disable an otherwise valid webhook host-approval path.

## R3 / 219 — closed

The observation writer now holds custody through its conditional write, releasing it only at `crates/buzz-session-provider/src/assignment_custody.rs:576`. Under the store lock it compares the complete row with the observed snapshot (`:517–525`) and refuses to modify any row owned by an attempt (`:527–535`). It no longer clears `attempt_owner`. Both “new owner survives” and “unchanged unowned row records the observation” tests passed. R2's mutex replacement is a separate serialization defect; it does not undo this row comparison.

## R5 / 221–222 — closed for the reviewed coverage holes

The positive requirements are explicit in `crates/buzz-core/src/project_work_fold_project.rs`: projected assignment at `:589`, nonempty canonical projection at `:597`, assignee signature at `:603`, included report at `:612`, criterion binding at `:620`, and matching bound assignment at `:628`. Disposition inclusion is unconditional at `:526`.

**Both requested counterexamples ran against the fold.** `report-absent-assignment` remains Open with `report_not_canonical` and null artifact (`conformance/project-work/fixtures/sequences/report-absent-assignment/expected-fold.json:28`, `:42`). `criterion-unassigned-report` leaves the unassigned criterion Open while its legitimate positive control stays Covered (`…/criterion-unassigned-report/expected-fold.json:30`, `:79`, `:91`). The test loads all 30 sequences (`project_work_fold_tests.rs:51`) and compares complete expected projections (`:149`).

The 31-row table is now a sufficient oracle for these counterexamples and its documented reason branches, not a proof of exhaustive protocol correctness: the Rust test reads the external table (`project_work_fold_tests.rs:577`), requires 31 rows (`:603`) and rejects unexercised templates (`:694`). The separate checker requires exactly one matching template (`conformance/project-work/check-fixtures.mjs:473`) and positive report/assignment/assignee relationships (`:499–523`).

**Independence qualification:** Git proves separation and order: oracle commit `dcda52700` touches conformance plus ledger; later implementation commit `ddfba303a` leaves that oracle unchanged. Both carry Brian's author identity and Claude Fable's coauthor trailer. Separate individual authorship is not independently verifiable from that evidence; I will not promote the ledger's assertion into proof.

No remaining empty-means-permission predicate was found in review-evidence evaluation. One unrelated empty-set waiver remains: goal membership at `project_work_fold.rs:419`, explicitly documented at `:92–97` as an unestablished goal set. It is not the R5 canonical-evidence loophole and is not a new blocker from this review.

**Readers / 223:** desktop shared vectors 6/6 and web 4/4 passed, including raw duplicate-key cases; duplicate rejection is present in desktop `codingSessionWireDecode.ts:33` and mobile `coding_session_wire.dart:26`; mobile runtime was not tested. No reader blocker added.

## Checks and release condition

Executed: 25 approval tests; 10 desktop/web reader tests; 18 core fold tests including all 30 sequences and 31 templates; 13 assembler tests; the fixture checker; five custody-boundary tests; five deferred-admission tests; the custody-reset probe. All existing targeted tests passed; the reset probe demonstrated simultaneous custody guards. No live control run, restart race or refusal-write fault injection was performed. Native hash tests were inspected, not executed.

Minimum blocking set: **R2 — preserve custody across actor turnover, including live establishments; R4 — make re-admission's terminal refusals durably answerable before fencing retries.** No further blocker was established in R1, R3, R5 or readers.

Hold — R2 custody replacement; R4 lost terminal answer.
