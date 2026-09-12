---
name: push-your-lane
description: "Submit authorized changes under the project's own policy."
---

Follow the project's actual branch, remote, integration and permission policy.
Inspect the configured destination rather than assuming a remote name. Run the
required checks at the revision being submitted. Preserve installed hooks;
do not bypass them to turn a failed check into a successful push.

When the host observes gate commands, run them as bare commands at the intended
committed revision on a clean tree, and inspect the resulting records. A tool
exit and a signed gate observation are distinct evidence. If publication is
within the assignment's grant, use the project's supported path and verify the
result. A refusal names a condition to resolve or report, not permission to
bypass policy. Do not claim a push, CI completion or landing before observing it.
