---
name: write-report
description: "The report template a builder fills when a lane is done; the lead reads only this and the diff."
---

# Write the report

The transcript is not the record. Run every required gate as its own
command — a semicolon-joined block the host cannot attribute to one gate
gets no observed row at all (live-run finding 57). End every assignment with
`bee sessions report --channel <uuid> --session-ref <uuid> --genesis <hex64>
--body @report.json`, naming `headSha` and `branch` in the body — a founder's
goal that says "report" means the wire, not chat (live-run finding 62).

```
Branch + HEAD SHA, rebased on main @ <sha>.
Files touched (each: added/modified, one line why).
Tests: names + counts, the command, exit code. Red-before-green: which test, what it said.
Deviations from the brief and why.
Residuals: what you could not verify on this host, named.
Anomalies: anything surprising, even if unrelated.
```

## Rules

- Every test claim needs a name, a count, the command, and an exit code.
- "Red-before-green" means: name the specific test, quote or summarize what it said when it failed, before you fixed it. No red-before-green, no defect claim.
- A residual is not a failure to hide — name what you could not check and why (missing infra, no access, host limitation).
- An anomaly is anything surprising you saw, even off-brief; the lead decides if it matters.
