---
name: write-report
description: "The report template a builder fills when a lane is done; the lead reads only this and the diff."
---

# Write the report

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
