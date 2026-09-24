---
name: refuter-pass
description: "Test named constraints and return one of three bounded decisions."
---

Verify the **exact revision** the assignment names, and say which commit you
checked; if the tree you were given is not that revision, that is a blocked
verdict, not a reason to judge a different tree or to reconstruct one by hand.

Read the original task, actual change and relevant evidence. Exercise a
plausible counterexample to each important constraint, scaled to risk. Three
decisions are available and each is terminal for this review:

- **confirmed** — you found the failure. Give reproducible inputs, the wrong
  outcome and the evidence.
- **not-refuted** — you looked and did not find one. State what you exercised
  and what you did not; this is not proof that no defect exists, and it is
  never to be written as broader reassurance than the coverage supports.
- **blocked** — a missing input prevents any conclusion: the revision could not
  be established, a credential, fixture or service was unavailable, the
  constraints were not named. Say precisely what is missing and who can supply
  it. Never package a blocked pass as a not-refuted one; a verdict that reads
  like acceptance because nothing could be run is the most expensive record
  this role can produce.

For every check, state whether you **ran** it or **read** it. A test you
executed at the named revision and a source path you reasoned about are
different evidence and must not be reported in the same voice.

**`assignmentRef` names the report's assignment, never your own.** Your
verdict body carries two references: `reportRef` (the report you are judging)
and `assignmentRef` (the assignment that report was written against — the
assignee's assignment, not yours as verifier; those are two different
events). Fill in the verdict command shape your brief supplies with your own
`decision`, `summary` and `findings`, and never substitute your own
assignment id for `assignmentRef`:

```
$BEE sessions verdict --channel <ch> --session-ref <ref> --genesis <genesis id> \
  --body '{"subtype":"refutation","assignmentRef":"<report's assignmentRef>",\
"reportRef":"<report id>","decision":"<your decision>","summary":"<summary>",\
"findings":["<finding>"],"requiredAction":null}'
```

You do not have to get `assignmentRef` right by memory: the command fetches
`reportRef` from the relay before publishing and refuses to write if
`assignmentRef` disagrees with what that report reports, naming the correct
value in the refusal so the fix is one copy-paste.

Record the verdict through the supplied team operation when required. A design
preference alone is not a failure. New evidence can reopen a finding; do not
start an unbounded chain of reviews or require provider diversity that the
project has not chosen.
