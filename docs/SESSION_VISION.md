# Sessions: the 2028 collaborative agent surface

## Purpose of this document

This is a product-vision brief for Fable and future contributors working on
Buzz sessions. It describes the experience we want, why it matters, the
qualities that must survive design decisions, and the Git workflow Andy built
for this fork.

It is deliberately **not** an implementation plan. It does not choose event
schemas, component boundaries, storage models, orchestration algorithms, or a
sequence of code changes. Those decisions should follow from the vision and
from careful study of what Buzz already provides.

## The proposition

The session managers we use today are circa 2025. They treat a coding agent as
an upgraded terminal process:

- start a provider session;
- choose a model;
- send prompts;
- watch that provider's transcript;
- manually move context when another provider is needed;
- lose continuity when the process, machine, or provider changes.

That makes the provider session the center of the product and leaves the human
acting as the integration layer.

In 2028, it is still called a **session**, but the word means something larger:

> A session is a durable, shared environment where humans, agents, tools, and
> artifacts work together around the work at hand.

A session is not “a Codex session,” “a Claude Code session,” or “a Grok
session.” Codex, Claude Code, Grok, and future providers participate in the
session. They do not define or own it.

The session belongs to its participants and its history—not to a provider,
model, process, or machine.

## One surface, one provider or many

The same surface must feel natural with one provider and with several
providers working at once.

A person might begin alone with Claude Code, bring in Codex to implement a
piece of the plan, ask another model to challenge the approach, and have the
first agent review the result. This should not require separate tabs, copied
prompts, pasted summaries, or disconnected transcripts.

Within one session:

- humans and agents are identifiable participants;
- more than one provider and model may be active;
- agents may address the user, another agent, a group of participants, or the
  session as a whole;
- agent-to-agent conversation is visible and attributable;
- delegation, review, disagreement, and synthesis are part of the same record;
- every execution can enter or leave without replacing the session;
- a short single-agent interaction remains simple and does not inherit
  multi-agent ceremony.

Provider identity should be visible when it helps explain behavior, capability,
cost, or provenance. It should not fragment the experience into provider-owned
rooms.

## The experience bar

T3 Code, the Codex desktop app, and the Claude Code desktop app are experience
references. The goal is not merely to copy their pixels. The goal is to meet
the standard they set for making agent work legible and controllable.

The session should stream a coherent account of the work, including, when a
provider exposes it:

- human messages and agent responses;
- agent-to-agent messages;
- working status and turn progress;
- plans and changes to plans;
- tool calls, arguments, progress, results, and failures;
- file reads, edits, commands, tests, searches, and other actions;
- diffs and produced artifacts;
- permission and approval requests;
- interruptions, retries, handoffs, and completion states;
- context and usage information;
- reasoning summaries or other provider-supported thought/status information.

Tool use and plans are not debug logs hidden in a separate developer console.
They are part of understanding what the participants are doing. At the same
time, a busy multi-agent session cannot become a wall of raw protocol traffic.
The surface should preserve both:

1. a readable narrative of what is happening and why; and
2. progressive access to the detailed tool activity and provenance underneath.

The composer should provide the controls that are honest for the selected
participant or execution—such as model, effort/reasoning, service tier,
permission mode, send, steer, and interrupt—without pretending every provider
has the same capabilities. Provider-specific strengths should remain available
through a consistent interaction language rather than being erased to the
lowest common denominator.

## Shared by the project

A session is project knowledge, not private state trapped on the computer of
the person who started it.

At minimum, authorized project members should be able to open a session from
their own Buzz client and observe it in real time. They should be able to see:

- why the session exists;
- who and what is participating;
- what each participant is doing;
- tool activity and results;
- decisions and changes of direction;
- produced artifacts;
- blockers and requests for human judgment;
- the lineage of executions that have contributed.

The stronger aspiration is that another authorized member can participate,
steer, or continue the session from their side. That does not require promising
that an opaque live provider process can be teleported between computers.
Continuing the same user-facing session with a new execution is still
continuation: the history, intent, decisions, artifacts, and identity of the
session survive even when the underlying runtime changes.

Exact live-process migration may be a later capability. The product model must
not make it a prerequisite for shared observation or provider-independent
continuity.

Authority should remain explicit. Observing, contributing context, steering
agents, approving actions, changing membership, and taking operational control
are different powers. A shared surface does not imply that every viewer may run
commands against somebody else's checkout.

