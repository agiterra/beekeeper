# Astra audit of the kettle team session — 2026-09-20

Written by Astra (GPT-6) at Brian's request, read-only, cutoff 11:49:46Z. Copied verbatim from /tmp/astra-kettle-audit.md by Fable; the runbook it scores is [2026-09-20-system-proof-runbook.md](2026-09-20-system-proof-runbook.md).

---

# Kettle team-session audit — 2026-09-20

**At the cutoff, the code is landed and independently checked; the requested end-to-end result is unfinished.** `main` is `fa927fdd085a7ad44104c33dea4ab550dc5107ea`. There is no published `verify` action or green action run in the observed run. The founder ruling remains unanswered. The verifier has delivered a substantive verdict, and the lead has started another round of protocol work to settle the assignments.

**Cutoff:** 2026-09-20 **11:49:46Z**, inclusive relay `created_at` second. Window starts at 11:27:00Z. This is a frozen audit of a continuing session, not a claim about what happened afterward. Brian deliberately withholding the ruling during this audit lengthens the measured open interval; that interval is not an estimate of ordinary human response time.

Read-only audit: no session messages, ruling answers, pushes, worktree changes, tests, or process interventions. Only audit scratch files and this report were written under `/tmp`. Read the runbook first, then the current-state map and governing vision. Source references below are in `/Users/brian/Projects/beekeeper/beekeeper`, inspected at `c2b155d873cb60d37aef8fe6f0006d82d0dc04a7`; this checkout is not shallow. Seat metadata reports bundled `bee` stamp `7b8f0a92`, app version `0.5.16`. Source interpretation is distinguished from observed runtime evidence. A read-only diff found no changes between the seat stamp `7b8f0a92` and `c2b155d87` in the cited settlement, workflow-admission, verification-input, action-schema and host-execution files.

## Evidence notation and scope

All transcript citations refer to kind **44225**, generation **1**, provider instance `1958c6c448e05eed`, driver `claude-agent-acp`:

| Label | Execution session | Actor | Effective provider/model |
| --- | --- | --- | --- |
| L | `74495ca8-2aeb-4f25-9f40-3124be1c476f` | `93710ade…` | `claude-primary` / `opus[1m]` |
| B | `6c628bef-7e1e-4600-9290-47fc772c4149` | `6240581c…` | `claude-primary` / `opus[1m]` |
| V | `f543d7bd-6059-44e3-a50b-4275d8440dd5` | `6d96ae61…` | `claude-primary` / `opus[1m]` |

Thus “L:122” identifies an exact transcript sequence, not a line in an agent's summary. At cutoff, the relay returned contiguous sequences L:1–181, B:1–115 and V:1–116: **412 transcript events, with no sequence gaps**. L and V each have an unfinished turn. The session is `e8338b95-b2bb-4ed8-84eb-57600a6e118e`, genesis `7cf4065c3e92493ef50ed5f3d3b12853f027a85acc7ba447e8f5218b6e15e835`, project channel `85b8db75-4b60-4741-bcfa-7f75cc238ff0`.

Relay reads used only `/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee`, with the specified identity and relay. `events query` used explicit kinds, channel, since and final until `1789904986`. Added 44220/44224 for delivery, 44223 for metadata, and 44245/44246 for policy/observations. Pulse was read separately through `pulse list --project … --since 1789903620`; project-scoped 44240/44248 reject a raw channel-only filter because they require `#a`. I do not infer absence of To-Dos from that rejected 44248 read. Seat manifests, provider state and the retained outbox were read as corroboration; the outbox is not a complete historical transcript. A separate read-only `git ls-remote` confirmed hive `main` at `fa927fd`. `/tmp/pivot-agents` lacks the `b8cd709` object and its checked-out actions file is older; the committed action evidence here comes from L:40–50, not an assumption that this clone was current.

## 1. Runbook §5 scorecard

