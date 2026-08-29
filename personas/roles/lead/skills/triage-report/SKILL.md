---
name: triage-report
description: "How to read a lane's report and refuter verdict, check the live value yourself, and dispose of it."
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

## The live-value check — mandatory before APPROVE

Before any `APPROVE` or `APPROVE-WITH-NOTES`, **run the lane's acceptance yourself and read the value it produced**: run the binary, hit the wire, look at the field. Not the lane's transcript of having done it — the output, on this host, now.

```
cargo run -p buzz-cli -- <the lane's own acceptance command>   # and read the field
bee events query --kinds <n> --channel <uuid> --format compact # and read the tag
```

This exists because a lead approved a founder column on a clean report and green tests; the column printed the *provider's* pubkey, and the operator caught it. Tests proved the code did what the code did. Nobody had looked at the value.

If the acceptance cannot be run on this host, that is not an approval with a note — it is `BLOCK: missing-input = <what would let it run, and who fetches it>`.

## Tiers

- Tier-0/1 (tests, fixtures, docs, types with no runtime change): your read of the report, a hunk-level look at the diff, and the live-value check are enough to merge.
- Tier-2 (provider runtime, custody/keys, relay ingest, durable state): wait for a refuter's terminal verdict before you merge, no exceptions.

## Verdicts

- `APPROVE` — merge. Only after the live-value check.
- `APPROVE-WITH-NOTES` — merge; the notes are record, not a condition on landing.
- `BLOCK: missing-input = <the one thing, and who fetches it>` — the lane stops here until that input exists.

A report without a command's exit code, a SHA, or a `file:line` did not happen — send it back.

## Publish the disposition

Every verdict goes on the wire before you move on, as one Pulse line:

```
bee pulse update --project <coordinate> --kind milestone --session <umbrella-uuid> \
  --content "<lane> — <verdict> @ <sha> — <next>"
```

`--kind blocker` for a `BLOCK`. The team's ledger is the relay; a disposition that lives only in your context is lost the moment the seat ends.

Cite the `docs/SESSION_STATE.md` item number the disposition settles. That number is what the next seat reads — it reads §3 Next plus the items you cite, never the whole 5,500-line file (ledger item 80f).
