# Astra audit of Andy's NES RPG Plan team session — 2026-09-20

Written by Astra (GPT-6) at Brian's request, read-only, captured ~11:59Z. Copied verbatim from /tmp/astra-andy-rpg-audit.md by Fable. Andy's own solo-vs-team comparison is quoted in ledger item 179.

---

# Andy’s NES RPG Plan — read-only audit

Audited 2026-09-20. Relay transcripts/metadata captured at approximately 11:59Z; completion fold rechecked at 12:06Z. No session messages, rulings, pushes, worktree changes, process controls, or fixes were performed. The earlier kettle audit remains `/tmp/astra-kettle-audit.md`.

**Finding:** the team produced and tested a game, and its independent review caught real defects. It failed to finish its own protocol. All eight assignments are now settled, but no terminal mission record exists. The screenshot’s “Running · acknowledged” accurately describes that unfinished state.

## Scope and citation key

This is the eight-seat session in the screenshot, not a new run of the kettle runbook:

- Channel: `5d1e59bc-3202-4399-afca-5fad40041564` (RPG Test sessions).
- Umbrella: `377f3623-a247-4deb-a3b6-2ba27a86c1e2`.
- Genesis: `454981b564466e21a9a028ea422d07554570746197fa7a69915beffc8ed7611d`.
- Goal: `955b27d0f30d338252f61d2311253ca55263f63b1b08e09a2bb1864d57254067`, 2026-09-19 16:25:11Z: plan, build and test a small NES-Zelda-style action RPG, hiring necessary roles.
- Provider instance: `7464daa50904b3d9`; all eight executions use `claude-agent-acp`, generation 1. Metadata records bundled bee `45175475`, pack `f027bd1c56fb57eef0606883b5027c7e0b7455ce`. This predates the kettle run’s installed build; these are not controlled comparisons of one version.

Evidence is preserved in `/tmp/astra-andy-evidence/`. `L253` means `eventSeq:253` in the lead’s kind-44225 transcript. Each per-seat JSON file contains the full event ID and timestamp for each sequence. These are relay transcript observations, not independent re-execution of the game’s tests.

| Key | Seat | Execution/session ID | Completed result sequences through lead’s final turn |
| --- | --- | --- | --- |
| L | Lead | `fa948d3a-13f0-4efd-ac07-8519278e7caf` | 72, 102, 119, 123, 151, 190, 200, 218, 248, 287 |
| D | Designer | `9ab1358c-09e1-4ecf-b241-709da3240673` | 74, 87, 96 |
| A | Architect | `56e21d26-2768-4828-ada6-5ec4bc60f103` | 111, 131, 138 |
| S | Project setup | `58a576f8-0940-459e-96f9-1db3a1e17b75` | 117, 135 |
| B | Builder | `1c81d5df-1bb9-4b25-9501-606e1f6fe6f4` | 264, 271, 349, 359, 366 |
| R | Runner | `4155bc69-2100-4970-be8f-10fd8c701a01` | 133, 143, 152 |
| P | Poker | `c6ed021c-6443-4587-8dc4-25979dcbbfc0` | 249, 258, 265 |
| V | Verifier | `9794e389-2988-4a38-b702-bab79aad0304` | 133, 143, 150 |

There is also an earlier same-goal umbrella, `d1ca90b2-1d1c-4497-b6db-24cb0bbbb74a`, with lead execution `1428f00c-3e1d-4044-b28b-ee4afa63c7ae`, four result rows and three designer hire attempts. It is excluded from the eight-seat totals. L22 and L27 reuse lessons and a plan from that attempt; L33 adapts the plan to the standalone repository. This run therefore does not establish successful cold startup from a completely untouched setup. The earlier attempt’s closure records must not be mistaken for closure of the session in the screenshot.

## What actually finished

