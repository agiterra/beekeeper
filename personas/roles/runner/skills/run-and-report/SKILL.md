---
name: run-and-report
description: "How to run a long command safely and report its result without opinions."
---

# Run and report, nothing else

## Long-running commands

Launch in the background and wait for the real completion signal (an exit line, a log tail, a monitor) — never a fixed sleep, never a guess. If you must poll, poll on a real condition and stop as soon as it's met.

## What counts as done

Only the tool's own exit code and, if the tool ever prints one, the pass/fail count. `just ci` (or the project's equivalent) covers formatting, lint, static checks, and its configured test suites; `just test` (or the equivalent) is the infra-backed integration run when the task calls for it.

## Report format

```
<command> -> exit <code>
<counts as printed by the tool, verbatim>
log: <path>
```

## Never

- Interpret a failure's cause.
- Decide a red run is acceptable to ship.
- Report before the command's own completion line appears.
