> Written by Astra (GPT-6), 2026-09-20, read-only adversarial review of Wave 0 at `c0b474548`; copied verbatim by Fable from /tmp/astra-wave0-review.md. Rulings: [UNIFIED_WORK_PLAN.md](../UNIFIED_WORK_PLAN.md) § 8 A2.

# Wave 0 adversarial review — 2026-09-20

**Verdict: hold the Wave 1 integration gate.** Lane 193 still has two definition-binding races; lane 194 is not yet a sufficiently determinate contract for parallel implementations. Lane 192 has no blocking defect. The fixes below fit the existing design; none requires expanding the closed session envelopes.

Reviewed local HEAD and origin/main at `c0b4745481ee686f5a3aa3295fc78dda6f5777fe`, including `docs/UNIFIED_WORK_PLAN.md` and A1, ledger 192–194, and the relevant implementation and fixtures. Paths below are relative to this repository. “Blocks” means the Wave 0 exit gate should remain closed, not that unrelated preparatory work must stop.

## Findings, ranked

### 1. P1 — Action-wide approval can authorize a definition the person never approved

**Lane 193; blocks Wave 1.** `crates/buzz-relay/src/handlers/command_executor.rs:1409` checks the pending run against the current definition, but `:1478` subsequently reloads the workflow and `:1486` uses that later hash for the autorun grant.

**Failing interleaving:** run A is waiting; the person grants `scope: action`; the comparison sees A/A; another publication changes the workflow to B before the later read; the grant is persisted for B. The old run may correctly stop on resume, but a fresh B run now finds a grant for B and requires no new approval. The host cannot save this: B's file and request hash agree (`crates/buzz-session-provider/src/action_steps.rs:285`). This is not just a stale status display: consent has moved to another command definition.

The event-coordinate lock is keyed by kind/author/d-tag, not a shared workflow approval/publication lock (`command_executor.rs:159`); the publication upsert and grant insertion use pooled operations (`:998`; `crates/buzz-db/src/workflow.rs:1480`). Bind the grant to the approved run's immutable hash, never a later workflow read. Add a barrier-controlled test that publishes B between comparison and grant creation, then starts B and asserts no B autorun grant or unapproved host request. The existing autorun test inserts grants directly and exercises sequential edits, not this handler race (`crates/buzz-workflow/src/run_definition_tests.rs:561`).

### 2. P1 — A run can carry B's hash while executing the parsed definition A

**Lane 193; blocks Wave 1.** `crates/buzz-db/src/workflow.rs:1025` binds the hash currently in the database at INSERT. `crates/buzz-workflow/src/executor.rs:1397` compares only database run/workflow rows; it never hashes the `def` argument actually executed at `:1609`. The assurance in `crates/buzz-db/src/workflow.rs:1013` that this case stops is therefore false.

**Failing interleaving:** a trigger handler parses A; B is published; INSERT binds the new run to B; the execution check sees B/B and permits the old A body. For example, a webhook can execute A's removed message/webhook step while its run claims B. This path retains the earlier definition at `crates/buzz-relay/src/api/bridge.rs:2419` and passes it at `:2450`. The same shape exists in event triggers (`crates/buzz-workflow/src/lib.rs:434`, `:487`, `:514`) and schedules (`:916`, `:971`, `:974`).

The host's file/hash check does not establish that the relay evaluated the same definition's conditions and step selection. Require the parsed definition, run binding and admitted current definition to agree before effects, or atomically admit the caller's expected hash when creating the run. Test the exact read-A/publish-B/create/execute-A sequence, including a changed condition. Retain the resume/emission fences; fixing only INSERT does not replace them.

### 3. P1 — The frozen happy path claims coverage without the evidence needed to establish it

**Lane 194; blocks Wave 1.** `conformance/project-work/README.md:338` defines fold inputs without canonical evidence or action definitions. Its `:405` requires evidence resolution and a satisfied proof; `:407` requires `unknown` for missing evidence. Yet `conformance/project-work/fixtures/sequences/happy-path/inputs.json:1` supplies no report, verdict, action result or committed `actions.yml`, and its events file contains only 44249 records. `expected-fold.json:25`, `:71` and `:125` nevertheless require covered review/action criteria and complete coverage.