The first builder delivery had 91 unit tests and two browser tests passing. Independent review then exposed defects that those tests missed. Poker’s report `e3301a670e3824abfc54d203d8b2e5195d77b2ed0aa8433f82551aed06241d32` supplies reproducible cases for boss flag timing, death/pickup ordering, knockback/warp ordering, keydown latching and straddled-door unlocking. Runner’s report `7006a9f587db77d6d303574e918b3f1a931ce22407616f676cee8ec5b9784973` records a scripted Chromium journey through death, continuation, dungeon, boss and win, with 20 screenshots. This is useful review, not merely more approving agents.

The lead ruled on the defects and assigned P4; builder report `512ff8a194e6e7d39c242111ad329aa8ebd736aabebc8f9d5fd1c6d18d6653ea` describes repairs. More importantly than the report, **L253** (`c90a4e6d2a4e36663b6cc64116a0774b76daae0a4060e5622e6f6b329da78114`) contains actual integration/test output at merge `35deaae`: typecheck, 20 test files/160 tests, build, 2 browser tests and 6 poke-browser checks. **L256** records 20 maps/0 errors. The five failing repros were adapted to assert the ruled behavior; they were not simply discarded (L251 details the changes).

The game is on `nes-rpg-plan-lead`. A read-only `git ls-remote` during this audit confirmed:

| Remote ref | SHA |
| --- | --- |
| `refs/heads/main` | `340a06434ae0558c4809573b1cdb3b593a17b56e` |
| `refs/heads/nes-rpg-plan-lead` | `35a1d91bb8c659ad6a9b1d1ec90879132e4a5341` |

L264 and L284 contain the successful pushes. The last changes are documentation; the quoted test output is for the integrated code before those documentation commits. **Andy did not ask to land on main.** Unlike kettle, leaving the game on a topic branch is not a breach of an explicit main-landing request. I have not personally played the game or independently assigned it a quality grade. L285 discloses that all playtesting was scripted, and human feel testing remained a follow-up.

## Why the mission is still running

1. At **20:52:53.516Z**, L268 (`c289b25ff7906d6cab5e4627afbefed14f37c43db94f05844d0a3f2b2e308840`) refuses completion: “mission.completed requires every named active assignment to have an acknowledged approving disposition.” Only project setup had published a typed ACK; other seats had largely responded in prose.
2. This was a **local CLI preflight refusal**, not the relay rejection the lead described. At installed revision `45175475`, `crates/buzz-cli/src/commands/sessions/operations.rs:237` verifies before submission at line 266; lines 881–915 implement this refusal. The fold requires ACKs for the active assignments named by the proposed completion; the lead named all eight. Eight assignments means seven assignees, with two builder assignments—not eight distinct assignees.
3. L282 (`e97fa6f5465d9d85cd700809c930d57835906881e4de3e7d8d279041f5af8d6e`) wakes six seats solely to obtain the other seven ACKs. Its commands omit `--wake-to`.
4. The ACKs succeed. **A135** (`dd0bd171153d4bd8f10e389af873a032cda66a07055c08956effcd1904cc788e`) explicitly reports delivery `not-requested`, `published:false`: no wake was requested, so nobody was woken. B363, D93, V147, P262 and R149 show the same delivery outcome. This is successful transaction publication without a continuation notification.
5. The final ACK, runner `c8fc5f14e89b7bbca747d98975cb24a7b192be4936c840421a6349b6edd2bba5`, arrives at **20:54:02Z**. The lead ends at **20:54:17.062Z**. All required ACKs therefore existed before it finished, but L283–287 contain no fold refresh or completion retry. L285 still promises to retry once they arrive.
6. The 12:06Z audit fold (`fold-final.json`) has **8/8 settled, no conflicts, no exclusions, no waiting decision, `canonicalTerminal:null`**. There is no pending human ruling explaining this state.

Both layers contributed: the model failed to refresh once after fanout; the system made protocol delivery and resumption an optional flag that the model had to remember. At `45175475`, `operations.rs:98–101,251–269` handles explicit wake requests, while `sessions.rs:2663` routes acknowledgement through generic write handling. A fulfilled completion prerequisite did not automatically resume the blocked operation.

