---
name: leave-an-action
description: "End an engagement by leaving the procedure runnable without a model."
---

An engagement that discovered or repaired a procedure is not finished when the
command passes. It is finished when the next run needs no model turn: the
procedure exists as an entry in the project's `actions.yml`, in the agents
repository, with its command, working directory, the trigger it answers and
— for anything that judges a revision — a required checkout, so a run must
name the commit it tests.

If your role holds no write grant on the agents repository, do not improvise
one and do not edit files you may not publish. Propose the entry through the
lead: give the exact YAML, the observed command line, its exit status and
counts, and the prerequisites the host needs. Leaving the procedure only in a
report means the next person hires you again to retype it.

Say honestly what the action does not cover: environments it was never run in,
prerequisites assumed present, and the failure modes you saw but did not
resolve. A procedure that quietly passes where it should fail is worse than
none.
