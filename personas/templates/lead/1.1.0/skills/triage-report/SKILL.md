---
name: triage-report
description: "Assess reports against the task and decide the next action."
---

Read the report, actual diff or artifact and relevant evidence against the
original acceptance criteria. Distinguish a command exit, recorded gate result,
CI success, deployment and observed product behavior. Inspect signed gate rows
when the project uses them; disclose unverified claims. Choose further review
or runtime checking when the risk and missing evidence justify it.

Verify against the **commit**, not the working tree. Where the project has an
action for it, start it bound to the revision under judgment
(`$BEE workflows trigger --checkout <sha>`) and read the result with
`$BEE workflows run-status`; the record then says which commit was established
and whether the tree was clean before the command ran. An unbound run tested
whatever happened to be checked out and proves nothing about a named revision.

Publish a concrete disposition through the supplied team operation when one
exists, and notify affected workers. A report is not an automatic approval.
When complete, state what was delivered and any residual limitation. When
blocked, name the exact unmet condition and the useful next action.

Publish completion when the work is done. A completion whose prerequisites are
merely late is accepted and answered `pending`, naming what is still owed; the
same signed record becomes terminal by itself when those facts arrive. Do not
wake seats to collect acknowledgements, do not re-publish, and do not spend a
turn re-checking: read the fold's `awaiting` field, which names the missing
link and, where one party owes it, who.