| # | Vision claim | Result | Evidence and limit |
| --- | --- | --- | --- |
| 1 | Starting work requires intent, not orchestration expertise | **Partial** | L:2 carries the original goal plus the host-supplied bench and hire instructions. The recorded intent command and first `turn_started` are both at 11:27:22Z; work begins with L:4 at 11:27:24.598. Brian's additional bench selection is corroborated by policy event `35a783db1f6d34e0d19ca03829471e1a0f399fb5a9aaa2b91fffff9106abc41c` naming seven identities and `claude-primary`. Under the runbook, that extra setup is a finding. No contemporaneous founder-screen capture proves prefilled fields, disabled controls or their remedies; exact typed-to-start latency and click count are unknown. |
| 2 | Decisions continue the work | **Pass, for the delegated design decisions** | Pulse plan `d70d0fea81b07b88ba22dfae93f9d9b308c825d83bf3b4d4e50472f791f04773` fixes layout, numbering, output and store shape with reasons; L:56 publishes it and L:70 reports the decisions to Brian without awaiting approval. Assignment `a6f032c3acfbe9351ea5ac77f155f2afde4c3943509b8d0b8756464f19544a52` carries them into the builder's work. This meets §5's stated pass criterion. The stronger vision requirement for an explicit reconsideration trigger is not fully met: the plan gives assumptions and reasons, but no named trigger. The later authority ruling is a different category. |
| 3 | Continue another participant's work | **Not exercised** | No absent colleague, reassignment, checkpoint reconstruction, old-body fencing or relaunch occurred in the observed cast/commands. B implemented both slices. A builder reporting and a lead landing its commit is not takeover of an unavailable execution. |
| 4 | Roles evolve with the project | **Not exercised** | No Runner was hired and no Runner procedure was edited. Manifests show L's `packRef.sha=7963578…`, B/V's `packRef.sha=b8cd709…`; this supports adoption of the later agents-repository snapshot on new local seats. The changed file was `actions.yml` (L:40–50), not a role. Different role-specific `composeRef` digests do not prove cross-machine role-update adoption or in-flight stability. |
| 5 | Observe parallel work before the push | **Partial** | The lead anticipated the shared-store conflict and deliberately combined A+B in one assignment, before B's first edit/push; plan `d70d0fea…`, assignment `a6f032c3…`, L:70. B's result has one store and one parser, independently checked at L:110 and V:39. This is good judgment and a coherent result. It avoids the experiment's contested parallel work rather than demonstrating live semantic-overlap detection between two builders. No second implementation was started and then reconciled. |
| 6 | Deterministic operations first | **Partial** | No observed turn repeatedly polls for a known process to finish. B's report `daa6ff5131b2fd9fe2aa2dd6cba39be0d314af8496d615d89741d4968bec7cd6` wakes L via command `5611b1a59b728315c1c4c7ce31a030e9757af6056e4094d6598dc21e9af52dbc`; L:96 starts from its pointer. V's verdict `e0f9cfb519cd12b08f447f2ab5ab1d4691d161fa07b0f818c5e61a37576a53c8` similarly wakes L:149. But no action completed, so action-run correlation/wake is unproven. A stale refusal notification consumed L's second turn, and closure diagnosis consumed a fourth. Those are coordination overhead even without a polling loop. |
| 7 | Gates earn their delay | **Fail at the workflow level; legitimate authority enforcement locally** | L:122 records both host-action publication refusals. Decision `a009c23407287654d124ed9ba5907d13874471f6459580b23edad6c0039708dc` is unanswered, with `blocks:[]` and no terminal completion (L:175). The missing permission prevents unauthorized host-command publication, a real boundary. But project setup accepted a routine goal without the capability needed to finish it. Separately, L:156–180 shows opaque settlement requirements causing another verifier turn after a canonical verdict. The run has not shown that this additional delay prevents a concrete failure. |
| 8 | Two machines, one result | **Not exercised; local result also incomplete** | All three executions use Brian's provider instance. The runbook explicitly says single-machine evidence must not pass this claim. Locally, L:115/118 and the read-only remote check prove the linear landing; L:106 and V:27 prove recorded green test executions. However, zero 30620/46010/46013/46020 events were returned for the run window, V:59 lists only the older `describe-checkout` workflow, and the mission has no terminal completion (L:175). The required chain ends before a published green `verify` run. |

**This run cannot speak to** actual cross-machine collaboration, missing-machine recovery or fencing, shared Runner procedure evolution, action completion delivery, founder-screen usability, or a team-versus-solo performance ratio. It also does not test simultaneous competing store implementations. Andy's comparison is context supplied by Brian, not a measured control for this task.

## 2. Runbook §6 per-seat cost table

A **turn** means one provider turn, not a tool call or an internal model request. Tokens and active duration below sum only terminal `result` rows. Open turns have unknown additional consumption. “Orientation” includes role/skill/help/schema discovery; mixed turns also did useful work. “Waiting” is observed **no-open-turn time after a result**, until the next prompt or cutoff, not CPU-idle time or an inferred reason for every second.

