> Written by Astra (GPT-6) for Brian, revision 4, 2026-09-20; copied verbatim by Fable from /tmp. Fable's review is in the map and ledger 178–179.

# Beekeeper: the next complete flow

Implementation plan, revision 4 · 2026-09-20 · Astra · for Brian and Fable

**Build one small task from intent to observed delivery, using the batch already underway. Measure before expanding the protocol.** This replaces revision 3 as the build plan. Its detailed analysis is preserved as [research notes and future hazards](/tmp/astra-beekeeper-durable-work-research-notes.md), not prerequisites to implement wholesale.

Planning remains read-only against the product: only `/tmp` artifacts were written. Branch inspection establishes committed source, not installation, successful gates or live behavior. Changes landing in the shared checkout belong to the ongoing integration work; they are not this audit's edits.

## 1. Outcome and scope

Brian gives the goal once. The team understands what must be delivered, receives usable work briefs, performs useful work, runs verification on the intended revision, observes delivery and finishes without another completion prompt. Agents supply judgment; software carries references, prepares inputs, observes results and updates derived state.

For the first proof, use a fresh kettle-sized task with one stable goal, one responsible lead, one implementation attempt at a time, one verification action and one Git destination. The work can have several acceptance criteria without requiring several seats. Lead and Builder are responsibilities; they need not always be separate executions.

This first slice does not prove worker takeover, lost-disk recovery, concurrent plan editing, cross-session control transfer or full mixed-version enforcement. Those claims need later experiments. A process restart with preserved durable state is a distinct, smaller guarantee.

## 2. Consume the current batch

Local main was `226141bba6a255aba9217ded80957934c733955c` when checked for this revision. Main includes lane 182's CLI changes and lane 184's explicit manual checkout. Other branch tips below were inspected independently; their presence is not a claim they are landed or deployed. The finalizer must pin the integrated candidate before testing.

| Lane | Source inspected | What Phase C consumes |
|---|---|---|
| 180 | `ffc54e849` | Project-carried `model-registry.yaml`, consistent resolution, and the team's runtime/model advice. The role skill requests capability and reads the effective choice; it does not remember configuration in prose. |
| 182 | `ca4524ca3`, included in inspected main | Offline body examples, fuller schema errors and report-derived verification input. |
| 183 | `ec3d3c8ae` | Publish the existing signed `mission.completed` while prerequisites are pending; the same record becomes terminal when the fold receives them. No second completion record, queue or CI continuation is required. |
| 184 | `226141bba`, inspected main | Explicit manual action revision and before/after checkout evidence. |
| 185 | `c8912d5dd` | Durable host-owned input establishment after the assignment has been observed. Close the remaining first-sighting/start gap only where the proof requires it. |
| 186 | `46cc2d2b6` | Owner-signed, project-scoped delegation to publish and trigger actions, plus readiness disclosure. Host-step approval remains separate. |

Do not recreate those interfaces. Fable's integration review decides their final composition. Lanes 181 and 187 remain part of that integration assessment; this plan makes no independent completion claim about them.

Two limits are explicit in the inspected code:

- **183 retains the acknowledgement requirement.** Its settlement fold still requires the assignee's ACK, and ledger 183(f) explicitly says the receipt still costs a turn. The improvement removes local refusal and the second completion turn; it does not by itself eliminate every ACK turn. Sources at `ec3d3c8ae`: `operations_completion.rs`, `pending_completion.rs`, `coding_session_team_transaction_fold_settlement.rs`, ledger 183.
- **185 still gets its first assignment sighting through the frontend**, and does not order establishment before the CLI wake. Its provider fence prevents execution on the wrong tree, but prevention is not automatic progress. Sources at `c8912d5dd`: `desktop/src-tauri/src/coding_sessions/assignment_establishment.rs:21` and `:37`. Test the never-opened-view case, not only closing a panel after it saw the assignment.

## 3. Build sequence

### A. Integrate and prove the existing path

