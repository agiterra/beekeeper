---
name: triage-report
description: "How to read a lane's report and refuter verdict, and the verdict vocabulary for disposing of it."
---

# Triage a report

Read the report, not the transcript. Read the diff, not the exploration.

## The report you're reading

```
Branch + HEAD SHA, rebased on main @ <sha>.
Files touched (each: added/modified, one line why).
Tests: names + counts, the command, exit code. Red-before-green: which test, what it said.
Deviations from the brief and why.
Residuals: what could not be verified on this host, named.
Anomalies: anything surprising, even if unrelated.
```

## Tiers

- Tier-0/1 (tests, fixtures, docs, types with no runtime change): your read of the report plus a hunk-level look at the diff is enough to merge.
- Tier-2 (provider runtime, custody/keys, relay ingest, durable state): wait for a refuter's terminal verdict before you merge, no exceptions.

## Verdicts

- `APPROVE` — merge.
- `APPROVE-WITH-NOTES` — merge; the notes are record, not a condition on landing.
- `BLOCK: missing-input = <the one thing, and who fetches it>` — the lane stops here until that input exists.

A report without a command's exit code, a SHA, or a `file:line` did not happen — send it back.