| Seat | Turns at cutoff | Input tokens | Output tokens | Orientation turns | Polling turns | Waiting, min | Refusals | Queued→started p50 |
| --- | --- | ---: | ---: | --- | ---: | ---: | --- | --- |
| Lead | 3 completed + 1 open | 4,302,237 + unknown | 39,489 + unknown | 1 completed orientation/diagnosis-only; 2 completed mixed; open turn doing closure/schema diagnosis | 0 | 10.22 | 1 hire; 2 action publications | 0 s, 4 paired starts; max 43 s |
| Builder | 1 completed | 2,404,645 | 28,750 | 1 mixed | 0 | 12.83 | 0 hire/authority; 8 report-body validation rejections | Not available: initial hire turn has no queued receipt |
| Verifier | 1 completed + 1 open | 2,543,270 + unknown | 38,422 + unknown | 1 completed mixed; open turn is filing/protocol correction | 0 | 0.87 | 0 hire/authority; 19 verdict-schema rejections + 1 refused correction | 0 s, 1 paired start; initial hire turn unpaired |
| **Completed-result subtotal** | **5 completed; 2 additional turns open** | **9,250,152** | **106,661** | Categories overlap useful work; not a token allocation | **0** | **23.91 seat-minutes** | Different refusal classes above must not be collapsed into hire failures | Samples too small for a performance claim |

**Dollar cost is incomplete.** L's second and third results carry `$0.591701` and `$1.705715`, totaling **$2.297416 of reported cost**. L's first, B's first and V's first omit `costUsd`; both open turns have no result yet. The run's dollar total is **unknown**, not $2.30 and not zero for B/V. No external price estimate has been substituted.

### Exact result rows

| Seat/turn | Result sequence | Input | Output | `durationMs` | Tool calls in `usage` | `costUsd` |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| L1 | L:72 | 1,555,537 | 18,615 | 250,843 | 32 | omitted |
| L2 | L:95 | 580,779 | 6,307 | 84,873 | 9 | 0.591701 |
| L3 | L:148 | 2,165,921 | 14,567 | 217,087 | 21 | 1.705715 |
| B1 | B:115 | 2,404,645 | 28,750 | 351,032 | 46 | omitted |
| V1 | V:111 | 2,543,270 | 38,422 | 586,596 | 47 | omitted |

These are **155 tool calls** and **24.84 summed active turn-minutes** across completed turns; they are not 24.84 elapsed minutes. The lead's completed active time is 9.21 minutes, builder 5.85, verifier 9.78. At cutoff the overall wall interval since the recorded intent is 22m24s; completion has not occurred.

Top-level `inputTokens` includes cache reads and writes. The disjoint prompt components are:

| Seat, completed results | Fresh input (`usage.inputTokens`) | Cache read | Cache write |
| --- | ---: | ---: | ---: |
| Lead | 112 | 4,195,840 | 106,285 |
| Builder | 78 | 2,332,655 | 71,912 |
| Verifier | 70 | 2,449,322 | 93,878 |

Do not add these to the already cache-inclusive input total. These aggregate consumption across internal model requests, not unique text or maximum context occupancy. Contract: `crates/buzz-core/src/coding_session_payload.rs:2032`, `:2140`, `:2164`.

### Classification and timing evidence