Land and gate the current batch through its existing finalizer. On the exact integrated build, run one small composed task. Record which facts are now supplied automatically and which still need model or human bookkeeping.

In this pass, explicitly test pending completion, explicit checkout, action publication/trigger authority and the input-preparation race. Do not call a readiness warning an enforced permission, or a publish/trigger grant permission to approve execution on a host.

Apply the small role corrections already supported by the protocol: use `$BEE` consistently and teach Verifier its existing `Blocked` result. Do not remove current safeguards before their software replacements work.

### B. Freeze only the interfaces the next slice needs

This is a short integration design step, not a separate platform program. Freeze:

1. The smallest signed work declaration and its references to existing assignments/evidence.
2. The host-assembled work brief.
3. The exact input/preparation/start interface consumed from 185.
4. The read projection that distinguishes declared coverage, observed results and legacy mission completion.

Use the same existing authority checks and folds where applicable. Do not add fields to closed 44244 envelopes. If a work record needs new structure, use one sibling kind with a bounded schema and references to existing facts.

The initial work declaration needs only stable identity, project/session scope, exact goal/plan reference, responsible participant, bounded acceptance criteria and intended artifact/delivery targets. Add only the typed association facts needed to join existing assignment/report/action/ref observations to that declaration. Do not copy full reports or create another mutable task-status database. A role/ownership declaration grants no permission.

Keep the declaration immutable for the proof. Unsupported amendments, competing declarations or a changed goal produce an explicit conflict/stale state, not a timestamp-selected winner or silent acceptance. Editing and takeover semantics can follow once the first slice earns them.

### C. One vertical slice

The lead records what this task must deliver. The host assembles each worker's brief, establishes its exact input and starts it through existing execution machinery. The builder produces the artifact. An action verifies the intended revision. An authorized participant makes any required semantic judgment. Beekeeper observes the named destination and exposes the evidence for all declared requirements.

The lead publishes completion through 183's existing mechanism. If ordinary settlement facts arrive later, the same signed completion becomes terminal without a second publication or model turn to check again.

Include the host-assembled brief in this slice from the beginning. It carries purpose, exact input, relevant project instructions, current decisions, scope, effective capabilities, evidence requirements and reporting references. Preserve lazy history access. Do not make workers rediscover the assignment body or infer another seat's local state.

Use 180's project configuration for routing. Keep model/runtime preferences in their structured project sources, including `team.yml` and the registry. A role prompt explains how to request and inspect a choice, not the current prices or remembered roster.

## 4. Completion and compatibility boundaries

Separate two questions in the projection:

- Did the existing mission protocol settle its named assignments?
- Does the evidence cover this work declaration's required outcomes and exact revision?

Lane 183 answers the first more reliably. The new work view answers the second. Reuse its fold-driven approach: when the necessary signed facts already exist, derive state instead of creating a queue to announce it again. Durable intent is needed for an effect still owed, not for every computed status.

**No genesis-version bump in this slice.** Use updated CLI/host/Desktop components in a controlled opt-in proof. Legacy clients keep their existing semantics. A sibling work kind does not make those clients aware of stronger requirements, so do not advertise universal enforcement. The new view must expose a legacy completion whose work coverage is missing as a discrepancy, never treat it as evidence of the stronger claim.

Publish the legacy completion after work coverage has been checked, allowing 183 to handle late settlement facts. Recompute work coverage when facts change; a stale or competing declaration cannot remain green because an old mission terminal exists. Before enabling stronger enforcement generally, decide the smallest effective compatibility/admission boundary using the proof's results. Do not assume it must be a new genesis version, or that it can be omitted forever.

## 5. Hazards that constrain the slice

The research notes remain an adversarial checklist. A hazard becomes implementation work when this slice exercises the affected path or promises the guarantee. Otherwise record the limitation and defer the mechanism.

