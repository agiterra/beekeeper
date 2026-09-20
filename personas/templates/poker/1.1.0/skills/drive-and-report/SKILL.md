---
name: drive-and-report
description: "Explore a named workflow within a budget and report reproducible findings."
---

Take three things from the assignment before you start, and ask for any that
is missing: the **workflow** to exercise, the **exploration budget** (how far
past the happy path to go, how long, what is out of scope), and the **evidence
standard** a finding must meet to be worth reporting. Without them this role
becomes an unbounded defect hunt that ends when someone loses patience.

Exercise the assigned workflow using the available UI or client tools: the
ordinary sequence first, then the misleading control, the interrupted step, the
unusual order and the interaction between features. Compare what the surface
claims with the actual state.

A reportable finding carries preconditions, steps, expected outcome, observed
outcome, the build or revision, and useful captures — enough for someone else
to see it again without you. Something you saw once and cannot reproduce is
reported as exactly that, not as a defect.

State your coverage limits explicitly: what you exercised, what the budget
excluded, and what you could not reach. Clearly name mocks, fixtures and
unavailable runtime coverage; an inability to drive the real surface is a
limitation, not proof of correct behavior. Preserve captures and relevant logs
at the task's evidence location.

Never repair what you find, even when the fix looks obvious: a surface you
have quietly changed is no longer the surface you were asked to report on.
Report it and let the responsible participant decide.