The screenshot is honest: `desktop/src/features/coding-sessions/lib/codingSessionMissionTransactionProjection.ts:301–323` at that installed revision intentionally shows running/acknowledged when ACKs exist without a terminal record. The defect is the unfinished flow, not a false success badge.

## Cost and coordination ledger

Window: first lead prompt **2026-09-19 16:38:47.871Z** through final lead result **20:54:17.062Z**. That is **4h 15m 29s elapsed**, not measured awake time. Goal publication precedes the first prompt by 13m 36.871s; the wire does not explain that startup interval.

| Seat | Turns | Input tokens¹ | Output tokens | Reported duration, min² | Reported USD³ | Turns containing orientation⁴ | Polling-only turns⁵ | No-open-turn wall min⁶ | CLI error responses⁷ | Queue→start p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Lead | 10 | 16,970,755 | 90,205 | 22.71 | 13.905748 | 4 | 0 observed | 232.78 | 20 | 0s |
| Designer | 3 | 2,122,072 | 75,907 | 16.24 | 2.781290 | 1 | 0 observed | 234.87 | 2 | 0s |
| Architect | 3 | 3,693,951 | 60,957 | 12.98 | 2.895462 | 2 | 0 observed | 237.97 | 5 | 0s |
| Project setup | 2 | 2,164,048 | 32,215 | 6.83 | 0.435316 | 2 | 0 observed | 244.04 | 9 | 0s |
| Builder | 5 | 23,531,885 | 221,947 | 48.02 | 13.487909 | 1 | 0 observed | 94.16 | 2 | 0s |
| Runner | 3 | 5,829,266 | 78,484 | 28.13 | 0.555213 | 1 | 0 observed | 16.10 | 1 | 0s |
| Poker | 3 | 8,788,554 | 118,617 | 28.30 | 0.608015 | 1 | 0 observed | 15.82 | 0 | 0s |
| Verifier | 3 | 3,391,588 | 35,783 | 8.24 | 3.871755 | 1 | 0 observed | 93.33 | 1 | 0s |
| **Total** | **32** | **66,492,119** | **714,115** | **171.44 seat-min** | **38.540707** | **13 mixed turns** | **0 observed** | — | **40** | — |

1. Sum of top-level `inputTokens` in the listed kind-44225 result rows, including cache. Their usage breakdown is **63,744,991 cache-read + 2,738,740 cache-write + 8,388 other input tokens**. These are repeated model inputs, not 66 million unique words of task context. Result usage records 632 tool calls.
2. Sum of `durationMs`, not CPU time, awake time, or critical-path time. Seats overlap. Builder has 139.27 minutes of prompt-to-result wall spans but only 48.02 reported minutes; runner 85.71 versus 28.13; poker 85.88 versus 28.30. One global transcript silence spans 19:30:44–20:21:26 (R61→R62). Sleep, transport gaps or provider accounting cannot be distinguished from these rows. Do not bill or label the difference as proven active work.
3. **Every seat’s first result omits `costUsd`.** These are sums of available fields, not total spend or a verified invoice. In particular the long initial product turns are unpriced here. Do not infer cheap poker/runner execution from their small displayed subtotals. No pricing assumptions were applied.
4. Orientation is embedded in working turns. Counts identify turns containing explicit role/skill reads, CLI help or schema discovery—not entire turns spent orienting, and not an orientation token percentage. Lead turns 1,2,9,10; architect 1,2; setup 1,2; other seats’ initial turns. Examples: L5/11/87/236/256; A6/83/126; S6/81/125; D10–16/61; B5/28; R5/119; P6/243; V5. The wire has no orientation label or per-tool token attribution. The runbook’s “one fifth of turns” threshold cannot be cleanly applied to these mixed turns.
5. No whole turn consisting of repeated “is it done yet?” polling was identified. There are ordinary status reads and checks inside productive turns. Zero polling does not prove reliable event continuation; completion still stalled.
6. Derived wall time with no recorded open turn between that seat’s first prompt and the common final-lead cutoff. It includes gaps between turns and time after its work ended, not just dependency waits. It does not prove awake idle resources, occupied processes, or physical worktree retention on Andy’s machine. No post-run overnight time is included.
7. Count of standalone JSON error responses in tool-result content, including deliberate schema probes and failed help/explain requests. These are not all authorization refusals or failed hires. Sequence evidence: L38/87/89/91/93/95/240/258/260/262/266/268/270/275/277/279; A70/80/84/122; S86/91/95/125/126/129; D65/69; B250/253; R126; V121. Some rows contain multiple errors. No refused hire was identified in this successful eight-seat umbrella; earlier attempts are separate.

