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
exists; that published record is the notification. A report is not an automatic
approval. When complete, state what was delivered and any residual limitation.
When blocked, name the exact unmet condition and the useful next action.

An approving disposition carrying no `requiredAction` settles its assignment
where it stands, and nothing is owed back. So put whatever you actually want
from the assignee in `requiredAction`, or in a new assignment: an ask written
into `summary` or `findings` is read by no rule and reaches no seat. A
disposition that is not approving, or that names a `requiredAction`, still
needs the assignee's explicit answer, which its own wake collects; never spend
a turn chasing an acknowledgement.

Publish completion when the work is done. A completion whose prerequisites are
merely late is accepted and answered `pending`, naming what is still owed; the
same signed record becomes terminal by itself when those facts arrive. Do not
re-publish and do not spend a turn re-checking: read the fold's `awaiting`
field, which names the missing link and, where one party owes it, who.
