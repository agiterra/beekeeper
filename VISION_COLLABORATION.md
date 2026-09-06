# Beekeeper — work continues

Product direction settled with Brian on 2026-09-06. This is intended behavior;
implementation and validation status belong in `docs/SESSION_STATE.md`.
The delivery sequence is `docs/COLLABORATIVE_WORKSPACE_PLAN.md`.

## Purpose

Beekeeper is a workspace where humans and agents collaborate on equal ground.
The project owns the durable working record: intent, decisions, responsibility,
artifacts, role procedures, execution evidence and outcomes. A person, model,
session or machine can become unavailable without making authorized work stop.

Software development is the first proving ground. The product's purpose is
coherent completed work, not maximum code production, agent count or token use.
More workers should increase useful throughput without multiplying conflicting
implementations or the time humans spend coordinating them.

## Starting work should require intent, not orchestration expertise

The normal start asks what the person wants accomplished and, when not already
known from context, which project it belongs to. Beekeeper resolves the saved
project setup and available authorized capabilities. It shows a short, concrete
summary and a clear start action. A single subscription is sufficient.

Agent identities, model routing, reviewer sampling and execution budgets are
optional advanced configuration, not a prerequisite for asking for help.
Remember project choices rather than asking each session to repeat them. Show
the effective choice instead of ambiguous "Not set" values. Missing setup gets
one actionable explanation beside the start action, not an unexplained disabled
button. Never imply automatic discovery or configuration succeeded without
evidence.

Preserve access to advanced controls without making everybody interpret them.
Explain their consequences in ordinary language; technical enforcement details
belong in inspectable diagnostics. An advisory preference must not look like an
enforced limit. Simplifying the screen must preserve this distinction.

## Decisions continue the work

Within delegated authority the protocol is **consult, decide, continue,
notify**. An uncertain agent investigates and, when useful, asks a fresh agent
with the relevant competence. A consequential structural disagreement can go
to a stronger cross-provider reviewer with authority to resolve it. Reviewers
receive the original task and evidence, not only an advocate's summary.

An authorized agent can make a binding decision. Review is not human by
default, and ordinary reversible work does not wait for a human response.
Record the decision, reasons, evidence, effects and reconsideration trigger so
others can act on it and Brian can redirect without being a dependency.

Uncertainty is different from missing authority. An additional model cannot
grant access to another person's resources, increase a delegated spending
limit, or authorize destruction of unpreserved work. Such limits are explicit
project policy, established ahead of time. Independent work continues while a
specific out-of-scope action awaits the principal who can authorize it.

Prefer the smallest coherent solution supported by the facts. Preserve an
inexpensive reversal path. Reversal costs include dependent work and external
effects; a git revert does not undo every consequence.

## Gates must earn their delay

Routine work proceeds under standing grants and automatic checks. Every
blocking check names the concrete failure it prevents, its scope, the evidence
that releases it and why detection/recovery or an asynchronous check is
insufficient. A second agent is not required for every action. Different model
providers can improve scrutiny but do not prove correctness or independence.

One subscription is a complete supported configuration. Choose reviewers from
the capabilities and budget actually available: deterministic checks first,
then a fresh same-model or same-provider context when review helps, with
cross-provider review as an optional improvement. Never stall routine work,
silently purchase another service, or require a human because a second
provider is unavailable. Record the review basis and its limits honestly. Any
exceptional stricter review requirement must be an explicit project choice,
not a built-in assumption that everybody has multiple subscriptions.

Review has a defined jurisdiction and a terminal disposition. Disagreement
goes to a designated adjudicator, not an unbounded chain of reviewers. New
evidence can reopen a decision; repeated anxiety alone does not create a gate.
Security checks enforce real authority boundaries without turning legitimate
delegation into a request for a human click.

## Observe parallel work before the push

An active work item exposes its goal, acceptance criteria, dependencies,
responsible participant, claimed scope, branch/base and latest checkpoint.
Claims coordinate responsibility; they do not grant permissions. Changes to
ownership are explicit and observable.

Detect overlapping intent as well as overlapping files. Two workers can touch
different files and introduce duplicate product mechanisms. Agents resolve
semantic overlap early against the product contract; deterministic machinery
tracks claims, dependencies, revisions and integration order. A merge without
conflicts is not proof of a coherent product.

