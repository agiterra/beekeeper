# Independent research brief: durable coding-session continuity

## Purpose

Investigate how Buzz should preserve useful coding-session continuity when work
moves between executions, processes, machines, providers, or models.

Approach this as an independent product and architecture investigation. Do not
assume a preferred storage mechanism, transport, retrieval design, or provider
integration. The goal is to establish what the problem actually requires,
learn from comparable products, and recommend the smallest design that meets
the evidence.

This is a research task, not an implementation task. Do not modify product
code.

## Epistemic standard

- Inspect the actual code. Support material claims with `file:line` citations.
- Treat comments, plans, handoffs, commit messages, and agent completion reports
  as leads rather than proof; verify behavior at the implementation boundary.
- Separate observed facts, reasonable inferences, unknowns, and recommendations.
- Prefer primary sources for external product behavior. Clearly label behavior
  that cannot be verified from public documentation or code.
- If an existing assumption is wrong, say so directly and explain what the code
  proves instead.

## Product problem

Buzz models a coding session as a durable collaborative unit of work. A session
has an identity, project/channel scope, transcript, goal, name, participants,
and one or more provider executions. The durable session is intended to outlive
any particular provider process.

Users expect to be able to leave work for hours, days, or weeks and return to
it. They may return:

1. through the original provider process on the original machine;
2. after that process or the desktop application has restarted;
3. through a new execution from the same provider;
4. through a different provider or model;
5. from another authorized machine; or
6. as another authorized project member.

Those cases are currently easy to conflate, but they do not necessarily have
the same context, security, fidelity, or portability properties.

The core question is:

> How can a new or returning execution become usefully oriented to the durable
> session without pretending it possesses provider-native context that it does
> not actually have?

## Observed user-facing failure

A user created a session, established prior conversation history, then added a
new provider execution to the same durable session. The user asked the new
execution to identify its continuity mode, summarize completed and open work,
and recover one earlier decision without guessing.

The execution responded that no prior working session was visible and that it
could not identify completed work, open work, or an earlier decision. From the
user's perspective, the durable session and transcript were visible in Buzz,
but the newly attached agent behaved like an unrelated fresh conversation.

This is the gap to explain and solve. Do not assume the visible transcript, the
provider's native conversation state, and the context available to a model are
the same thing.

## Important distinctions to prove in code

Determine the actual lifecycle and persistence boundary for each of these:

- durable Buzz session identity;
- signed transcript and lifecycle facts;
- provider execution identity and generation;
- provider-native conversation/session identifier;
- in-memory agent state;
- machine-local persisted state;
- project membership and session authority;
- user-facing labels such as fresh, resumed, loaded, reconnected, or continued.

For every boundary, identify who writes it, where it lives, who may read it,
how long it survives, whether it is portable, and what proves its integrity.

## Empirical scale datum

One deliberately small test session named `ReplayTest2` produced only eight
structured history items. Its complete, untruncated structured representation
was 13,646 bytes. Treat this only as a scale datum, not as evidence for any
particular design. Establish what grows with turns, tool calls, reasoning,
attachments, parallel executions, and time.

## Constraints and invariants to verify

Confirm these against the code and governing design documents rather than
treating them as automatically true:

- A durable session can outlive an execution.
- A replacement execution must not be described as a native continuation when
  it is not one.
- Provider credentials, signing keys, opaque native cursors, and private host
  paths must not enter shared session history.
- Shared history is subject to channel/project access control and session
  authority rules, including use by authorized non-founders.
- Signed provider facts establish provenance, not necessarily objective truth.
- Missing or incomplete history must be disclosed rather than silently treated
  as complete.
- The design must remain useful for sessions that accumulate history over
  weeks, not merely a two-turn demonstration.
- A failure to recover continuity must leave a usable fresh execution and must
  be visible to the user.

Identify any additional invariants the current system already imposes.

## Codebases and products to investigate

### Local code

- Buzz: `/Users/brian/Projects/buzz`
- T3 Code: `/Users/brian/Projects/t3code/t3code`

Read each repository's contributor instructions before inspecting or running
anything. Keep all investigation read-only.

### Comparative products

Research relevant continuity behavior in:

- T3 Code;
- Claude Code and Claude desktop coding workflows;
- pi.dev / Pi coding-agent tooling; and
- any other directly comparable system whose implementation or primary
  documentation materially clarifies the problem.

For each product, distinguish durable UI history from actual model context.
Determine what survives a process restart, application restart, machine move,
provider change, and long delay. Do not infer cross-machine continuity merely
because a conversation remains visible in a sidebar.

## Questions the research must answer

1. What does Buzz mean today by a persistent agent, and which parts of that
   persistence are identity, configuration, memory, live native session state,
   or reconstructed conversation context?
2. What exactly allows T3 Code to reopen an old local session after weeks, and
   which parts depend on the original machine or provider's private state?
3. How do the comparative products distinguish reopening, resuming, forking,
   importing, replaying, and starting fresh?
4. What information is required for an agent to be genuinely useful on return:
   full transcript, summaries, decisions, goals, plans, tool results, file
   state, repository commit, working-tree changes, or something else?
5. Which information must be exact, which may be summarized, and which should
   never be transferred?
6. How should continuity work when several executions contributed to one
   durable session or are active concurrently?
7. What authority should a non-founder require to continue the work, and how is
   that authority verified after membership changes or revocation?
8. How should the system behave when history is incomplete, too large,
   unavailable, conflicting, deleted, or only partially authorized?
9. What are the meaningful scalability thresholds for latency, bytes, tokens,
   storage, indexing, and repeated synchronization?
10. Which continuity properties can be provider-neutral, and which inevitably
    require provider-specific support?
11. What should the user interface promise and disclose at every continuity
    level?
12. What experiment would discriminate among the strongest designs with the
    least implementation work?

## Evaluation dimensions

Evaluate candidate designs against at least:

- orientation quality and task usefulness;
- factual fidelity and provenance;
- honest completeness/truncation reporting;
- same-machine and cross-machine portability;
- provider and model portability;
- founder and authorized non-founder behavior;
- privacy, authorization, revocation, and secret exposure;
- startup latency and interactive latency;
- token, bandwidth, storage, and inference cost;
- behavior with long-running and multi-execution sessions;
- offline and degraded-network behavior;
- implementation complexity and operational burden;
- inspectability, reproducibility, and testability;
- failure recovery and user-visible diagnostics.

## Requested deliverable

Produce one evidence-backed report with these sections:

1. **Executive finding** — the most important conclusion in plain language.
2. **Current Buzz behavior** — an end-to-end map with `file:line` evidence.
3. **Comparative findings** — what each relevant product actually persists and
   restores.
4. **Continuity taxonomy** — precise levels that do not overclaim.
5. **Requirements and threat model** — including non-founder and cross-machine
   cases.
6. **Design space** — at least three materially different approaches, with
   tradeoffs and failure modes. Derive these independently.
7. **Recommendation** — the smallest justified design, including what should
   explicitly be deferred.
8. **Proof plan** — focused experiments, fixtures, measurements, and acceptance
   criteria.
9. **Open questions** — facts that remain unverified or require a product
   ruling.

Do not begin from or attempt to validate a presumed solution. Begin from the
observed failure, the durable-session product promise, and the implementation
facts.