**Failing case:** feed precisely the published happy-path inputs to two implementations. One correctly refuses to infer a passing action from an event ID; another trusts the lead's binding. Only the latter matches the expected fixture. The contract also does not specify the canonical review disposition or action-result kind/status, expected hash, request/result linkage and clean before/after revision predicates. Those requirements are explicit in `docs/history/2026-09-20-astra-unified-plan.md:91`.

Freeze the complete pure-function input shape, evidence validation predicates and actual canonical evidence fixtures. Add missing-event, wrong signer, wrong run/hash, failed action and dirty-revision cases. A signed association must remain a claim to verify, as README `:307` already says. These are read dependencies on existing evidence, not new keys in its closed envelopes.

### 4. P1 — Coverage can combine tests and delivery from different revisions

**Lane 194; blocks Wave 1.** `conformance/project-work/README.md:269` attaches a commit to each binding; `:420` makes coverage complete when every criterion is individually covered. It never requires those criteria to establish one common delivered artifact.

**Failing case:** review/tests are valid for A, documentation review for B, and the fresh delivery observation for C. Every local proof passes the stated rule; the whole declaration becomes complete even though delivered C was never verified. This contradicts the kettle example's exact-delivered-commit requirement (`conformance/project-work/fixtures/plans/valid/kettle.md:20`) and original plan `:91` cited above.

Define the candidate artifact against which declaration coverage is evaluated and require all applicable criteria, including delivery, to prove that artifact. Add a mixed-SHA rejection fixture. Do not let human-readable acceptance prose substitute for this mechanical invariant.

### 5. P2 — The documented authority set contradicts the existing predicate it requires

**Lane 194; blocks the Wave 1 authority implementation.** `conformance/project-work/README.md:300` describes existing `may_lead` as founder plus live lead-seat actors. The actual predicate also admits any active `may_steer` grant (`crates/buzz-core/src/coding_session_team_transaction_fold.rs:95`).

**Failing case:** a collaborator with a steer grant but no lead seat signs an amendment retiring criteria. A lane reusing `may_lead` accepts it; a lane implementing the README's enumeration rejects it. Specify the actual existing predicate, or explicitly rule on a narrower authority contract, and add a steer-grantee fixture. This requires no 44228 envelope change. Do not silently settle it independently in CLI and relay.

### 6. P2 — A permitted decision-backed declaration is immediately stale

**Lane 194; blocks the Wave 1 declaration fold.** `conformance/project-work/README.md:217` allows `goalRef` to name a goal **or a decision**. `:388` and Decision 14 at `:710` require equality with the current 44227 goal.

**Failing case:** an authorized adoption references its authorizing decision, as the schema permits. That ID differs from the current goal, so its declaration can never attain coverage. Preserve the decided current-goal rule: restrict `goalRef` to that goal, keep decision references distinct, and add a fixture. The ruling is coherent; the earlier schema text was not reconciled with it.

### 7. P2 — Fork detection does not define descendant or duplicate-root conflicts

**Lane 194; blocks the Wave 1 declaration fold.** `conformance/project-work/README.md:389` defines conflict as unsuperseded successors of the **same predecessor**, while `:222` requires explicit resolution of all competing heads.

**Failing case:** P forks to A and B; A2 supersedes only A. The remaining heads A2/B no longer share an immediate predecessor, so a literal implementation can clear conflict without resolving B. Two initial declarations sharing a workId and empty `supersedes` are similarly unspecified. Define conflict over maximal valid declarations for that workId, scope predecessor references, and test both cases. The current direct-sibling fork fixture does not settle either (`conformance/project-work/fixtures/sequences/fork/expected-fold.json:14`).

### 8. P3 — The fallback matcher also catches a genuine version refusal