Queue latency is matched kind-44224 `turn_queued` and `turn_started` by command ID, at integer-second precision. Lead has 10 matched pairs, maximum 43s (another 15s); worker follow-ups all have 0s differences. Production-window pairs: D2, A2, S1, B4, R2, P2, V2. Initial worker turns have no matched queue receipt in this extraction, so 0s is not a measurement of their first-hire startup latency.

At 21:06:04.505Z Andy asks the designer, “How do I test this?” (D97). D106 adds one completed turn: 390,834 input, 2,179 output, 30,820ms, two tools, $0.2891065 reported. Including this human follow-up gives **33 turns, 66,882,953 input, 716,294 output, 171.96 reported seat-minutes, $38.829814 reported**. It is excluded from production-window totals, and is not a completion repair in the lead transcript.

### A measurable piece of avoidable work

Eight worker turns respond to approving verdicts: D75–87, A112–131, S118–135, B265–271 and B350–359, R134–143, P250–258, V134–143. Six more repair the missing typed acknowledgements: D88–96, A132–138, B360–366, R144–152, P259–265, V144–150. Thus **14/32 turns (43.75%) are worker disposition/acknowledgement handling**, not new product implementation or adversarial testing. Some also preserve notes and learn the protocol; this is not a claim that every token in them is worthless. L120–123 is an additional lead turn responding to the architect’s prose acknowledgement.

The six explicit “Housekeeping, no new work” repair turns alone total **3,083,031 input tokens, 4,586 output tokens, 69.345 reported seconds and $9.3254195 reported costUsd**. Their result sequences are D96/A138/B366/R152/P265/V150. Those six costs are all present. They still did not close the mission.

## Vision scorecard applied to this run

These are the kettle runbook’s eight claims used as an audit lens. Andy’s task did not require kettle’s store trap, project verify action, role-change experiment, absence drill or main landing; omissions of those experiments are not invented task failures.

| # | Claim | Result | Evidence and limit |
| --- | --- | --- | --- |
| 1 | Starting requires intent, not orchestration expertise | **Partial** | L2 contains one founder goal and the supplied bench; subsequent lead prompts are agent report/ack wakes. Seven workers are hired. But an earlier same-goal attempt and reused setup lessons precede this run, and pre-start clicks cannot be reconstructed. The wire alone cannot award the full one-goal/one-start UI claim. |
| 2 | Decisions continue the work | **Partial** | L219–248 resolve poker findings and dispatch P4 without a human ruling. Assignment `3c9f1245f2e17dd1b4088dfe0ae9fb85578dbc7ccb3dbbda12a80c338f048ecd` and final approving verdict `3daa740f1f49d6f9f199fb389a11ee2a9c5a240ea66dba084b24bef6a7d2c336` carry the consequences. PLAN rulings D14–D19 exist in transcript edits, but the fold has no typed decision records; complete notification/reconsideration-trigger coverage is not established. |
| 3 | Continue an absent participant’s work | **Not exercised** | All eight bodies use one provider instance. No mid-slice takeover and old-consumer fencing drill was observed. Reusing the earlier lead’s plan is not that proof. |
| 4 | Roles evolve with the project | **Not exercised** | Metadata names pack `f027bd1c…`; no mid-run role publication/adoption transition was demonstrated. Several seats write personal schema notes; that is not proof of shared procedure adoption. |
| 5 | Observe parallel work before push | **Partial** | P1 designer/architect/setup scopes are separated before work, then integrated before the builder (L33–62, L73–151). Review supplies convergent fixes. This does not test detection of two independent implementations of the same mechanism, which was the kettle trap. |
| 6 | Deterministic operations first | **Fail** | Reports do wake the lead and no polling-only turn was identified, but L282 plus A135/B363/D93/R149/P262/V147 leave resumption to an omitted optional wake. All prerequisites arrive and no completion occurs. Fourteen worker turns service dispositions/ACKs. |
| 7 | Gates earn their delay | **Fail overall** | Independent testing earns its delay by finding reproducible defects; repaired evidence is in L253. The final ACK gate adds no new product evidence or authority, costs six repair turns, and leaves a settled mission running. No concrete product failure prevented by this final delay was recorded. |
| 8 | Two machines, one result | **Not exercised** | One provider instance, one integrated topic branch. There is no two-machine convergence evidence, and the mission lacks a terminal record. |

