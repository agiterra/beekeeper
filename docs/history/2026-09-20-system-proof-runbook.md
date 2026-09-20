# The system proof: the acceptance story, run for real — runbook, 2026-09-20

Drafted by Fable at Brian's request. Not run yet. Whoever runs it fills § 6
and § 7 in and dates them. Governing text:
[`VISION_COLLABORATION.md`](../../VISION_COLLABORATION.md) § "Measure useful
velocity" and the acceptance story in
[`COLLABORATIVE_WORKSPACE_PLAN.md`](../COLLABORATIVE_WORKSPACE_PLAN.md) § 6.

## 1. The question

Not "does each feature work" — the pivot proofs (ledger 171) and Andy's
173–177 answered that per feature. The question is whether Beekeeper, run
the way the vision says a person runs it, produces **one coherent completed
change** from **one stated intent**, with the agents spending their turns on
the work rather than on orientation, polling, waiting, and each other.

Three sub-questions, each with a number we can read off the relay:

| Sub-question | Measure | Where it is read |
| --- | --- | --- |
| Is there a flow? | Time from intent typed to first seat working; clicks and questions before that; count of refusals and hand-fixes along the way | wall clock, `HIRE_*` refusal codes on the session, the operator's notes |
| Are agents working extra hard? | Tokens and turns per seat (`TurnCost` on turn receipts); share of turns spent orienting (`--help`, re-reading roles), polling (checking on something software should announce), or waiting; wake latency `turn_queued → turn_started` | receipts on the session channel, seat transcripts in the provider outbox |
| Is it the 2030 surface? | The eight vision claims in § 5 scored pass / partial / fail with evidence, not opinion | § 5 table |

## 2. The task the agents get

Real, small, and shaped so that two builders are *tempted* to build the same
mechanism twice — the exact overlap the vision says must be caught early.

In the empty `pivot-test` repository (one README commit `e682191`), build
**`kettle`**, a to-do CLI in Python 3 with no dependencies:

- `kettle add <text>`, `kettle list`, `kettle done <n>`; items in
  `~/.kettle.json`.
- Unit tests that run with `python3 -m unittest`.
- A `verify` project action in `actions.yml` that runs those tests on the
  host, so the team gets a machine verdict against the exact commit.
- README usage section.

Slices the lead is expected to cut: **A** store + `add` + `list`; **B**
`done` + README; **C** the `verify` action. A and B both need the JSON store.
That is the trap: two stores is a fail on "observe parallel work", one store
with B waiting on A or B reusing A's claim is a pass.

Brian's intent, typed once, verbatim, in the Pivot Test channel or the new
team session goal field and nothing else:

> Build the kettle CLI described in the plan, with tests and a verify
> action, and land it on main. Use the team.

Everything else is the system's job. Every extra thing Brian has to type or
click is a finding.

## 3. Cast and machines

| Where | Seats |
| --- | --- |
| Brian's Mac, installed `7b8f0a92d` or later | Lead (persistent), Builder A, Verifier |
| Andy's machine, same build | Builder B, Runner |

Both are Pivot Test agents already on the roster (174). If Andy cannot join,
run both halves on one machine in two sessions and score claim 3 (absent
colleague) and claim 8 (two machines) as **not proven**, not as pass.

## 4. Steps, and what each one proves

1. **Intent, not orchestration.** Brian types the goal above. Record: what
   the founder pre-filled (folder, roles, provider), what it asked, what was
   greyed out and why. Start.
2. **The lead plans and hires.** Do not help it. Record the plan it publishes
   (Pulse plan, `bee pulse update --kind plan`), the assignments (kind 44244,
   each with `base_sha`), and every hire's disposition and latency.
3. **Overlap.** Builder A and Builder B start. Read their claims and their
   first commits. The pass is that one of them, or the lead, notices the
   shared store before either pushes a second one. Record who noticed, how
   (a claim, a channel message, a diff), and at what minute.
4. **Role change mid-run.** Andy edits `roles/runner.md` in the agents
   repository (one line: "run `python3 -m unittest` before reporting") and
   pushes. The next Runner hire on Brian's machine must compose from the new
   commit (`composeRef` in the seat manifest) with nobody re-staging by hand.
   Record the old and new `composeRef`, and what the already-running Runner
   reports it is using (it must not change mid-turn).
5. **Absent colleague.** Andy quits his app while Builder B is mid-slice with
   an unpushed commit. Brian's lead reassigns slice B. Record: native
   continuation or reconstruction, what the checkpoint carried, what
   local-only work was disclosed as lost, and whether Andy's relaunch tries
   to resume a competing execution (it must be fenced).
6. **Completion without polling.** Slice C's `verify` action runs on push to
   `main` (or manually if the push trigger is not live on hive; record
   which). The waiting seat must be woken by the run's completion. Count
   every turn in any transcript that asks "is it done yet" — the target is
   zero.
7. **Decision without a human.** Somewhere in the run a structural question
   will come up (where the store lives, how `done` numbers items). The pass
   is a recorded decision with reasons and a reconsideration trigger, made by
   an authorized seat, with Brian notified and not waited on.
8. **One coherent result.** `main` has kettle, tests green by the action,
   one store, README, and a history a stranger can walk from intent to
   landing: goal → plan → assignments → commits → gate rows → run record.

## 5. Scorecard

| # | Vision claim | Pass looks like | Result | Evidence |
| --- | --- | --- | --- | --- |
| 1 | Starting work requires intent, not orchestration expertise | one goal, one start, nothing greyed without a remedy | | |
| 2 | Decisions continue the work | ≥1 recorded agent decision, Brian notified not asked | | |
| 3 | Continue another participant's work | slice B lands after Andy leaves; old body fenced | | |
| 4 | Roles evolve with the project | new Runner commit adopted on the other machine without hands | | |
| 5 | Observe parallel work before the push | shared store caught before a second push | | |
| 6 | Deterministic operations first | zero polling turns; wake on completion | | |
| 7 | Gates earn their delay | every wait names what it prevented; no approval click that a grant could have covered | | |
| 8 | Two machines, one result | § 4 step 8 | | |

Score honestly. Partial is a real grade; the point of the run is the gap
list, not the trophy.

## 6. The "working extra hard" ledger

Fill one row per seat from the receipts and transcript:

| Seat | Turns | Input tokens | Output tokens | Orientation turns | Polling turns | Waiting (min) | Refusals | Queued→started p50 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |

Orientation = reading its own role, running `--help`, rediscovering the
repo layout. A seat that spends more than a fifth of its turns there is the
system's fault, not the model's, and the fix is in the briefing or the role.

## 7. Known gaps going in (do not rediscover)

- The Actions tab does not list published actions (171a); publish and read
  with `bee actions`. The inbox shows approval requests as raw JSON with no
  approve control (171b); an autorun grant on `verify` avoids the click,
  otherwise approve by CLI/hand-signed 46030 and score claim 7 down.
- `bee workflows approve` cannot answer a host-step request (171c).
- Seats get no session reference in their environment (171 open).
- Handover (claim 3) has a contract in `docs/HANDOVER_IMPL.md`; how much of
  it is live on `main` today is the thing this step measures.

## 8. Outcome

_Unfilled. Date, build, cast, and the two tables above._