| Hazard | Minimum response for this proof |
|---|---|
| Wrong checkout or wake before preparation | Exercise a never-opened view and a deliberately early wake. Establish and resume through a supported path without a human repair prompt; do not weaken the existing input fence. |
| Existing ACK still requires a model turn | Count and disclose it. Do not claim 183 eliminated it. Any change to ACK authorship/meaning needs its own narrow authority/contract review. |
| Action definition changes while a run waits | Bind the exercised definition/input; refuse or park a mismatch. Do not execute changed commands under old approval. Broader immutable-run/continuation changes are required only if that resumable path is included in the proof. |
| Approval or result is accepted but resume is lost | If approval/result restart recovery is claimed, exercise the actual crash boundary and make continuation durable. Otherwise disclose the unproven boundary; do not describe generic workflow recovery as shipped. |
| Push commits but its event/response is missing | Observe authoritative committed state before declaring delivery. Do not repeat a push merely to manufacture a receipt. Historical delivery after later ref replacement needs retained proof before that guarantee is claimed. |
| Missing/ambiguous authority or effect outcome | Block only the dependent operation and state the reason. Do not infer grants or blindly replay external effects. |
| Work/To-Do visibility differs | Keep the work's existing audience. No automatic project-wide To-Do creation or private content copying. Reuse an existing task reference if safe; broader binding/transfer UX is deferred. |
| Another goal or declaration appears | Mark the affected coverage stale/conflicted and return a bounded judgment to the lead. Do not implement concurrent plan editing as an incidental feature. |

The definition-binding and workflow-resume concerns are static source findings, not reproduced incidents. Their detailed sources and counterexamples remain in the research notes. They must not become generic blockers for unrelated batch fixes.

## 6. Base roles included in this delivery

Keep all eight available. Every engagement names its question, inputs, authority/scope, expected evidence and exit condition. Use specialists only when the work or project policy justifies them.

| Role | Job worth a model turn | What to change |
|---|---|---|
| Lead | Interpret intent, choose acceptance, delegate useful work, resolve tradeoffs, judge the integrated result. | Supply judgment requests with evidence; remove preparation/polling/receipt administration as the host takes it over. |
| Builder | Implement the scoped outcome and ordinary tests; challenge contradicted assumptions while continuing independent work. | Supply exact inputs and reporting references. Rename `brief-is-law` to reflect outcome ownership in a future versioned update. |
| Architect | Resolve a consequential structural boundary or costly assumption. | Bounded question and terminal decision; no compulsory architecture stage. |
| Designer | Make the whole user task understandable, usable and truthful, including recovery/accessibility. | Reduce unnecessary steps and decisions rather than merely displaying internal complexity accurately. |
| Verifier | Challenge the actual artifact against named constraints. | Teach `Blocked`, alongside `Confirmed` and `NotRefuted`; report coverage limits and never equate lack of a counterexample with full acceptance. |
| Runner | Establish or repair an unfamiliar execution procedure; investigate a bounded environment problem. | Known commands run as actions. Leave a reusable validated procedure so later runs require less model work. |
| Poker | Explore real user sequences, misleading controls and feature interactions. | Give it a workflow, exploration budget and evidence standard; no unlimited defect hunt or silent repairs. |
| Project Setup | Discover and establish usable project procedures and capabilities. | Demonstrate readiness by exercising the relevant commands and permissions; generated text is insufficient. |

Shared corrections:

- Working contract: preserve authority, exact references, evidence and staged-version rules. Fix hire examples that hardcode `bee` despite the `$BEE` rule.
- Memory: retain scope, revision, evidence and supersession. Current instructions and observations outrank stale recollection.
- Pulse: replace per-turn rereads with relevant change delivery only after replacement coverage is demonstrated. Until then preserve the safeguard and measure its cost.
- Packaging: place essential role obligations directly in the assembled brief; keep longer conditional procedures on demand. Avoid a series of tiny skill reads just to learn basic obligations.
- Configuration: use the project registry and team manifest; no model should remember configuration the system can supply.