## Continuity

Closing the view and reopening it should reveal the same kind of session—not a
regular channel that has forgotten it was a coding surface.

Likewise:

- restarting Buzz must not erase the session's identity;
- an execution ending must not end the session by accident;
- changing a model must not create a new user-facing universe;
- bringing in another provider must not require transporting the work by hand;
- a provider becoming unavailable must leave an intelligible record;
- future executions should be able to continue from the durable state of the
  session, subject to access and capability limits.

Internally, providers may have their own runtime sessions, generations,
continuations, and context windows. Those are execution details beneath the
shared session.

## Provider neutrality

Provider neutrality is not a larger dropdown containing several hard-coded
vendors. It is a product boundary.

The arrival of a new provider should not require provider conditionals to be
woven through the session model, transcript, composer, project UI, and trust
model. A provider should be able to describe what it can do, and the surface
should represent those capabilities truthfully.

Neutrality also does not mean flattening all providers into identical text
generators. Models and runtimes may support different planning modes, permission
modes, reasoning controls, continuation semantics, tools, and forms of output.
The shared session should normalize the concepts needed for collaboration while
preserving useful provider-specific capability.

Provider selection is an execution choice. Session identity is not.

## What Buzz already brings

This vision fits Buzz because Buzz is already more than a local chat client.
The assembled product has native foundations that are relevant to the desired
experience:

- one signed event fabric with durable storage, live fan-out, and historical
  queries;
- shared channels, membership, identities, and project access boundaries;
- human and managed-agent identities with provenance;
- projects that group repositories and related work;
- provider-neutral coding-session lifecycle and turn commands;
- provider catalogs that can describe multiple available providers;
- durable coding-session metadata, receipts, and transcript facts;
- normalized transcript representations for assistant output, plans, tool
  calls, tool updates, results, usage, and lifecycle state;
- workflows with run state and human approval concepts;
- Git repositories, patches, issues, statuses, and signed artifacts.

These currently appear as adjacent systems rather than one session experience.
The opportunity is to make them feel like a single shared reality. Existing
contracts are valuable substrate, but none of their present names or boundaries
should be mistaken for the final product model.

## Product invariants

Design exploration may change almost anything except these outcomes:

1. **The session is the primary user-facing object.** Providers and their
   runtime sessions are participants or executions beneath it.
2. **One provider remains effortless.** Multi-provider capability must not make
   the common case feel like orchestration software.
3. **Multiple providers are first-class.** Their output, tools, plans, and
   conversations coexist in one surface.
4. **Agent activity is legible.** Tool calls, plans, progress, failures, and
   results are observable with useful progressive detail.
5. **The record is attributable.** It is always possible to determine which
   human, agent, provider, or tool produced an event or artifact.
6. **The session is durable.** Its identity and meaningful history outlive a UI
   view, provider process, model selection, and individual execution.
7. **The session is shareable under project authority.** Authorized members can
   observe it, and the design leaves room for participation and continuation
   from another member's client.
8. **Capabilities are honest.** The UI exposes controls a participant actually
   supports and does not invent equivalence where none exists.
9. **Sensitive execution state stays protected.** Shared observability must not
   leak credentials, environment secrets, or private machine state.
10. **A future provider does not reshape the product.** Adding intelligence
    expands the session; it does not create another silo.

## What this is not

The intended product is not:

- a tab strip with one independent chat per provider;
- a provider picker placed on the existing single-agent transcript;
- a group chat that hides all consequential tool activity;
- a raw merged log of every protocol frame;
- a mandatory workflow or “mission” ceremony before asking one agent a question;
- a claim that every model exposes private chain-of-thought;
- a promise to migrate arbitrary live processes between computers;
- an excuse to erase provider-specific strengths;
- a session that silently becomes an ordinary channel after navigation or
  restart.

## Experience-level proof

The vision is becoming real when a person can:

1. open one session associated with the work at hand;
2. begin with one provider and interact as simply as in a leading coding-agent
   desktop app;
3. inspect plans, tool calls, changes, and results without leaving the surface;
4. add another provider or agent to the same session;
5. watch those agents exchange work, review one another, and report back in one
   attributable stream;
6. have an authorized teammate open the same session from another client and
   understand it without receiving a separate summary;