- **Orientation:** an explicit 18-call early lead set is represented by result sequences L:6,7,11,14,15,18,19,22,23,25,27,29,31,33,35,42,52,60. Some mix task inspection with interface discovery. This supports the supplied “about 18 of the first 60 calls”; it does not mean 18 provider turns. L2, sequences 73–95, is wholly diagnosis/documentation of an already-recovered routing issue. B:6–20 is initial orientation; B:67–96 is repeated report-schema discovery/correction; V:8,9,16,20,23,28,31,57–71,79–86,97–101 covers skills/help/schema recovery. V's second prompt and L's fourth turn are protocol follow-up. All completed turns contain some orientation. The runbook's “one fifth of turns” heuristic cannot measure the token share inside these mixed, long turns.
- **Refusals versus mistakes:** there are 15 explicit CLI `user_error` JSON records in L, 11 in B and 26 in V through cutoff, including deliberate probes and ordinary argument mistakes. These are not 52 host/permission refusals. B's 8 report-validation errors occur at B:67,72,77,88,91 and three at B:93; its “four rejected attempts” summary excludes the additional schema probes. V's 19 schema errors occur at V:20,23,28,31,63,67,71,79; V:84 is the distinct refused correction. Counts omit plain shell/Python failures and do not double-count a tool's wrapper and the enclosed error.
- **Polling:** B:110/112 is one Pulse read retried after a JSON-shape error; V:68/72 is analogous. L:156/168/170/175 inspects the settlement contract, including reads after its own write. I classify these as orientation/diagnosis and confirmation, not a loop waiting for an external process. The practical overhead is real even with zero strict process-polling turns.
- **Waiting:** L waits 2.97 minutes after L:95 for B's report, then 7.25 after L:148 for V's verdict; the first turn boundary adds only 68 ms. B waits from B:115 at 11:36:56.191 through cutoff: 12.83 minutes, still idle with a clean retained worktree in its latest metadata. V waits 51.915 seconds between V:111 and V:112. These seat-minutes overlap. They do not prove the seats were needlessly retained for every second, and no hours-long idle period has been observed here.
- **Human dependency:** the ruling is open from 11:37:53 through cutoff, **11.88 minutes**. L/V continue independent work during it. Do not add this interval to the seat-wait subtotal.
- **Latency:** pair kind 44224 by `(session, commandId)`. L's queue/start differences are **0,43,0,0 seconds**; V's second turn is **0 seconds**. Same-second timestamps mean resolution-limited zero, not instantaneous service. B/V initial hires emit `created` and `turn_started`, but no `turn_queued`. Separately, successful hire-request→start is **6 seconds** for B (11:30:59→11:31:05) and **7 seconds** for V (11:38:53→11:39:00). These are different metrics.

**The wire does not carry** an orientation/polling/waiting classification, exact human click history, an authoritative waiting-reason duration, per-tool token attribution, complete dollar costs for this run, or cost so far for its open turns. Initial-hire queue times are absent. It does carry turn duration, cache accounting, result tokens and ordinary command queue/start receipts. Missing measurements remain unknown.

## 3. Assessment — under 400 words

This is a useful partial flow whose unfinished joins are being implemented by the agents in conversation. Planning, bounded delegation, branch publication and report-driven waking work. The builder produced the small program quickly, and the lead sensibly avoided manufacturing parallelism inside one store. The verifier added value by finding weak tests and reproducing the disclosed durability problems.

But the program was green and pushed before much of the coordination work began. The builder spent another 2m19s getting from confirmed push to accepted report, then another minute finishing protocol housekeeping. The verifier spent 9m47s on a change whose suite runs in under a second, and was immediately recalled for filing corrections. Comparing those durations does not make review worthless; it shows where to inspect the overhead.

Most waste is systemic: missing schemas, inconsistent command conventions, a stale refusal delivered as a new founder turn, and a settlement view that hides an existing disposition until acknowledgement. The models contribute avoidable mistakes: probing write commands against a live relay, putting the old SHA in a verifier assignment, and inventing a missing-refutation explanation instead of establishing it. The lead even taught the verifier the unsafe probing habit. This is shared responsibility, with the interface setting the trap.

The ruling was the right local call. A seat grant is not authority to publish commands onto an operator's host. The lead preserved that boundary and kept useful work moving. The system nevertheless accepted a goal it lacked standing authority to finish. That delay was avoidable at setup, not by making the lead more aggressive with credentials.

My one change for the next run: make **`verify <commit>` a complete project operation**, authorized once during project setup, executing the approved test action on that exact commit and delivering its signed completion to the waiting lead. That would turn the central promise into an actual flow. Schema help is urgently needed too, but fixing it alone would merely get this team to the same human dependency faster.

## 4. Findings beyond the anticipated gaps

### A. A recovered hire refusal caused a redundant, apparently human-authored turn

First hire event `d00b9a466464e3fbf9326825e4526d737daea2edf458a4310b21f4849029fa21`, at 11:30:49, requests class `builder`, risk `(impact=2, uncertainty=2, irreversibility=2)`. L:64 records **`HIRE_NO_ROUTE`**, because `/Users/brian/Projects/pivot-test/team/model-registry.yaml` could not be opened: **ENOENT**. This was not a full bench, routing-policy rejection of a measured model, or absence of offered models. L:82 sees 31 catalog targets; L:85 reports no registry in the searched paths.

The second hire `2bc3e3e3f16063c67ba3943221285fea5f4eabfb7de1bd4a84f67f16bfbd2724`, ten seconds later, omits routing. It starts B successfully using the identity's `opus[1m]` setting (L:67). The diagnostic's suggested remedy says “nothing offered clears” the class/risk and proposes changing class/risk or overriding; that is misleading for a missing registry.

