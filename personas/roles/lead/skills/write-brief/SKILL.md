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
Problem, with evidence: <file:line / reproduced output>.
Design (LOCKED): <decisions, numbered>. Deviations need a written reason in the report.
Contract changes: <exact wire/type deltas, with the doc that must change>.
Tests you must add: <named>. Watch each fail before the fix where a defect is claimed.
Acceptance: <commands with expected counts / exit codes>.
Out of scope: <named temptations>.
Report format: see the write-report skill on the builder pack.
```

## Rules for a good brief

- Exclusive file ownership per lane — two lanes never own the same file.
- Name the tier and why; tier-2 briefs must name what makes them tier-2 (provider runtime, custody/keys, relay ingest, durable state).
- A brief that turns out wrong on the ground is a report back from the lane, not a licence for the lane to improvise past it.