Version changed templates under existing rules, preserve project customizations and keep active instructions fixed. Detailed role-by-role reasoning and source links remain in §23 of the research notes.

## 7. Implementation ownership

Fable performs the integration review and assigns strict file ownership after the current batch is pinned. Suggested small work packages:

| Package | Deliverable | Boundary |
|---|---|---|
| Work declaration and evidence projection | Minimal sibling schema, existing-fact joins, conflict/unknown disclosure, CLI read surface | No general control chain, takeover or policy engine |
| Host work brief | Verified assignment-scoped inputs, decisions, capabilities and reporting references | Consume 180/182 and existing context/history APIs |
| Prepare/start completion | Close the actual residual 185 gap demonstrated by the proof | Reuse its durable store and the provider fence; no parallel preparation implementation |
| Integrated role and UI surface | Bounded engagements, small role fixes, truthful coverage/status view | No eight-role default; no all-client redesign |
| Finalizer and composed acceptance | Register interfaces, run required gates and the measured proof | One owner for shared registrations and conflicting provider/core files |

Evidence/action fixes are narrowly added to the responsible existing package when the selected proof actually exercises their failure path. Rebase onto the final batch; do not design from this ref snapshot after main advances.

## 8. Acceptance and measurement

Run on an isolated fresh project, not the existing live audited sessions. Record exact app/CLI/provider/role/action revisions, assignment/event IDs, artifact commit and destination observation.

Required proof:

1. One goal, one visible declaration of required outcomes, standing authority supplied through normal project setup.
2. Worker receives a usable host-assembled brief; no schema guessing or copied context from another seat.
3. Assignment arrives with Mission never opened; an early wake cannot strand the worker or run it on the wrong revision.
4. The named action tests the intended revision and discloses before/after tree state.
5. The actual destination is observed; a report or intended SHA is insufficient.
6. Completion published before the final settlement fact becomes terminal through re-folding, with no second lead completion turn or re-publication.
7. A provider/host restart at a supported boundary with preserved stores does not lose pending work. Record unsupported uncertain-effect boundaries separately.
8. Another updated client reconstructs the work coverage and its evidence from the shared record.

Measure time to accepted delivery, useful versus administrative model turns, orientation calls, schema retries, human repair prompts, queue/preparation wait, token categories, reported cost coverage and product defects. Record residual ACK turns explicitly. Host-assembled briefing is a candidate for the largest orientation improvement; measure it rather than claim savings in advance.

Functional pass means the declared artifact/action/delivery outcomes are evidenced and the supported recovery path needs no coordination repair from Brian. An honestly blocked unsupported authority/effect case is a safety pass, not a completed mission. Do not claim zero administrative turns if ACKs still consume them.

Use focused conformance and integration tests, then repository-required gates on the integrated candidate. Run applicable relay/database tests and UI smoke separately where required; `just ci` does not include Playwright smoke. A source-text assertion or green lane report is not the composed proof. No production tests or live mutations were performed to write this revision.

## 9. Deferred until evidence justifies them

No requirement to build these before Phase C:

- New session-genesis version or universal profile negotiation.
- General accepted control chain, per-transition receipts and CAS plan admission.
- General dependency scheduler or a second completion queue.
- Concurrent plan edits, scoped worker takeover or cross-session work transfer.
- Comprehensive To-Do binding/privacy redesign.
- Universal historical delivery journal, arbitrary effect replay or transparent key/process migration.
- Hard multi-provider monetary budgets, broad resource arbitration or general shadow-mode infrastructure.

The existing authority and input boundaries remain mandatory. Deferral narrows the claim; it does not permit false completion, private-data disclosure or unauthorised effects.

After the proof, decide whether stable work identity, brief assembly and evidence coverage measurably reduce effort and improve reliability. Then select the next experiment—likely two independent workers, a changed requirement and a replacement worker—and build only the additional machinery needed to make that experiment safe and honest.
