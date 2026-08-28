---
name: drive-and-report
description: "How to drive the real built app and produce evidence for an honesty finding."
---

# Drive the real app

> Shared with the designer seat: `see-the-app` in the designer pack references
> this procedure rather than copying it. Keep it general enough to hold.

Use the project's own run/screenshot tooling rather than inventing a new path — most projects that need this have one (a screenshot script, an E2E bridge, a preview tool). Find and use it before assuming you need to build your own harness.

## Sequence

1. Get the real app running (built, not mocked, unless a mock bridge is the task's own stated method for reaching this UI).
2. Navigate to the surface named in your dispatch.
3. Trigger the state the finding depends on (disconnect a provider, seed a message, open a menu) rather than describing it from memory.
4. Screenshot the state that shows the gap. Crop to the relevant control — a full-window screenshot the reader has to hunt through is a weaker report.

## Reporting a finding

```
<surface/control> claims: <what the UI says>
actually: <what is true underneath>
screenshot: <path>
```

## Never

- Claim a finding you did not visually confirm.
- Post a screenshot to a host that isn't the project's sanctioned one for this purpose.
- Fix the bug — hand the finding to a builder.