Meanwhile command `5cdb8dbe51b95c0f29f38c86ee95147442919b0af4b93fb1c7f20f3a11e18a69`, at 11:30:50, queues the refusal text as a new turn **signed by Brian's key**. It starts only at 11:31:33, after the successful retry. L:74 interprets it as “You're pointing at the refusal,” then spends 84.873 seconds, 580,779 input tokens and 6,307 output tokens investigating it. Brian says he did not send that message. The evidence establishes an extra founder-signed command and redundant turn; it does not establish another human intervention. Startup software should not make its own recovered error look like new user intent.

All seats use `opus[1m]`. No cheaper-model comparison was exercised, and “most expensive available” is not established here. The evidence is fallback to a saved setting after a failed routed hire.

### B. Learning the schema by writing reached the real ledger

L:31/33/60 probes assignment/report writes; its verifier brief explicitly recommends probing an empty verdict body (hire `aa5c7121ba0714193c3ea4035fc09341eec6d83fbe0f5a1216752bcb95a4a464`). V:63 eventually publishes placeholder **`approve-with-notes`, summary `s`, findings `["f"]** as event `4b7ce7d9f7ec6367006255f483485624f8869bba83825e306450c451a82f8b40` at 11:42:36.

That event was **stored but excluded**, because its `assignmentRef` named the verifier assignment instead of the report's builder assignment. No wake was requested. V:84/94 confirms the exclusion; V then publishes a substantive canonical verdict `e0f9cfb5…`, without superseding the excluded event. This is not evidence that a dummy approval governed the work. It is evidence that schema discovery can create durable live records, with the fold saving this particular case for an unrelated reference mismatch. The later verdict describes its placeholder as “probe”; the actual stored values were `s` and `f`.

### C. The exact-input contract did not establish the intended verifier checkout

Assignment `0ceddfb27b2611e89b2c85b22fdd9783c5adb4ad4911c528b75d030c3963056b` says verify `fa927fd` in its objective and acceptance steps but sets **`baseSha=e6821913923edc618202981ce1f22bb4c4e382b9`**. V's checkout stays clean at that README-only commit (V:55/108 and metadata). V manually archives `fa927fd` to scratch and tests there (V:19/27), providing useful correct-revision evidence outside the structured seat input.

The source fence recognizes only strict assignment-pointer turns (`verification_input.rs:63`); this hire's initial turn is prose. Its tree comparison reads structured `baseSha`, not the prose objective (`:142`, `:265`). Thus this run cannot be called a successful exact-input-fence proof. It was rescued by the verifier's manual workflow.

### D. Publishing the proposed action would not, by itself, finish the exact-commit story

L:40 adds a **manual** trigger, `working_directory: "."`, `python3 -m unittest discover -s tests`, timeout 300s, and no `checkout` binding. L:44 validates its parse and L:50 pushes `b8cd7095bb41c6d9e147917d022090a2d8e2f07d`. No push-trigger capability test or reason for choosing manual is recorded.

The default is the recorded project directory as currently checked out, **dirty or clean** (`crates/buzz-workflow/src/schema.rs:297`). The provider samples `headSha`/`dirty` after execution (`crates/buzz-session-provider/src/action_steps.rs:1217`). Commit-isolated checkout exists for `ref_updated`/`ci_result`, but not a manual trigger (`schema.rs:614`). A later green manual run needs evidence that it actually tested `fa927fd`; the YAML does not ensure that. This is a latent contract gap, not an observed wrong-commit action run—there has been no run.

### E. The real authority boundary is more specific than “an owner key”

Observed refusal: L:122. Source admission requires **channel owner/admin** for host steps (`crates/buzz-relay/src/handlers/command_executor.rs:804`), plus project creator/roster Owner/endorsed-repository founder for project publication (`:821`). Agents-repository write access is a different grant (`crates/buzz-persona/src/team.rs:68`). No narrow seat-based publication delegation appears in that path. Brian's literal private key is not intrinsically required; an identity satisfying the actual authorities could qualify. Merely making the lead a “relay admin,” one ruling option, is not demonstrated to satisfy both checks.

Execution also has a separate approval/autorun stage, bound to the action definition hash (`crates/buzz-workflow/src/executor.rs:880`; `command_executor.rs:1331`). The runbook anticipated host-step approval, but this run hits **publication authority earlier**, so it never reaches that known gap. An old grant on `describe-checkout` would not authorize this new `verify` definition. I did not read or assume an existing grant for the unpublished action.

### F. Settlement diagnostics have already induced a false repair

V's substantive verdict is canonical (V:94), but both assignments show `settled:false` and null disposition fields. L publishes a second approving disposition, `8cd1f37c6581499456c218bf8c5db29dceb3dc09909bddf3f3f58d1767a9692e`, and sees the same nulls (L:166–175). L:176 concludes that a missing **refutation** prevents settlement and recalls V to refile; command `8c1221a58384f1230e52ddc2802927532f5cfb33680aadc8ea7b18a70c804405` starts V's second turn at 11:49:39.

That explanation is contradicted by the inspected fold: `crates/buzz-core/src/coding_session_team_transaction_fold.rs:846` accepts an approving disposition/report pair; `:866` requires an acknowledgement; only a complete chain populates `dispositionEventId` at `:906`. It does **not** require a refutation there. The missing acknowledgement explains why the existing disposition is not displayed in settlement. This is a model reasoning error amplified by a lossy diagnostic surface. The verifier's own assignment also still lacks a report/settlement chain at cutoff. There is no observed `mission.completed` refusal yet, so I am not claiming the exact eight-acknowledgement failure from Andy's run has repeated.

### G. The verifier found concrete limits in the test evidence

V:46/50 removes `kettle/` in a scratch copy: four tests still pass because a missing-module error satisfies “nonzero exit and nonempty stderr.” They are `test_add_without_text_fails_visibly`, `test_done_on_an_empty_list_fails`, `test_no_command_fails_visibly`, and `test_unknown_command_fails_visibly`. V:43 also exposes that `test_add_does_not_touch_the_developers_real_file` checks the scratch file exists, not that the real home file stayed unchanged. Separate recorded checks show the real file remained absent during this run; these are coverage gaps, not evidence of an actual unintended write.

B's historical red-before-green claim has stronger evidence than the verifier used: **B:27** actually records the red run, 14 collected tests, 9 failures and 1 import error, before B:31–36 creates the implementation. B:41 records 25 green tests. We cannot rerun the past, but the provider transcript corroborates its ordering; this is not only the builder's final testimony.

V:51 also reproduces the named lost-update and crash-truncation residuals. The latter can leave a zero-byte store. Those are disclosed limitations, not surprise scope additions. The audit did not independently execute code or test other Python versions/platforms.

## 5. Evidence index for the principal milestones

| Milestone | Time UTC | Durable evidence |
| --- | --- | --- |
| Recorded intent/start | 11:27:22 | command `c4378f8535cb9eb56c4dc09d0192f607296faa0d9e38abd522531d692b74c776`; start receipt `ec9c0d76eff8a5a12dbd5c56d082f2798b241f5e8b2be59d579c24787a6ee528` |
| Action source pushed | 11:29:17 | L:50; commit `b8cd7095bb41c6d9e147917d022090a2d8e2f07d` |
| Builder working | 11:31:05 | start receipt `76db077ab4acc608e170ccf0441cf0ce7a86be63306a30c4d2bf758d2305568a` |
| Builder push confirmed | 11:33:37.954 | B:62 |
| Builder report accepted | 11:35:55 event; tool returns 11:35:56.565 | report `daa6ff5131b2fd9fe2aa2dd6cba39be0d314af8496d615d89741d4968bec7cd6`; B:96 |
| Code landed | 11:36:44.966 | L:115, event `da541a2fa9c7d5d7bfda64d86659a5eb3fb93cca8bbe75b4948abaa20e2be0fb`; confirmed L:118 |
| Publication refused | 11:36:57.152 | L:122, event `93516de8b037361899171d6c279698722050491635396affe666ea471049bb21` |
| Founder ruling opened | 11:37:53 | `a009c23407287654d124ed9ba5907d13874471f6459580b23edad6c0039708dc` |
| Verifier working | 11:39:00 | receipt `8bf2e188306c7ee4ff034e7e90e08699b25c220b61213f5a67629a97aeb80791` |
| Substantive verifier verdict | 11:46:47 | `e0f9cfb519cd12b08f447f2ab5ab1d4691d161fa07b0f818c5e61a37576a53c8`; V:91/94 |
| Frozen settlement state | 11:49:01.653 | L:175, event `a8e3dfcc96565af2eb04ec365f319d704eebede109c68095cf17f5a814689e34` |

The read-only relay snapshot used for arithmetic is `/tmp/astra-kettle-evidence/final.json`; it is supporting audit material, not a modification to the project record. The single audit deliverable is this file.
