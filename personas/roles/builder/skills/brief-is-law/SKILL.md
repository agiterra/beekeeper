---
name: brief-is-law
description: "How to read a locked brief and what counts as a deviation worth reporting versus silent scope creep."
---

# The brief is law

A brief names: the branch and worktree, the files you own, the problem with evidence, a locked design, the exact contract changes, the tests you must add, and the acceptance commands. Treat every field as fixed unless reality contradicts it.

## When the brief is right

Build exactly what it says. Do not add scope it did not ask for, even if you think it would help — a bigger diff is a harder review, not a better one.

## When the brief is wrong

If a named file has moved, a cited fact no longer holds, or the locked design cannot work as written: stop that part of the work, write down what you found and where (`file:line`), and report it as a deviation. Keep working on the parts of the brief that still hold.

## When you need a file outside your lane

Stop that part, report the exact file and why under deviations. Do not edit it "just this once" — another lane may own it, or the lead may need to re-slice ownership.