## Additional findings and limits

**The CLI makes agents reverse-engineer a protocol they are required to obey.** Designer D65/69 discovers report test-field and outcome spelling; architect A70/80/84/87 learns the same shape separately; setup S86/91/95/96 fails help and even searches strings in the bee binary. Builder B250/253, runner R126 and verifier V121 hit report-schema errors again. Lead L258/260/262/266 discovers the completion body one missing field at a time, then L275/277/279 discovers ACK shape. L22’s inherited cheatsheet explicitly recommends probing with `{}`. This is concrete system-induced overhead; model guessing contributes, but the repeated failure across competent roles points to deficient machine-readable recipes and briefing.

**An old session remains relevant to routing.** L240 refuses a send because the lead identity is seated in two umbrellas and needs `--session-ref`. A durable identity spanning sessions is legitimate; failure to carry the active umbrella into the operation creates avoidable ambiguity. This is independent of the final ACK gap.

**“No decisions on the wire” is narrower than “no decisions were made.”** The fold’s decisions array is empty, matching the screenshot, while PLAN rulings and approving dispositions plainly exist (L229–248 and the P4 assignment/verdict). Important reasoning lives across documents and transaction types; the UI is not a complete summary of it.

**Model and benchmark claims need qualification.** Metadata records lead `opus[1m]` and seven workers `claude-fable-5-1[1m]`, all Claude, not eight identically named Opus seats. The wire does not establish market pricing rank, why those models were chosen, or provider diversity. I have not audited the solo game session, its cost records, awake time or output. Therefore the reported “6× tokens, 3× awake clock, B+ versus B” comparison is **unverified**, neither endorsed nor disproved. Comparing this RPG directly to the much smaller kettle task cannot validate those ratios either.

## Assessment (under 400 words)

This is a working collaboration core surrounded by a protocol the agents still have to operate manually. Design, implementation, independent playthrough, adversarial tests and targeted repair form a real causal flow. Five reproducible bugs found after the first green suite are evidence that the extra scrutiny bought something.

The ending exposes the missing system layer. The game is integrated; every assignment is settled; the mission still says running. Almost half the completed turns are workers processing dispositions or repairing acknowledgements. Software should preserve receipt and continuation semantics without hiring model judgment to spell an ACK. The CLI also repeatedly makes each seat discover the same JSON schema through rejected commands.

The model owns a specific mistake: the lead could have refreshed the fold after fanout, seen the ACKs already present, and completed. It also chose substantial design and architecture overhead for a small game. But requiring perfect recollection of optional wake flags and hidden body schemas makes this failure predictable across models. The system owns most of that avoidable coordination work.

This final gate did not earn its delay. Independent testing did. Unlike kettle’s real publication-authority boundary, Andy’s stall names no unavailable authority and needs no human ruling. It is an unmet bookkeeping transition whose conditions were subsequently fulfilled.

The single best change is to make completion a durable pending operation: record the authorized request, name its missing prerequisites, and re-evaluate it when those facts arrive, with an explicit terminal disposition. Transport receipt should not consume a fresh reasoning turn; substantive assent can remain explicit. That would close this exact loop and remove the temptation to solve it with more prompts, polling, or another person’s click.
