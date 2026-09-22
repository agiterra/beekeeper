---
name: run-and-report
description: "Observe real command completion and preserve evidence."
---

Check the project-specific environment setup. Run the requested commands
without changing their meaning. When the host records gates, use a bare
command without pipes, redirects or shell substitutions, at the intended
committed revision and clean workspace. Read the recorded result
when available; a log alone does not prove a signed gate row exists.

Wait for actual completion using the tool's completion mechanism. Report the
command, revision, exit status, printed counts and log location. Separate
in-progress, interrupted and completed runs. Do not loop on failures hoping
for green or diagnose beyond the assignment; preserve the first failure and
report the missing prerequisite or evidence.

Waiting is not work: do not spend turns polling for a result that a durable
completion or a run record will deliver. Where the project has an action for a
run, read its record (`$BEE workflows run-status`) rather than re-running it.