**Lane 192; does not block Wave 1.** `desktop/src/features/coding-sessions/lib/codingSessionCommand.ts:51` uses substring matching for “unsupported coding-session command tag.” The actual version refusal “unsupported coding-session command tag version” at `crates/buzz-relay/src/handlers/ingest.rs:3173` also matches.

**Failing case:** an unsupported envelope version causes an unnecessary untagged retry. The version is preserved, so that retry also fails; both failures are logged and no successful downgrade is claimed (`desktop/src/features/coding-sessions/lib/codingSessionHireDisclosure.ts:168`). Match the normalized rejection exactly. This is a narrowness defect, not an authorization bypass or evidence that the fallback hides a refusal.

## What I checked and found sound

- **No production creation path found that omits the new hash:** source search finds one `INSERT INTO workflow_runs`, at `crates/buzz-db/src/workflow.rs:1022`. Manual/event/ref/schedule/webhook callers use that helper. Finding 2 concerns which definition it binds, not a missing column.
- **The ordinary changed-definition stop is shared:** execute/resume entry checks at `crates/buzz-workflow/src/executor.rs:1581`; suspension emission checks before creating/publishing approval or host requests at `crates/buzz-workflow/src/suspend.rs:147`; approval admission checks at `crates/buzz-relay/src/handlers/command_executor.rs:1409`. Host-result resume reaches the common executor (`crates/buzz-relay/src/handlers/host_steps.rs:617`). I found no separate unchecked emission path in the reviewed routes.
- **Historical runs fail closed:** migration leaves the binding nullable (`migrations/0046_workflow_run_definition_hash.sql:18`); missing hashes return `definition_unknown` (`crates/buzz-workflow/src/executor.rs:1387`). Database errors propagate rather than granting execution (`:1395`).
- **Host verification is useful but narrower than relay consent:** it checks compiled definition hash, step index, ID and kind (`crates/buzz-session-provider/src/action_steps.rs:285`, `:297`, `:320`, `:323`). If its repository copy differs, it refuses. It cannot detect a wrong autorun grant or certify which definition the relay evaluated; findings 1–2 explain those disagreements.
- **The contract's architecture remains sound:** explicit adoption pins commit/path; tip edits are not silently adopted (`conformance/project-work/README.md:124`); amendments do not carry evidence forward (`:413`); coverage is separate from legacy mission settlement (`:431`). Kind 44249 leaves 44244/44223/44228 untouched (`:599`). I found no requirement that forces changing those closed envelopes.
- **Delivery observation has a real producer:** relay-signed 30618 checks match `crates/buzz-relay/src/api/git/manifest_event.rs:55` and `:108`; README freshness at `:291` rejects superseded ref observations. This is stronger than accepting a reported push.
- **The fallback preserves the refusal:** target, marked answer text and boundary delivery survive the fresh signed retry; only its host-answer tag is removed (`desktop/src/features/coding-sessions/lib/codingSessionHireDisclosure.ts:130`). Unrelated errors do not retry (`:148`); authority/admission still precedes provider suppression (`crates/buzz-session-provider/src/lib.rs:4567`).

## Verification and limits

Read-only review of the named files, creation/resume/approval/autorun/webhook/schedule paths, host comparator, five sequence fixture sets, plan fixtures and record fixtures. The fixture shape checker passes; it explicitly does not implement the fold (`conformance/project-work/README.md:657`). The existing fallback test file passed **10/10**; exercising its actual predicate confirmed both the unknown-tag and version-refusal strings match, while membership/timeout strings do not.

The two races are source-derived concrete interleavings, **not live exploit reproductions**. I did not rerun Postgres/Rust integration tests, the reported before/after reproduction, or a real older-relay/host run; no database, build, repository or live session mutations were performed. The reported lane-193 “one request before, zero after” establishes that tested sequence, not these additional interleavings. Wave 1 should proceed once findings 1–7 have explicit fixes and adversarial fixtures/tests, without widening the protocol program.
