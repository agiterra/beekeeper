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

## Copy every capture out before you run anything else

Playwright wipes `test-results/` at the start of the next run, so a capture
left there is gone the moment you run one more command — including the re-run
you do to check one detail. **Copy the files out first**, into the folder your
report will cite (in this repo: `docs/design/<feature>/<walk-or-review>/`), and
cite the copied path. Never cite a `test-results/` path: a finding whose
screenshot no longer exists is a finding nobody can check.

Hash the set before you use it (`shasum -a 256 <dir>/*.png`) — identical hashes
mean two captures caught the same pixels, not two states.

## When you cannot drive the real app, name the instrument you used

Say it before the first finding: what stopped you (no built app on this
computer, the only build is the operator's live one, the state needs a two-hour
run) and what you used instead. Then make the substitute as close to real as it
can be. The strongest version so far replayed **the umbrella's own signed relay
events** through the E2E mock bridge, so the pixels came from events the wire
actually carried rather than from fixtures — see
`docs/design/singularity/WALK-2026-08-29.md` §0.

A finding from a substitute instrument is still a finding. A finding that does
not say which instrument produced it is a claim about the real app that nobody
made.

## Reporting a finding

```
<surface/control> claims: <what the UI says>
actually: <what is true underneath>
screenshot: <path>
```

## Never

- Claim a finding you did not visually confirm.
- Present a mock-bridge or replayed capture as the real app without saying so.
- Post a screenshot to a host that isn't the project's sanctioned one for this purpose.
- Fix the bug — hand the finding to a builder.