Integrate against the actual current base, retain rejected alternatives as
history and avoid preserving two near-identical implementations just because
both were already written. Do not serialize unrelated work behind one agent.

## Continue another participant's work

Responsibility can move to another authorized participant. If the existing
execution is reachable and its resource controller permits it, continue there.
Otherwise reconstruct from durable checkpoints, artifacts and evidence on an
authorized body. Disclose local-only work that could not be recovered.

A checkpoint includes the accepted task, decisions, artifact references,
current revision, tests, unresolved questions and next useful action. It does
not require private model reasoning. Takeover changes the execution claim and
fences the old consumer; a returning machine must not silently resume competing
work. Native session continuation and reconstruction are distinct outcomes.
The returning participant sees who continued, what changed and what landed.

## Roles evolve with the project

A role is a project-qualified responsibility and its versioned procedures,
tools, constraints and expected evidence. An agent is a persistent participant
that can fill roles. An execution runs a particular version of a role. These
are different facts; a home-role preference proves neither capability nor work.

The role lifecycle is discovery, evidence-backed correction, authorized
validation, publication, notification and observed adoption. Validation may be
performed by software or authorized agents. It does not imply human approval.
Publish once and make every affected machine learn the change before relevant
new work, rather than paying to rediscover CI/CD procedures on each machine.

Keep the project source, locally resolved artifact, staged execution revision
and actual adoption evidence distinct. Git publication is not adoption; a
staged hash alone does not prove successful execution. Use existing pack
sources and execution provenance rather than a competing role registry.
Separate project procedure from machine-specific credentials and configuration.

An active execution does not silently change instructions. Routine updates
apply at the next execution boundary; incompatible changes require a named
checkpoint/restart policy. Revocations are enforced at affected operations.
Offline participants retain identity and history, and resolve the current role
before their next affected task. A successful local workaround is a proposal
for shared learning until its broader applicability has been validated.

## Deterministic operations first

Software subscribes to events, tracks CI, correlates commits and runs, records
exit results, maintains leases, deduplicates delivery, schedules eligible work
and notifies the waiting participant. No model turn is spent asking repeatedly
whether a known process finished. A bounded software poll is acceptable when
an external system offers no event stream; it does not require model polling.

A small specialist may classify a failure; stronger judgment resolves an
unfamiliar architectural question. Escalation is driven by evidence and task
requirements, not by permanently assigning an expensive model to every role.
Completion events distinguish command exit, CI success, deployment and product
acceptance, each tied to the exact revision and authorized observing source.

## Agents are participants in the conversation

An agent can be addressed in project chat, accept work, consult colleagues,
report evidence and hand over responsibility. Membership, permission to
consume resources, conversational availability and execution are separate.
Neither an offline body nor an empty lease erases the agent's identity.

The channel is a first-class work surface. A participant can mention an agent,
ask for status, assign work and continue the discussion with teammates in the
same thread. Specialized session views add detail; ordinary interaction does
not require leaving the channel. Relevant decisions, handovers and outcomes
return to that conversation with links to their evidence.

Adding an agent does not subscribe the channel to every internal tool call or
make the agent respond to every message. Participation rules define when it
responds. A direct request gets a visible disposition: accepted, queued,
continued elsewhere or unable to proceed, with an actionable reason where
available. Humans and authorized agents can address it under the same project
policy; its owner's presence is not a routine requirement.

Chat work survives a missing body through relay-durable delivery and explicit
dispositions. Do not claim that received means completed, that silence means a
machine is off, or that a replay guarantees exactly-once external effects.
Revalidate current authority when consuming pending work and publishing effects.

## Measure useful velocity

Track completed work and product regressions alongside coordination delay,
time waiting for human decisions, repeated orientation, review cost, duplicate
implementations, stale role use and recovery time. Surface attribution and
freshness, and permit unknown answers. Do not turn these measures into quotas
that reward meaningless activity.

The acceptance story is two people on separate machines with their agents:
they see overlapping work early, share an updated Runner procedure, continue
an absent colleague's slice, receive CI completion without model polling, and
return to one coherent result with an inspectable history.
