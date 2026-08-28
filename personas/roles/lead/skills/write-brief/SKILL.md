---
name: write-brief
description: "The locked brief template a lead fills for every lane before dispatch."
---

# Write a brief

A brief is law until you change it in writing. Fill every field; leave nothing implicit.

```
LANE <id> — <one line>
Tier: 0/1 | 2 — because it touches <what>.
Branch: <topic>/<lane>, worktree from local main @ <sha>.
Owns (exclusive): <paths>. Must not touch anything else; if it needs to, STOP and report.
Problem, with evidence: <2–3 file:line entry points, or reproduced output>.
Ledger: §3 Next, plus items <numbers> — read those and nothing else of SESSION_STATE.md.
Design (LOCKED): <decisions, numbered>. Deviations need a written reason in the report.
Contract changes: <exact wire/type deltas, with the doc that must change>.
Seat: <model + thinking level, and why> — see the choose-model skill.
Tests you must add: <named>. Watch each fail before the fix where a defect is claimed.
Acceptance: <commands with expected counts / exit codes>.
Out of scope: <named temptations>.
Report format: see the write-report skill on the builder pack.
Dispatch: bee sessions send --channel <channel-uuid> --session-ref <umbrella-uuid> \
  --to <role> --content -
```

## Evidence is entry points, not an exploration

Two or three `file:line` pointers is the whole evidence budget. **Time-box it: one read per file you name, and no grep sweep before the first brief.** A lead spent three and a half minutes exploring before writing brief one, produced no better a brief for it, and did the builder's job while doing it. If you cannot name an entry point after one read, that is the missing input — say so in the brief and let the lane find it.

## Name the ledger items; never send a lane at the whole file

`docs/SESSION_STATE.md` is ~3,700 lines. A seat told to "read the ledger" spends
about a quarter of its context before it starts, and a codex seat is already
~25% used at boot (ledger item 80f). Cite the numbered items the lane actually
needs and let it read §3 Next plus those:

```
grep -n '^79\. ' docs/SESSION_STATE.md               # where the item starts
sed -n '<start>,<start+100>p' docs/SESSION_STATE.md  # read that window only
```

If you cannot say which items a lane needs, that is a brief you are not ready
to write — not a licence to hand over the whole file.

## The dispatch line is part of the brief

End every brief with the exact command that dispatches it, `--session-ref` included. A role slug is unique only inside one umbrella, so `--to lead` without `--session-ref` is refused by the CLI — a brief that omits it is a brief nobody can send.

## Rules for a good brief

- Exclusive file ownership per lane — two lanes never own the same file.
- Name the tier and why; tier-2 briefs must name what makes them tier-2 (provider runtime, custody/keys, relay ingest, durable state).
- Name the seat's model and the reason in one clause. "Sonnet, because this is a two-file mechanical edit" is a reason; "Sonnet" is not.
- A brief that turns out wrong on the ground is a report back from the lane, not a licence for the lane to improvise past it.