7. leave and return without losing the coding-session experience;
8. continue the session through a later execution even if the original runtime
   is no longer available;
9. add a future provider without redesigning what a session is.

This is an experiential test, not a prescribed implementation sequence.

## Questions intentionally left open

Fable should explore these rather than treating this document as an answer key:

- How should a person address one agent, several agents, or the whole session?
- How much agent-to-agent discussion belongs in the primary narrative, and how
  much should collapse into expandable activity?
- How should simultaneous operators and conflicting steering be represented?
- What is the clearest visual relationship among session, participant,
  provider, model, execution, turn, tool, and artifact?
- When should the session summarize activity without losing provenance?
- Which parts of activity are shared, redacted, private to an operator, or
  available only to members with elevated authority?
- What does “continue this session” feel like when the next execution runs on a
  different member's machine or through a different provider?
- How should provider-specific controls appear without making the composer feel
  inconsistent or technical?

The answer should be judged by the product invariants and experience above, not
by fidelity to today's coding-session implementation.

## Andy's Git and integration workflow

This repository is an Agiterra integration fork of `block/buzz`. Andy set it up
linux-next-style so features remain independently maintainable while the daily
product combines them.

### Branch roles

| Branch | Role |
| --- | --- |
| `main` | A pure, fast-forward-only mirror of upstream `block/buzz`. It never receives local feature commits. |
| `feature/<name>` | One upstreamable feature, normally based on `main`. Feature code stays independent and upstream-clean. |
| `integration/glue` | Cross-feature adaptation, fork-only documentation and tooling, integration CI, and `scripts/integrate.sh`. |
| `integrated` | The generated daily product assembled from `main`, the feature stack, and glue. It is rebuilt and force-pushed. |
| `build/YYYY-MM-DD[.n]` | Immutable pins for known assembled builds. |

The current relevant feature branches are:

- `feature/project-containers`;
- `feature/project-access`, stacked on project containers;
- `feature/coding-sessions`;
- `integration/glue`, which owns coupling among those features.

This document lives on `integration/glue` because the vision crosses feature
boundaries and describes fork-specific workflow. Provider/session work that can
stand independently belongs on `feature/coding-sessions`. Adaptation that ties
sessions into project containers, access, or fork-only behavior belongs in
`integration/glue`.

### Important working rules

- Do not commit directly to `integrated`; it is generated and its history is
  rewritten on rebuild.
- Do not put local product changes on `main`; it mirrors upstream.
- Keep upstreamable coding-session changes on `feature/coding-sessions`.
- Keep cross-feature integration and Agiterra-only material on
  `integration/glue`.
- Activate the repository's Hermit environment before Git commands and hooks.
- Sign commits with `git commit -s`.
- An integration rebuild merges the feature stack, rebases the glue patch
  series over the assembly, runs the gate, creates a `build/*` tag, and may
  force-push the assembled branches.
- Consumers of `integrated` re-fetch it or pin a `build/*` tag; they do not
  treat it as a conventional pull-only branch.
- Do not push the current local work merely to checkpoint it. Brian is batching
  fixes to avoid triggering repeated builds; push only when Brian or Andy asks
  for the integration ceremony.

The full mechanics are documented in `docs/INTEGRATION.md` and
`CONTRIBUTING-FORK.md` on the assembled/glue branches.

### Current local handoff context

As of 2026-08-12:

- `/Users/briansweet/agiterra/BuzzForkV2-coding-sessions` is the
  `feature/coding-sessions` worktree;
- that branch has local commits for reliable provider startup and Claude model
  discovery that have not been pushed;
- the current Claude-specific implementation should be treated as a starting
  artifact, not as a constraint on the provider-neutral session vision;
- `/Users/briansweet/agiterra/BuzzForkV2-integration-glue` is the
  `integration/glue` worktree and contains local project/session navigation
  adaptation;
- `/Users/briansweet/agiterra/BuzzForkV2` is the generated `integrated`
  worktree used for running the assembled product and currently contains local
  exploratory changes that must not be overwritten.

Before changing anything, inspect the status of the relevant worktree and
preserve all existing local work.

## The shortest version

> One session, one shared reality, any number of humans and agents. Providers
> supply intelligence and execution; they do not define the workspace. Plans,
> tool calls, conversations, decisions, artifacts, and provenance stream into
> one durable surface that authorized project members can observe and, over
> time, continue from their side.
