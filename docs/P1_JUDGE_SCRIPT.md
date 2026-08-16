# P1 seed-quality spike — Brian's judge script

**What this is:** the judging protocol for gate G1 — the ~1 hour where you
decide whether replayed transcript history produces *real* continuation.
The mechanical steps (export, translate, seed) live in the harness RUNBOOK
(delivered with the P1 harness; path in the orchestrator's report). This
document is only about how to run the judgment and what verdict to record.

**The bet being tested:** that a fresh execution seeded from durable 44225
history continues the session meaningfully better than a pasted summary
would. If it doesn't, B2/B3/B4 shrink to record-only continuity
(SESSION_EXECUTION_PLAN §P1 stop-condition).

---

## 1. Pick the session (5 min)

Criteria — a session that:
- has **15+ turns of real work** (edits, commands, at least one decision or
  change of direction mid-stream);
- **you remember well enough to catch fabrication** — you are the answer key;
- has a concrete, resumable next step that was never done.

Avoid: trivial sessions, sessions that ended cleanly with nothing left to do.

## 2. The three arms (per provider: Claude first, then Codex)

Run all three in the same workspace, same model, same order:

| Arm | Setup | Purpose |
| --- | --- | --- |
| **A — cold** | Fresh execution, no context | Floor. 2 minutes: ask Q1–Q2 only. |
| **B — pasted summary** | Fresh execution; paste your own 5-line summary of the session | **The stop-condition comparator.** Ask all five questions. |
| **C — seeded** | Fresh execution seeded via the harness package (initial prompt path; also `session/load` if the runbook says the adapter supports it) | The bet. Ask all five questions. |

Write your 5-line summary for arm B *before* looking at the package, so the
comparison is honest.

## 3. The five questions (score each 0 / 1 / 2)

1. **Orientation** — "What is this session's goal, and what has been
   accomplished so far?" (2 = accurate and specific; 1 = vague but right;
   0 = wrong or invented.)
2. **State** — "What was the last thing being worked on, and what is the
   immediate next step?"
3. **Buried detail** — ask one question whose answer sits mid-history (a
   specific file changed, an error hit, a decision made and why). Pick it
   before the runs. (2 = retrieved correctly; 0 = fabricated. Confident
   fabrication is worse than "I don't know" — score 0 and flag it.)
4. **Attribution** — "Who decided X — me or the agent?" for one real
   decision. Also watch: does it treat replayed history as *history*? If it
   starts executing a stale instruction from the transcript as if it were a
   fresh command, that is a red flag regardless of score.
5. **Continuation** — give it the real next task: "Continue: <the actual
   next step>." (2 = proceeds using context — right files, right
   conventions, no re-asking for things the history contains; 1 = works but
   re-derives or re-asks; 0 = starts over or goes wrong.)

Q5 is the money question. If you only have time for one thing per arm, do Q5.

## 4. Red flags (note independently of scores)

- Confidently fabricated specifics.
- Executing stale instructions from the replayed history.
- Claiming prior work as its own in a misleading way ("I already ran the
  tests" when a *previous execution* did) — attribution-≠-verification.
- Package truncation dropping exactly the context Q3/Q5 needed (that's a
  packaging finding, not a model finding — note which policy dropped it).

## 5. Verdict (record per provider)

- **PASS** — arm C scores ≥8/10 **and** beats arm B by ≥2 points, with no
  stale-instruction red flag → G1 passes for that adapter; B2
  (productized seeding) is worth building.
- **STOP-CONDITION** — arm C within 1 point of arm B (or worse) → the
  plan's stop line trips: seeding adds little over a summary. B2/B3/B4
  shrink to record-only continuity and you decide the fork story.
- **ITERATE** — middle ground, or a packaging-caused failure: one package
  format revision is allowed inside the spike; rerun arm C once.

Also record: package size vs the 32 KiB reality, truncation applied,
delivery path used per adapter (initial prompt vs `session/load`), wall
time, and anything that surprised you. That plus the two verdicts is the
complete P1 completion report.

**One honest caveat:** you wrote arm B's summary knowing the session, so
arm B is a *strong* comparator — a human-quality summary. That is the
right bar: automated seeding is only worth building if it beats or matches
what a departing operator would have written by hand, because in the
machine-death case nobody wrote one.
