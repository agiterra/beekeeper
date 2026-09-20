> Written by Astra (GPT-6) for Brian, 2026-09-20, read-only against the repo; copied verbatim by Fable from /tmp/astra-unified-plan.md. The execution plan that governs the build is [docs/UNIFIED_WORK_PLAN.md](../UNIFIED_WORK_PLAN.md).

# One project, one contract, one evidenced result

Build plan · Astra for Brian · 2026-09-20 · proposed work, not a claim of implementation.
Source baseline: fetched `origin/main` twice; the second fetch returned `0848809665701d70b8ad19e48796416fcad0f794` (189–191 now included). Repository citations below refer to that commit unless marked otherwise. Installed/deployed parity is unverified; do not confuse a source landing with a running capability. Only the requested fetch changed Git refs/objects; no checkout, code, session or repository document was changed to produce this plan.

## 1. The join

**The project owns the instructions, the plan that defines success, and the evidence that the team delivered it.** Andy supplies the project-owned agents repository, versioned roles, model routing and approved host actions; durable work connects them to a goal through an exact plan revision, stable criteria, assignments and observed results. When complete, Brian gives a goal, the lead exercises judgment, software prepares and tracks the work, and the project can explain what remains, who owes it, and what proves delivery—even after a supported execution restart. This follows Andy's separation of code and instructions and the collaboration vision's shared working record; it does not promise arbitrary machine loss in the first release (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md:42`; `VISION_COLLABORATION.md:9`).

## 2. A plan file is the contract; the work record is its binding

### Proposed `beekeeper-plan/v1`

A UTF-8 Markdown file under `plans/` has YAML frontmatter: `schema`, project-unique stable `id`, `status: in-force|superseded`, `title`, `code_repository`, `delivery_ref`, nonempty `criteria`, and `retired_criteria` (default `[]`). Each criterion has unique slug `id`, human-readable `accept`, and `proof` of exactly one form: `{kind: review}`, `{kind: action, name, step}`, or `{kind: git-ref}`. These are evidence requirements, not executable commands or a dependency language. The body provides context, scope and permitted decisions. `action` resolves in the same agents repository's `actions.yml`; commands stay there. All criteria are required; a proposed waiver is an explicit plan amendment, never a green checkbox.

Validation rejects unknown schema/keys, duplicate IDs, active/retired overlap, empty acceptance text, escaping/absolute paths, symlinks, malformed repository/ref names and oversized input (64 KiB file, 64 criteria). Resolve repository names to canonical project-associated repository coordinates at adoption; refuse ambiguity. Read the committed blob with `git show <commit>:<path>`, never the working copy or fetched tip. The existing agents-file reader reads the fetched tip, so it must not be reused unchanged for contracts (`crates/buzz-session-provider/src/agents_checkout.rs:98`).

IDs identify obligations, not list positions: reorder, spelling corrections and a moved file preserve IDs; new obligations get new IDs; removed/replaced obligations go into `retired_criteria` and are never recycled within this plan. A substantive change to an existing obligation may retain its ID but requires new evidence under the newly adopted revision. Every evidence association binds the declaration event, criterion ID and artifact commit, so matching IDs cannot silently carry an old pass forward. For this slice, re-attest all criteria after amendment; selective evidence reuse can wait.

`status` governs **new adoption**, not execution state. `superseded` and `plans/archive/` refuse new adoption; archive remains optional organization/history. Existing declarations still resolve their original commit and path. Git commit is the version; no second version counter. Merely editing, renaming or archiving a file does not cancel or rewrite work already adopted.

### Worked example: `plans/kettle.md`

This preserves the existing kettle requirements and combines its test/action obligations. Source: `/tmp/pivot-agents/plans/kettle.md:3` through `:35`, clean checkout at agents commit `b8cd7095bb41c6d9e147917d022090a2d8e2f07d`. The example is a proposed new commit, not that historical blob.

```markdown
---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: >-
      Dependency-free Python 3 package kettle runs as python3 -m kettle.
      add <text> appends; list numbers oldest-first from 1;
      done <n> marks that item done and list shows it with [x].
    proof: {kind: review}
  - id: shared-storage-and-parser
    accept: >-
      All commands use exactly one store module and one command parser.
      Data lives in ~/.kettle.json, overridden by KETTLE_FILE for tests.
    proof: {kind: review}
  - id: verified-landed-revision
    accept: >-
      python3 -m unittest discover -s tests passes from the code root.
      The agents-repository verify action is published and runs that command
      green on the exact code commit observed at the delivery destination.
    proof: {kind: action, name: verify, step: verify}
  - id: usage-documentation
    accept: README has a Usage section showing add, list and done.
    proof: {kind: review}
  - id: delivered-main
    accept: One coherent history on main contains the accepted implementation.
    proof: {kind: git-ref}
retired_criteria: []
---
# Kettle
Build the CLI under kettle/ with tests under tests/.
Suggested slices: A is store/add/list/tests; B is done/README/tests and uses
A's store; C is the verify action in the agents repository. The lead may
combine A and B and must prevent a second store or parser.
The team may decide layout, output format and numbering after deletion
without asking Brian; record reasons in Pulse. This does not add a delete command.
```

The accompanying `actions.yml` entry uses `trigger: {on: manual}`, a `verify` step with `command: [python3, -m, unittest, discover, -s, tests]`, and `checkout: required`; manual bound checkout and refusal without a commit already exist (`crates/buzz-relay/src/handlers/command_executor.rs:1161`; `crates/buzz-session-provider/src/action_steps.rs:365`).

### Proposed signed join and amendment semantics

Allocate one sibling work-event kind in `buzz-core/src/kind.rs`; no number is reserved by this document. Keep 44244 unchanged: its bodies/envelope reject unknown fields (`crates/buzz-core/src/coding_session_team_transaction.rs:380`, `:543`). Reuse existing channel access, signed-event storage and session authority; no new task database or authority chain.

A declaration contains `{schema, workId, projectRef, sessionRef, goalRef, responsibleActor, planRef:{repository,commit,path}, supersedes}`. `workId` is a UUID stable across amendments; `planRef.commit` is the full immutable agents-repository commit; `path` is e.g. `plans/kettle.md`. Read plan ID and criteria from that blob: do not duplicate their text on the declaration. `supersedes` is a list of declaration IDs: empty initially, one for an ordinary amendment, all conflicting heads for explicit conflict resolution. Distinguish the plan commit, staged role revision, code artifact commit and action definition hash in every API and display.

At adoption, compile each referenced action from `actions.yml` at **the same agents commit P**, using the publication compiler and project scope; this fixes its expected definition hash and named step. Missing/invalid actions block adoption, not drafting: the lead commits the plan and action before adopting. A later action edit requires explicit adoption at P2 even if the Markdown bytes are unchanged; evidence cannot nominate its own expected hash.

The same sibling kind permits bounded association records: `{declarationRef, criterionIds, assignmentRef, replacesBinding?}` and `{declarationRef, criterionIds, artifactCommit, evidenceRefs, completionRef?}`. Evidence refs identify existing canonical reports/verdicts, action publication/run/result, and repository observation. Association is a claim to verify, never authority or proof by itself. Persist the signed references; derive statuses. Idempotently resume a partially published association/assignment sequence from its recorded IDs before issuing another wake.

Adoption/amendment requires the existing `may_lead` authority used for assignments, further scoped to the named project/session; repository write alone is insufficient. The responsible lead or another current `may_lead` actor adopts a committed plan against the explicit current declaration. A replacement names its predecessor and the authorizing goal/decision event; divergent successors display conflict and cannot complete. No timestamp winner. Resolve a fork with an authorized record naming all competing heads. This is a small declaration fold, not a per-transition relay receipt/CAS program.

A new agents commit alone means “new plan revision available”; the current contract stays pinned. For a requirement-changing steer, the lead first records its accepted goal/decision change, which marks the affected declaration stale; software does not infer semantic amendments from arbitrary chat. The lead then commits and explicitly adopts the amendment, updates affected assignments and supplies new evidence. Other independent work can continue. A role edit takes effect through the existing execution-boundary/restart process, not plan adoption. A changed action definition must refuse old approval, not borrow role restart semantics.

`none`: no clone; give the worker only the lead-authorized criterion/body excerpt needed for its assignment, with provenance. `read`: inspect the clone and propose an edit through a report. `write`: edit plans and roles, commit and attempt push under the actual Git authority. Lead/Project Setup hold coarse write for this slice; other seats default none/read. The grant concerns the working copy, not relay push permission, and Codex's tool fence is advisory (`crates/buzz-persona/src/team.rs:68`; `crates/buzz-session-provider/src/agent_fence.rs:98`). No invented “plans-only secure writer.” These choices answer Q8 and give Q9 a concrete “no split in v1.”

### What “done” means

Review criteria require an authorized semantic disposition linked to the exact artifact; green tests cannot establish architectural or UX judgments by themselves. Action criteria require the expected definition hash, publication/run/result linkage, exit 0, and matching clean before/after code revision. Delivery requires a fresh authoritative observation of the specified repository/ref with its tip exactly equal to that tested artifact SHA; archive the observation with its signer and time. If the read cannot establish delivery, show unknown—not a reported push as proof. This slice proves delivery as observed, not perpetual retention after arbitrary history rewriting.

The updated CLI checks this coverage, then publishes the existing signed `mission.completed`; ordinary late ACK/verifier/decision facts settle that same event through 183. Bind the completion ID to its declaration, and recompute work coverage on every relevant fact. Legacy mission settlement and work coverage remain separate; an old client's terminal record with missing coverage is a discrepancy. No genesis bump or claim that old clients enforce the stronger contract. ACKs still exist and may cost turns (`crates/buzz-core/src/coding_session_team_transaction_fold_settlement.rs:97`; `crates/buzz-cli/src/commands/sessions/operations_completion.rs:17`).

An abandoned attempt stays in history as abandoned, not approved. A replacement uses a new assignment and an explicit replacement binding; current completion names the fulfilling assignments. Existing completion checks its **named** assignments, and assignment correction preserves the original assignee/role (`crates/buzz-core/src/coding_session_team_transaction_fold.rs:477`, `:824`). Do not forge a missing worker's ACK or pretend a different worker is the old one.

## 3. Every open question in Andy's §6

Source for the question list: `docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md:1013`–`:1045`. Decisions below are this build plan's scope decisions; none waits on Andy.

| Q | Disposition and owner |
|---|---|
| 1 Include syntax | Already settled: retain `![[…]]`; no new syntax. |
| 2 Names | Already settled: `team.yml`, `actions.yml`; add schema to `plans/*.md`. |
| 3 Code-branch role override | Historical answer superseded by §4.11: agents repository is source; keep explicit definition restart. Spec `:701` retires branch override. |
| 4 Actions create a new umbrella | Freeze out: route/hire inside an existing authorized umbrella. Brian owns any later custody expansion. |
| 5 Automatic restart on role edit | Freeze off; retain explicit restart/adoption. Plan amendment is separate and may be decided by an authorized lead. |
| 6 Solo read denial | Moot under separate repository; ordinary Solo behavior remains. |
| 7 Community-wide repository names | Keep disclosed collision refusal, no silent suffix. Later naming change is Brian's product decision; not a dependency. |
| 8 Plan schema/archive | Answered by §2: frontmatter controls new adoption; commit/path preserves history; archive is optional organization. |
| 9 Plans-only versus roles write | No new grant split in this release. Coarse write is honestly disclosed; an enforced path-scoped grant is a later Brian-owned security design. |
| 10 Setup redesign | Existing-source maintenance uses a Project Setup seat with write and observed command results. The legacy wizard redesign remains open, owned by Brian/Fable; freeze it outside this slice and never overwrite a project's customizations to satisfy it. |

## 4. Build lanes with exclusive file ownership

Brian owns scope, Astra finalizes, Fable reviews the integrated candidate. First pin the integrated 180–191 baseline and freeze the schema/brief fixtures below. New paths are explicitly proposed, not assertions they exist. Brace lists mean exactly those files; directory ownership is exclusive and stated explicitly. Each lane owns its adjacent named tests; any additional file requires an ownership amendment before editing. Only the finalizer commits/lands; lanes do not. No lane edits another's files or the original live test projects.

**W1 — contract and projection.** Outcome: one parser and one pure fold resolve a pinned plan, its authorized bindings and current evidence coverage.
- Owns `crates/buzz-core/src/{project_plan.rs,project_plan_tests.rs,project_work.rs,project_work_tests.rs,project_work_fold.rs,project_work_fold_tests.rs,kind.rs,lib.rs}`; `crates/buzz-sdk/src/{project_work.rs,lib.rs}`; `crates/buzz-relay/src/handlers/{project_work.rs,project_work_tests.rs,ingest.rs,mod.rs}`; new fixture directory `conformance/project-work/` exclusively.
- Consumes **183 pending completion/missing-link fold**; **184 bound-checkout evidence**; **186 existing authority projection**. Expose `parse_plan`, `fold_work` and validated association types to other lanes; authorize work records with current session authority, scope and signer checks, without granting capabilities.
- Proves: same events in every arrival order give same coverage; duplicate publication is harmless; foreign/forged links rejected; ID reorder/retirement; missing blob; plan amendment/fork; old result cannot satisfy changed criteria; legacy terminal is not current work completion. Existing 44244 conformance stays unchanged.
- Must not touch 44244 schemas/fold, To-Do schema, action executor, or invent a second task list. Existing Pulse derives from the canonical fold, and To-Do visibility is explicit (`crates/buzz-core/src/pulse_declared_work.rs:7`; `crates/buzz-core/src/project_todo.rs:19`); add no automatic project-wide To-Do copy.

**W2 — agent-facing commands.** Outcome: the lead can validate/adopt a plan, bind assignments/evidence and complete from one inspectable CLI flow.
- Owns `crates/buzz-cli/src/lib.rs`; `crates/buzz-cli/src/commands/sessions.rs`; `crates/buzz-cli/src/commands/sessions/{work.rs,work_tests.rs,operations.rs,operations_completion.rs,operations_completion_tests.rs}`; new `crates/buzz-cli/tests/work_contract.rs`.
- Consumes **180 project registry**, **182 examples and report-derived `baseSha`**, **183 durable completion**, **184 `--checkout`**, **186 action delegation**, **190 runs/run-status/approval-ref reads**. Implement `sessions work validate|adopt|bind|status`; add opt-in work reference to CLI arguments, not closed 44244 bodies. Use existing assign/report/complete builders and reads.
- Proves: no live-write schema discovery; command retry preserves work/assignment IDs; plan resolves at commit after tip moves; cannot complete against stale evidence; pending completion exits successfully then settles without re-publication. Permission/read errors never become empty success.
- Must not alter acknowledgement meaning, model policy, workflow APIs or Git push admission.

**W3 — prepare, brief and continue.** Outcome: canonical assignments produce a useful brief and start only after their exact inputs are established, independent of any panel.
- Owns `crates/buzz-session-provider/src/{lib.rs,session.rs,state.rs,verification_input.rs,verification_input_tests.rs,agents_checkout.rs,work_brief.rs,work_brief_tests.rs,work_driver.rs,work_driver_tests.rs,assignment_inputs.rs,assignment_inputs_tests.rs}`; `desktop/src-tauri/src/coding_sessions/{assignment_input.rs,assignment_input_tests.rs,assignment_establishment.rs,assignment_establishment_tests.rs,workdir_store.rs,workdir_store_lock.rs}`.
- Consumes **180 routing facts**, **181 host answers without model wakes**, **182 exact inputs**, **183 settlement re-fold**, **185 preparation queue**, **187 reconnect clone reuse**, **188 recorded worktree recovery**, **189 listener replay/liveness**. Provider's verified complete-discovery path supplies assignments; raw ungoverned 44244 never moves a tree. Existing gaps are explicit at `desktop/src-tauri/src/coding_sessions/assignment_establishment.rs:21`, `:37`.
- Move the Tauri-independent 185 queue/checkout core into provider `assignment_inputs.rs`; Tauri commands become adapters. Retain the same `assignment_inputs` records, OS lock, refusal semantics and bounded interrupted-attempt retry. No concurrent unlocked writer, duplicate queue or destructive checkout. Tauri already depends on the provider (`desktop/src-tauri/Cargo.toml:116`).
- Defer an early valid wake durably while preparation is pending; resume once ready and revalidate authority/input immediately before start. Failed/dirty preparation reports one bounded blocker. Assemble criterion excerpts, goal/decision refs, exact code base, staged role obligations, effective permissions/model source and report examples; history remains lazy. No whole-repository prompt injection.
- Proves: never-opened Mission view; early wake; restart after queue-before-checkout; duplicate delivery; invalid assignment; dirty tree preserved; each opens at most one intended turn. Same-host worker replacement waits for observed process/effect quiescence, durably fences the old assignment/execution against restart or late wakes, preserves checkpoint/dirty data, then creates a new assignment/binding. An unreachable machine alone is not quiescence.
- Must not rewrite active role instructions, retain seat secrets to manufacture ACKs, alter listener 189, or promise lost-disk/cross-host exactly-once execution. Current verified input resolution already checks canonical assignment, actor, role and command (`crates/buzz-session-provider/src/verification_input.rs:95`).

**W4 — action definition safety.** Outcome: a run can never resume changed commands under approval of an earlier definition.
- Owns `crates/buzz-workflow/src/{lib.rs,executor.rs,suspend.rs,run_definition_tests.rs}`; `crates/buzz-db/src/{lib.rs,workflow.rs}`; `crates/buzz-relay/src/handlers/{command_executor.rs,host_steps.rs}`; `crates/buzz-relay/src/api/workflows.rs`; new `crates/buzz-test-client/tests/e2e_work_definition_binding.rs`; `schema/schema.sql`; proposed `migrations/0046_workflow_run_definition_hash.sql` (reconfirm number before freezing ownership).
- Consumes **184 bound commit/provenance**, **186 publish/trigger grant distinct from approval**, **189 request replay**, **190 observable runs and usable approval refs**. Persist the definition hash selected at run creation atomically with the run, never reconstructed from the latest workflow. Check it at approval, every resume and host-request emission; mismatch stops with `definition_changed`, requiring a fresh run/approval. Historical runs missing the binding remain readable but cannot gain unproven authority to execute.
- Proves against Postgres + real host fixture: change definition while awaiting approval, reuse same step ID, grant old request; changed command executes zero times. Also change between steps; hash-stable approval works; code checkout remains bound; duplicate result does not repeat a command; autorun grants re-arm on changed definition. See §7 for the source gap this closes.
- Must not delegate host approval to a seat, change command policy, add a scheduler, or build general workflow replay. Restart acceptance below parks before host claim/after durable result; arbitrary relay-crash resume is not claimed.

**W5 — product surface.** Outcome: the existing Mission/Actions surfaces show the adopted contract, exact evidence and one actionable next step.
- Owns `desktop/src-tauri/src/{lib.rs,commands/mod.rs,commands/coding_session_team_fold.rs,commands/coding_session_team_fold_tests.rs,commands/project_work.rs,commands/project_work_tests.rs,commands/workflows.rs,events/workflows.rs}`; `desktop/src/shared/api/tauriWorkflows.ts`; `desktop/src/features/coding-sessions/{hooks/useProjectWork.ts,lib/projectWork.ts,lib/projectWork.test.mjs,ui/ProjectWorkCoverage.tsx,ui/ProjectWorkCoverage.test.mjs,ui/CodingSessionMissionInspector.tsx}`; `desktop/src/features/project-actions/{ui/ProjectActionCard.tsx,ui/ProjectActionCard.test.mjs,ui/ProjectActionRunRow.tsx}`; new `desktop/tests/e2e/project-work.spec.ts`.
- Consumes **183 awaiting/pending fields**, **184 commit binding**, **185 displayed preparation**, **186 readiness/delegation**, **190 run/approval reads**. Render the core projection; no duplicate TypeScript fold. Link contract revision/criteria to original evidence, show stale/conflict/unknown explicitly, distinguish legacy terminal. Wire required checkout on Run and show the definition/code revision in approval.
- Proves mounted UI and isolated relay-backed run: app and CLI agree; pending ACK names its owner; changed plan invalidates coverage; no unavailable endpoint is shown as an empty history; Run cannot silently omit required commit. Current Run calls `triggerWorkflow(workflow.id)` without checkout (`desktop/src/features/project-actions/ui/ProjectActionCard.tsx:51`); CLI is the acceptance fallback until this lane lands.
- Must not become the scheduler, start a turn on render, redesign setup/To-Dos, or mint owner grants for a read-only viewer.

**W6 — role obligations.** Outcome: keep all eight identities available and teach each only the judgment/procedure it actually owes.
- Owns new versions only under `personas/templates/{lead,builder,architect,designer,verifier,runner,poker,project-setup}/1.1.0/` exclusively, plus `crates/buzz-persona/src/role_work_contract_tests.rs`. Existing versions and project clones remain untouched.
- Consumes **180 project-carried model advice**, **182 body examples**, **183 pending completion**, **184 verify action**, **186 capability disclosure**. Lead hires only for a bounded useful outcome; Builder includes ordinary tests; Architect resolves consequential boundaries; Designer checks the user journey; Verifier teaches `Blocked`; Runner leaves a reusable action; Poker explores with a budget; Setup proves readiness with commands. All use `$BEE` and the supplied brief.
- Proves composition/version provenance, no mandatory eight-seat staffing, no polling/ACK-repair instruction, and preservation of custom project text. Existing `$BEE` contract and inconsistent hire example are `personas/templates/working-contract/1.0.0/TEMPLATE.md:18` and `personas/templates/lead/1.0.0/skills/hire/SKILL.md:16`; verifier skill currently lists only CONFIRMED/NOT-REFUTED (`personas/templates/verifier/1.0.0/skills/refuter-pass/SKILL.md:8`).
- Must not reduce the roster, encode model prices/pins in prose, mutate staged roles or require specialists on every task. Eight identities are the creation contract (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md:628`).

**W7 — finalization and proof.** Outcome: one integrated, installed candidate passes the composed flow and the measured experiment.
- Owns `crates/buzz-persona/src/lib.rs` (test registration only), `desktop/playwright.config.ts`, new `scripts/test-unified-work-flow.sh`, new `docs/history/2026-09-20-unified-work-proof.md`, and future updates to `docs/{CURRENT_STATE.md,SESSION_STATE.md,PROJECT_TEAMS_AND_ACTIONS_SPEC.md}`. These are future build ownership, not permission for this planning turn to edit them.
- Consumes **180 registry routing; 181 host notices; 182 schemas; 183 pending completion; 184 exact actions; 185 preparation; 186 delegation; 187–188 reconnect; 189 listener liveness; 190 run reads/approve; 191 `just push`**. No duplicate fixes. Reconfirm candidate, installed app/CLI/provider and relay capabilities independently before testing; missing relay support is a deployment prerequisite, not a model repair.
- Proves required `just ci`, applicable `just test`, explicit desktop smoke and the §5 live run on isolated projects; `just ci` omits Playwright (`TESTING.md:28`). Use 191's `just push` on the final SHA; its mechanism is `scripts/push-with-floor.sh:1` and `scripts/pre-push-floor-stamp.mjs:1`. Reviewer findings return to owning lanes; finalizer does not quietly share their files.
- Must not test faults on the two audited sessions, claim a green unit suite is live proof, or suppress gates to meet a clock target.

**Sequence:** W7 pins the baseline; W1 writes its interface/fixture stub and W7 reviews/freezes it first. W1 and W4 then run in parallel; W6 can compose independently. W2/W3/W5 may build against frozen fixtures in parallel, but their integrated tests wait for W1; action/coverage acceptance waits for W4. W3 is deliberately one lane because discovery, durable preparation and turn opening share provider files. W7 integrates once all component tests pass, commissions adversarial review, gates, installs a candidate and runs acceptance. No waiting for an Andy push or review; Brian decides remaining scope.

## 5. Acceptance: finish useful work without operating Beekeeper as a second project

Run a **control** and then the **decisive disruption experiment** on fresh projects; score separately so induced downtime is not hidden in the comparison. Setup/approval decisions are recorded product interactions, never disguised as zero human involvement. Eight identities exist; the lead need not hire eight.

1. Create the project through Andy's two-repository flow. Adopt the kettle plan at agents commit P; record resolved team/template revisions, registry origin/hash, code repository and installed component versions. Keep the code repo free of Beekeeper roles/plans. Start one team goal; prove the live owner-signed 186 grant lets the lead publish/trigger verify without borrowing Brian's key or opening a publication ruling.
2. Require at least one **routed hire**, showing `registrySource: agents-repo`, requested class/risk, chosen runtime/model and actual seat runtime. Merely seeding a registry is not success; reader order and disclosed source exist at `desktop/src-tauri/src/commands/model_registry.rs:111` and `crates/buzz-cli/src/commands/sessions/route.rs:327`.
3. Keep Mission closed. The host prepares a verifier assignment at its report-derived `baseSha`, delivers its compact brief and starts it once. Deliberately race the wake ahead of checkout; it must defer and resume without a repair prompt or verification on the wrong revision.
4. Lead publishes verify from the agents repo; operator makes the **expected** first host approval through a supported product control and may grant future runs for that exact hash. Exercise 190's approval-ref command if the GUI is unavailable; count GUI absence separately. The lead's delegation must fail to approve a host step. Run with `--checkout <full code SHA>` and show definition hash plus clean matching before/after SHA. Change the definition while another run awaits approval: old approval executes no changed command, and a fresh definition requires fresh approval.
5. Action output follows an `actions.yml` routing rule into the existing lead umbrella. Known waiting/rerun bookkeeping uses software; do not hire Runner just to poll. Observe the actual main ref and all criteria at the delivered SHA, then publish completion before delaying the last genuine assignee ACK in the test harness. Release it: the **same** completion ID becomes terminal without another lead turn. Count the ACK turn if required.
6. Disruption run: explicitly give two independent scopes (Builder: implementation/tests; Designer: README/usage). This is a resilience test, not a staffing recommendation. After a pushed checkpoint, lose one worker while the other continues. Observe the dead process and outstanding effects settled on this host before the lead assigns its remainder to a replacement; retain local-only work as such and never erase it. Returning stale attempts cannot supply current coverage or resume competing writes.
7. During that run, send one deliberate requirement change—e.g. `list --open`—through ordinary steering. Lead amends the plan, adds criterion `list-open`, commits P2 and explicitly adopts it. UI/CLI show supersession; a late green result for P cannot satisfy P2. Keep unrelated work moving; resupply/re-attest criteria against the new artifact. No manual edits to protocol state.
8. Restart the app/provider with preserved stores at a pending-preparation boundary, and separately after a durable action result before its routed wake. Recover through normal replay, preserving the agents clone and declaration. Exercise 189 by suppressing one subscription delivery while leaving socket heartbeat alive: request must recover by the probe/reconnect path without manual relaunch. Read the same contract/evidence on a second updated client. Do not infer cross-machine takeover or lost-disk safety from these tests.

**Number to beat:** today was **5 h 31 m goal-to-terminal, about 35 min team work, six human repairs** (`docs/SESSION_STATE.md:14558`–`:14572`). Release target for the kettle-sized control: **zero unplanned human repairs**, under **60 min gross goal-to-terminal**, with expected approval wait disclosed separately and no correctness regression. These are targets, not predictions. The disruption run must complete with zero repair prompts beyond the scheduled steering/approval/fault injections; record each injected delay separately.

Measure goal/adoption/assignment/queued/prepared/started/report/action-request/approval/claim/result/delivery/completion times; useful versus administrative turns; orientation tool calls and schema refusals; ACK turns; chosen models; input/output/cache categories and reported-cost coverage; waiting by cause; recovery time; defects found/escaped. Do not infer dollars from cache tokens or process state from silence. Success requires every criterion at the final adopted plan and artifact, a terminal record, no duplicate effects, and an independent reader agreeing. Unknown safety state is a proper block, not a completed acceptance run.

## 6. Cuts and freezes

From revision 4: remove the separate acceptance-criteria payload and plan abstraction—the Markdown file is authoritative. Replace general interface-design Phase B with W1's small contract. Remove a general completion queue, generic dependency scheduler, broad role rewrite, new task-list UI, general takeover/lease platform, genesis negotiation and shadow-mode program. Keep scoped amendment and same-host recovery because the experiment now requires them; keep the research hazards as tests only for paths actually exercised.

From Andy's spec: preserve two repositories, versioned templates, eight installed identities, project registry, actions/routing, hash-bound approval and explicit role restart. Freeze new-umbrella hire, automatic role restarts, path-specific write grants, naming migration and setup-wizard redesign. Existing schedule/CI triggers remain supported but are not expanded in this delivery. Do not ship a second workflow engine or ask a model to carry configuration already in `team.yml`. Future work must earn its place through this run's measured residuals.

## 7. Rulings and evidence-based qualifications

**No disagreement with Fable's five rulings.** The sibling kind protects closed 44244 readers; other additive readers ship together. Eight available identities remain eight identities. Role adoption and executable-command approval stay distinct. Plan-file join replaces duplicated criterion state. Completion consumes 183's same-event re-fold.

There is one implementation gap beneath the changed-action ruling: approval resume loads the **current** workflow (`crates/buzz-relay/src/handlers/command_executor.rs:1822`); executor falls back from current-hash autorun to a run approval (`crates/buzz-workflow/src/executor.rs:894`); that approval matches run/step ID without definition hash (`crates/buzz-workflow/src/suspend.rs:99`). The host compares current compiled definition to request hash (`crates/buzz-session-provider/src/action_steps.rs:285`), which does not close that earlier substitution. This is a static counterexample, not a reproduced live exploit; W4 must reproduce and close it.

Two inherited limitations are retained honestly: 185 still lacks provider-owned first sighting/start ordering (`desktop/src-tauri/src/coding_sessions/assignment_establishment.rs:21`, `:37`), and 183 preserves assignee ACKs (`crates/buzz-core/src/coding_session_team_transaction_fold_settlement.rs:97`). W3 closes the former; this slice measures the latter without inventing credential custody. Source inspection and plan validation are complete; no build, live acceptance, deployment or production mutation was performed for this document.
