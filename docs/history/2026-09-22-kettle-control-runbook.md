# `kettle-control` — the control run: runbook, 2026-09-22

**Run record — 2026-09-22, 12:48:54Z → 14:04:15Z (1 h 15 m 36 s), Brian at the
keyboard, Fable orchestrating; installed build `36f337d4a`, hive `build_time`
`2026-09-22T03:19:28Z`. Session `613d53a6-50e3-4010-838a-0a736a0b4da1` in
channel `29cc5029-8686-4dd4-85dc-d66423912bab`. Terminal `35b6e791`,
`completed --without-coverage`. Two unplanned human acts, one planned
approval. Eight findings, filed as ledger 236(a)–(h). No §4 known-open defect
fired; the run was not stopped.** Measurement:
[`/tmp/kettle-control-measure.md`](file:///tmp/kettle-control-measure.md) over
`bee sessions measure` JSON and `bee sessions audit` (this machine only).

Drafted by Fable at Brian's request, 2026-09-21; filled in the day it ran.
§3, §5, §6 and §7 are the run's record. Governing text:
[`UNIFIED_WORK_PLAN.md`](../UNIFIED_WORK_PLAN.md) §1, §2, §8 **A11** (the ruling
that lifted the "control run waits on R2/R4" gates, for this run only). Modelled
on [the 09-20 runbook](2026-09-20-system-proof-runbook.md); baselines are ledger
178 (morning kettle) and 205 (Kettle Smoke).

## 1. The question

**How well does the flow run when nothing is broken on purpose?** A
*measurement*, not a safety claim (A11). No faults are scheduled. Three numbers:

1. Goal typed → terminal record, against 5 h 34 m (178(r)) and ~70 min (205).
2. Unplanned human acts, against six operator repairs (178) and zero (205).
3. Turns and tokens the team spent, from `bee sessions measure`, not from a
   model reading transcripts (197).

It says nothing about the known-open defects in §4; if one fires, the run
**stops** and that is the result.

## 2. Preflight (night before, or at the keyboard before the clock starts)

None of this counts against the two-hour budget. A red line is fixed before the
goal is typed, never during the run.

| # | Check | How | Want |
|---|---|---|---|
| P1 | The app is the build that landed tonight | quit and relaunch `~/Applications/Beekeeper Dev.app`; read **About** and `"$BEE" --version` | the sha `CURRENT_STATE.md` records for the installed bundle after 231/232 (templates 1.2.0). Write it down; it is the run's identity |
| P2 | hive is running that landing | `curl -sH 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq '{build_time}'` | a `build_time` **later** than the landing. `software_commit`/`/health` answer `unknown` here — a disclosed non-answer, not a failure |
| P3 | Relay git auth is wired | `"$BEE" git status` | wired. Without it a fetch hangs on a username prompt (`GIT_TERMINAL_PROMPT=0` to see it fail fast). Needs **git 2.46+** — the bundle must not find Apple's 2.39 first (memory: git ≥ 2.46 required for relay auth, 2026-09-19) |
| P4 | A provider is connected and a model answers | the app's provider row; nothing greyed | connected, not "Idle over a disconnected provider" |
| P5 | Login keychain unlocked, machine will not sleep | unlock once now; `caffeinate -dimsu` in a spare terminal | a tauri-dev rebuild re-prompts the keychain and parks the app with no provider (memory: dev app refresh needs Brian at keyboard) |
| P6 | No dev server is about to rebuild under the app | `pgrep -fl 'tauri dev'` | empty. Never rebase or switch branches in the live checkout during the run |
| P7 | Nothing else is using the desktop dist | no `just ci`, no `just smoke` running | idle (memory: smoke and ci share desktop dist) |
| P8 | The bundled `bee` is what you will type | `export BEE="/Users/brian/Applications/Beekeeper Dev.app/Contents/MacOS/bee"` | `"$BEE" --version` prints the P1 sha. A repo-built `bee` on `PATH` is stale — never use it |
| P9 | Env for every `bee` call | `export BUZZ_RELAY_URL=wss://hive.agiterra.org; export BUZZ_PRIVATE_KEY=$(cat ~/.nostr/key)` | set. **Never echo, log or paste the key** |

## 3. Steps

Clock starts at step 3 (goal typed). Fill a row per step as you go.

1. **Create the project.** In the app: new project **`kettle-control`**, its own
   agents repository `kettle-control-beekeeper-agents`, an empty code repository
   for the CLI. *See:* the seed writes `model-registry.yaml` at the agents-repo
   root (180), `actions.yml` with verify commented out (206 A), and the roles.
   *Write down:* the seeded agents commit and the recorded checkout path.

2. **Author the plan file** `plans/kettle.md`, schema `beekeeper-plan/v1`.
   Copy the frontmatter shape verbatim from `/tmp/kettle-smoke-agents/plans/kettle.md`
   if it is still there, else from `conformance/project-work/README.md` §(a) —
   exactly the keys `schema, id, status, title, code_repository,
   delivery_ref, criteria, retired_criteria`, each criterion exactly
   `id/accept/proof`, `proof` one of `{kind: review}`,
   `{kind: action, name: verify, step: verify}`, `{kind: git-ref}`. Set
   `code_repository` to this project's code repo id and
   `delivery_ref: refs/heads/main`.
   - **Path A — the app.** Projects → **Files** tab: open `plans/`, create the
     file, save it as a draft, then **Commit…** in the drafts panel (225).
     **This is the Files tab's first run against hive** — Andy proved it in the
     dev app against a *local* relay only. Read the commit dialog's verbatim
     host result: `pushed: yes|no|unknown`, and whether the `commit.record`
     published or the Retry banner appeared.
   - **Path B — the CLI/git**, as the two prior runs: clone the agents repo,
     write the file, commit `-s`, push, then `"$BEE" sessions work validate`.
   - **Rule:** try A; if it has not produced a commit within **5 minutes**,
     switch to B and record that A failed and where. Record the path used
     either way — A failing is a finding, not a delay to hide.
   - Validate before the run with `"$BEE" sessions work validate` — read
     `--help` for the exact flags (plan repo, commit, path); the README's
     flag list was once wrong, the binary is the authority.

3. **Type the goal, once. Clock starts.** In the new team session's goal field,
   verbatim, and nothing else:

   > Build the kettle CLI described in the plan, with tests and a verify
   > action, and land it on main. Use the team.

   *Write down:* the minute, the goal event id, the session uuid and the
   session channel uuid. *See:* the bench pre-selected, "Use roles" on, the
   registry resolving from the agents repo — all three were 205's findings and
   all three are fixed (180, 186, 207). Each one that still needs a click is a
   finding.

4. **Hands off while the lead plans and hires.** Seats are **lead, builder,
   verifier on this Mac only** — Andy's machine is not assumed, so §6 claims 3
   and 8 score *not proven*, never pass. *Write down:* the plan published, the
   work declaration adopted (plan commit + path), each assignment (44244, with
   `base_sha`), each hire's disposition, class/risk and routed model, and the
   minute the first seat starts working.

5. **One approval, through the card.** When the verify action's host step wants
   an approval, answer it **in the app's approval card** — inbox row or Actions
   tab — with **Approve and allow future runs of this exact definition**
   (211, 218). *See:* argv rendered one numbered argument per row, the run's
   own `definition_hash`, and the commit the run will test (206 B). *Write
   down:* the minute the request appeared, the minute you clicked, the hash,
   and the run id. If **Approve is disabled**, read the reason the card gives
   and stop — see §4. Do not approve by CLI, by hand-signed 46030, or by
   publishing anything. This click is the only interaction after step 3.

6. **Hands off to terminal.** The verify action runs green on the exact commit;
   the lead reads the result itself and publishes `mission.completed`. *Write
   down:* the landed sha, the run id with its `exit_code`/`checkout.sha`, the
   terminal event id and minute, and whether coverage read complete in the
   Mission panel.

7. **Read the numbers.** §5. Then §6 and §7.

| Step | Clock (UTC) | Observed | Finding? |
|---|---|---|---|
| 1 project created | before the clock | project `kettle-control`; agents repo seeded at `996f9bbd` with `model-registry.yaml`, `actions.yml` (verify commented out) and the roles; checkout path recorded | no |
| 2 plan committed (path A) | before the clock | **Path A worked, first run against hive**: `plans/kettle.md` drafted and committed from the Files tab → commit `53385d6c`, `Beekeeper-Drafts` trailer. `sessions work validate` said `adoptable: false` — no verify action existed at that commit yet, which is expected, not a defect | no |
| 3 goal typed — **start** | **12:48:54Z** | turn `812784c1…` (kind 44220, no wake command id — Brian's own act). Genesis 12:48:11Z, `session.create` 12:48:42Z, lead's first turn 12:48:43Z. Bench pre-selected, "Use roles" on, registry resolved from the agents repo — 205's three findings all held fixed, no click needed | no |
| 4 first seat working | 12:51:12Z (builder) | hire `91b8719e…` 12:51:07Z → host `session.create` `22147cb4…` 12:51:09Z → first turn 12:51:12Z → assignment 12:51:23Z → report 12:54:12Z. 2 m 18 s from goal to a builder working | no |
| 4 hires routed | 12:51:07Z, 13:42:07Z | builder (Claude sonnet, 200k context) and verifier (Claude opus[1m]). Both routed from the agents-repository registry; no `HIRE_NO_ROUTE`. Roster names read "Builder 3", "Lead 3" | **236(d)** |
| — decision request | 12:50:46Z → 13:40:13Z | ruling `c8e72f29` on the seat's missing git identity; answered 49 m 27 s later, by CLI on Brian's behalf. Builder parked that whole time with finished code. Never surfaced in the app | **236(a)**, **236(c)** |
| 5 approval requested | 13:41:56Z | `action.trigger` 13:41:55Z → `action.approval_requested` (46010). Never appeared in the inbox; Brian found it in the Actions tab only after being told | **236(e)** |
| 5 approval given | 13:56:37Z | Actions card, **Approve and allow future runs of this exact definition** → 46030 `ff86e639…`, founder key. Card showed argv one row per argument, `definition_hash` `f5f248e93ff4…`, commit `0cbe84e8`, `checkout: required` — all correct. **Brian's latency: 14 m 41 s** | no |
| 6 landed on main | by 13:41:55Z | `0cbe84e8`, the commit the trigger and the card both name. Verifier hired 13:42:07Z, report 13:43:25Z, **NOT-REFUTED** verdicts 13:43:52/53Z against an archive-of-commit test run | no |
| 6 verify ran green | 13:57:25Z | `action.claimed` 13:57:24Z (47 s after the grant), result 46023 `fd05dc2d…`: `exit 0`, 19 tests, 756 ms, `checkout.sha 0cbe84e8`. **It did not wake the lead** — no 44220 attempted, nothing in the provider log; 6 m 29 s until Brian nudged at 14:03:54Z | **236(g)** |
| 6 terminal record | 14:04:15Z | `mission.completed` `35b6e791`, kind 44244, `completed --without-coverage`. Coverage is absent because `sessions work adopt` was refused by the relay at 13:44:04Z (twice) and no 44249 record exists. The completion names its own three follow-ups | **236(f)** |

*Approval requested → approval given* is **Brian's** latency (55 of 205's 70
minutes were this); everything else is the system's. Report them apart.

## 4. The operator rule, and what is known open

**Rule.** If one of the known-open defects below fires: **STOP.** Record the
minute and the receipt or event id. No repair, no hand-fix, no CLI workaround,
no relaunch — a stopped run is a result and is publishable. Any *other* stop,
or any hand-fix, is a finding numbered in the ledger the same day (§7).

**Known open — excluded from this run's claims.**

- **R2 — seat custody under restart/abort.** A seat can be acquired while a
  prior actor's processes are alive. Astra's fourth look,
  [2026-09-21-astra-wave2-fourth-look.md](2026-09-21-astra-wave2-fourth-look.md);
  ruling A9.1; lane **229** in flight.
- **R4 — durable answers on crash.** A held wake can be retired as delivered
  when its answer was never staged. Same review; ruling A9.2; lane **230** in
  flight.
- **A webhook-triggered action cannot be approved from its card** (ledger 218):
  its stored hash covers a secret the public event does not carry, so the card
  answers `definition_hash: null` and offers no grant. Our verify action is
  `manual`, so this should not bite — if Approve still will not enable, that is
  step 5's stop condition.
- **Plan drift is not disclosed.** A declaration pins its plan commit and the
  fold reads it there, correctly; nothing prints "main moved since the
  declaration". Ruling **A10**; lanes **234/235** in flight. Do not treat a
  moved agents-repo tip as a defect this run.

Nothing else is expected to fail. R2/R4 are *safety* defects and this run makes
no claim about them either way (A11.1).

## 5. Measurement, verbatim

Run at the end, on the bundled `bee`, with `$BEE`, `BUZZ_RELAY_URL` and
`BUZZ_PRIVATE_KEY` exported per P8/P9. Replace `<…>`; times are UTC, the
`--since` a minute before the goal, the `--until` a minute after the terminal.

```bash
"$BEE" sessions measure \
  --channel <session channel uuid> \
  --session-ref <session uuid> \
  --since <YYYY-MM-DDTHH:MM:SSZ> \
  --until <YYYY-MM-DDTHH:MM:SSZ> > /tmp/kettle-control-measure.json

"$BEE" --format compact sessions measure \
  --channel <session channel uuid> --session-ref <session uuid> \
  --since <…> --until <…>
```

Read and copy out: turns completed and open; per-seat cache-inclusive input and
output tokens; tool calls; active and waiting minutes; queue→start latencies;
`user_error` occurrence counts; `person_actions` vs
`host_actions_under_founder_key` vs `unattributed_founder_actions` with the
`rule` on each row (206 C — a `session.create` answering a hire is the host's,
not Brian's); and the **`honesty` block**, which prints `unknown` with a reason
for anything the wire cannot support. Quote an `unknown` as unknown.

Then the run's own record and the coverage — `"$BEE" workflows run-status --run
<run uuid>` and `"$BEE" --format compact sessions work status --session <session
uuid>`. Wall clock, from §3's table: goal → first seat working; approval
requested → approval given; last work turn → terminal.

| Measure | This run | Kettle 09-20 (178) | Smoke (205) |
|---|---|---|---|
| Goal → terminal | **4,536 s (1 h 15 m 36 s)** | 20,042 s (5 h 34 m) | ~70 min gross, no terminal |
| Approval wait (Brian's) | 881 s (14 m 41 s), through the card | hand-signed, 46 min | ~55 min |
| Unplanned human acts | **2** (both by CLI on Brian's behalf: the 13:40:13Z ruling answer, the 14:03:54Z wake) + the one planned approval | 6 operator repairs | 0 |
| Work turns | 11 (lead 7, builder 3, verifier 1) | 5 | 5 |
| Cache-inclusive input | 8,817,612 (lead 4,834,854; builder 3,225,787; verifier 756,971) | 9.25 M | 8.53 M |
| Output tokens / tool calls | 64,012 / 140 | 107k / 155 | 89,811 / 142 |
| Reported cost | **$2.7037 — partial**, 8 of 11 results carry `costUsd`; the honesty block says so and no price table was consulted | $2.30, 3 of 5 unpriced | $2.955081, 2 of 5 priced |
| Closing protocol | none — the terminal was the lead's own turn | +11 turns / +8.2 M | n/a |
| Polling turns | 0 (detector, per the honesty block) | 0 | 0 |
| Acknowledgement-only turns | **0** (`disposition_or_acknowledgement_turns: 0`) — lane 210's rule held | n/a | 0 |
| `user_error` | 0. The only refusal on the wire is the relay `tag-count` 400, classed `relay_error` | n/a | n/a |

Coordination, verbatim: `person_action_count: 5`,
`host_action_under_founder_key_count: 2` (both `session.create` answering a
lead's hire — the host's, not Brian's, per 206 C),
`unattributed_founder_action_count: 1` (the `cli-wake-v1:` wake at 13:40:13Z).
Of the five person actions, two are the session create and the goal, one is
the approval, and two are the interventions counted above.

Quote the honesty block as it stands: dollar cost is **partial**; host notices
vs typed prompts is **unknown** on this build; orientation, polling and waiting
are **detectors**, not facts the producer asserted.

## 6. Scorecard — the eight vision claims (09-20 runbook §5)

Scored pass / partial / fail / not proven, with evidence, never opinion.
Partial is a real grade.

| # | Vision claim | Pass looks like | Result | Evidence |
|---|---|---|---|---|
| 1 | Starting work requires intent, not orchestration expertise | one goal, one start, nothing greyed without a remedy | **partial** | One goal typed 12:48:54Z; bench pre-selected, "Use roles" on, registry resolved — 205(c)–(f) held fixed, no click. But 1 m 52 s in the system asked Brian a question it had the facts to answer (236(a)), and the plan text says decide without asking |
| 2 | Decisions continue the work | ≥1 recorded agent decision, Brian notified not asked | **fail** | `rulings_opened: 1`, `rulings_answered: 1` — and the one ruling was a question *to* Brian, not a decision taken and reported. It surfaced in no app surface at all (236(c)); the Decisions panel read "no fold has run". The work stopped for 49 m 27 s waiting on it |
| 3 | Continue another participant's work | not exercised — single machine, no faults | not proven | No fault was scheduled (A11.1) and no second machine was assumed |
| 4 | Roles evolve with the project | not exercised this run | not proven | Templates 1.2.0 were the seats' text (231), but no role changed during the run |
| 5 | Observe parallel work before the push | one store, one parser; overlap caught before a second push | **partial** | The wire carried it: verifier hired 13:42:07Z alongside the builder, NOT-REFUTED verdicts 13:43:52/53Z bound to `0cbe84e8` with an archive-of-commit run. The founder could not *observe* any of it — Mission, Decisions and Settlement all read "unknown" for the whole run (236(b)) |
| 6 | Deterministic operations first | zero polling turns; wake on the run's completion | **partial** | Zero polling turns and zero acknowledgement-only turns. The decision answer's wake did reach the lead (13:40:13Z, `cli-wake-v1:`), but the host result 46023 at 13:57:25Z woke nobody — no 44220 attempted, nothing in the provider log (236(g)). A person closed the loop |
| 7 | Gates earn their delay | the one approval names what it prevented; no click a grant could have covered | **partial** | The card was right and bound: argv per row, `definition_hash` `f5f248e93ff4…`, commit `0cbe84e8`, `checkout: required`; allow-future-runs offered and taken; the host claimed 47 s after the grant. But the request reached no inbox (236(e)), so all 14 m 41 s of the delay was invisible waiting, not a considered gate |
| 8 | Two machines, one result | not exercised — Andy's machine not assumed | not proven | Seats were lead, builder and verifier on this Mac only |

Plan adoption, the run's largest gap, is not one of the eight claims: the
plan-as-contract mechanism Waves 1–2 built was **never exercised live**,
because the relay refused `sessions work adopt` (236(f)). Nothing in this
scorecard should be read as evidence for it.

## 7. Findings

Filed the same day as **ledger 236(a)–(h)** in
[`SESSION_STATE.md`](../SESSION_STATE.md) — `grep -n '^236\. ' docs/SESSION_STATE.md`
— each with its event ids, times and `file:line`. In short:

1. **236(a)** — the seat has no git identity and the role template turns a
   missing config into a founder question. 49 m 27 s parked. *Lane 239.*
2. **236(b)** — the desktop's strict CSAT receipt reader rejects the relay's
   `projectRef` field, so Mission/Decisions/Settlement read "unknown" for every
   owner-founded team session. Same defect as 204, fixed in the CLI only.
   *Lane 238.*
3. **236(c)** — the founder decision request surfaced nowhere in the app.
   *Closes with (b).*
4. **236(d)** — roster names read "Builder 3", "Lead 3". *Observation.*
5. **236(e)** — the 46010 approval request never reached the inbox; cause not
   isolated, recorded unknown. *Lane 238.*
6. **236(f)** — `sessions work adopt` refused by the relay, `tag-count … this
   one carries 7`; the 7th tag's source is **not isolated**. No 44249 record;
   the plan-as-contract mechanism went unused live. *Lane 237.*
7. **236(g)** — a green host result wakes nobody. *Lane 240, after custody.*
8. **236(h)** — what worked, recorded so it is not re-litigated: the Files-tab
   plan commit against hive; hire → assignment → report → verdict in four
   minutes; a correct, commit-bound approval card; host execution 47 s after
   the grant; zero acknowledgement turns; an honest completion that lists its
   own follow-ups.

No §4 known-open defect fired, and no hand-fix was applied to keep the run
going. The two interventions are counted, not excused.
