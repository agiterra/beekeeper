---
name: run-and-report
description: "How to run a long command safely and report its result without opinions."
---

# Run and report, nothing else

The transcript is not the record. Activate hermit first, on a line of its
own, then run every required gate as its own bare command — a pipe, a
redirect, a `$(…)` or a trailing `; echo` of `$?` gets no observed row at all,
silently (live-run findings 57 and 77; the shape is in `skills/push-your-lane`
§ Gates the host can see). End every assignment with
`bee sessions report --channel <uuid> --session-ref <uuid> --genesis <hex64>
--body @report.json`, naming `headSha` and `branch` in the body — a founder's
goal that says "report" means the wire, not chat (live-run finding 62).

## Long-running commands

Run the gate bare and in the foreground when the tool's window allows it — that is the only run the host records a row for. When it cannot finish inside the window, launch it in the background with its output in a file, wait for the real completion signal (an exit line, a monitor) — never a fixed sleep, never a guess — and say in the report that the background run produced **no observed row** (live-run finding 77). If you must poll, poll on a real condition and stop as soon as it's met.

## What counts as done

Only the tool's own exit code and, if the tool ever prints one, the pass/fail count. `just ci` (or the project's equivalent) covers formatting, lint, static checks, and its configured test suites; `just test` (or the equivalent) is the infra-backed integration run when the task calls for it.

## Report format

```
<command> -> exit <code>
<counts as printed by the tool, verbatim>
log: <path, or "the tool result" for a bare foreground run>
observed row: yes (bare) | no (wrapped or background)
```

## Never

- Interpret a failure's cause.
- Decide a red run is acceptable to ship.
- Report before the command's own completion line appears.
