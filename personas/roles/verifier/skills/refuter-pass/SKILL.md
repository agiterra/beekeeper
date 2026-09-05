---
name: refuter-pass
description: "How to run a one-pass refutation against a brief's named constraints and phrase a CONFIRMED verdict."
---

# One pass, against named constraints

The transcript is not the record. Activate hermit first, on a line of its
own, then run every required gate as its own bare command — a pipe, a
redirect, a `$(…)` or a trailing `; echo` of `$?` gets no observed row at all,
silently (live-run findings 57 and 77; the shape is in `skills/push-your-lane`
§ Gates the host can see). End every assignment with
`bee sessions report --channel <uuid> --session-ref <uuid> --genesis <hex64>
--body @report.json`, naming `headSha` and `branch` in the body — a founder's
goal that says "report" means the wire, not chat (live-run finding 62).

Read the brief's constraints first — the "Refuter constraints" line a lead writes for tier-2 work. Then read the diff once, straight through, hunting only for ways those named constraints break.

## Phrasing CONFIRMED

`CONFIRMED: <inputs/state> -> <wrong outcome>`

Example: `CONFIRMED: two turns queued while the provider is killed -> the second is answered twice on restart`.

Make it reproducible: state exactly what you did or would do to trigger it, and what happened or would happen instead of the constraint holding.

## Phrasing NOT-REFUTED

`NOT-REFUTED` — plain, no hedging. You tried against every named constraint and none broke.

## Out of bounds

- A design you would have made differently is not a finding.
- A style nit is not a finding.
- Anything outside the brief's named constraints is not your pass to make — name it to the lead as a separate note if it matters, but it does not change your verdict.
