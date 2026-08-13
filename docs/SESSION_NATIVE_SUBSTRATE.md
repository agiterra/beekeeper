# Session native-substrate reconnaissance

**Date:** 2026-08-13
**Scope:** reconnaissance requested by `SESSION_HANDOFF_SOL.md` before the next
coding-session implementation step.
**Repositories inspected:** `BuzzForkV2-coding-sessions`,
`BuzzForkV2-integration-glue`, and the local MIT-licensed reference checkout at
`/Users/briansweet/agiterra/t3code`.

## Executive result

Buzz already has most of the durable collaboration substrate the session
surface will need. It does **not** yet have durable provider-runtime
continuity.

The next implementation priority remains resume and generations. The work
should persist a host-private ACP cursor, negotiate the provider's actual ACP
resume/load capability, and create a new Buzz execution generation whenever a
runtime reattaches. A provider restart must stop being equivalent to permanent
session death.

The reconnaissance changes several later recommendations:

- do not build a second approval center for sessions;
- do not build a second unread/mention/inbox system for sessions;
- do not invent a second durable agent identity model for executions;
- do not treat the relay's hosted Git as a replacement for local workspace
  checkpoints;
- do not assume the existing ACP pool survives a provider-process restart;
- settle schema compatibility as part of resume, before adding lifecycle
  actions to the signed protocol.

The provider-neutral session remains the product boundary. The primitives
below are implementation substrate, not reasons to turn a session into a
workflow, channel, managed agent, or Git repository.

## 1. Workflow approvals versus session approvals

### What already exists

The workflow approval path is farther along than the `WF-08` TODO initially
suggests:

- `crates/buzz-workflow/src/executor.rs` can return
  `StepResult::Suspended { approval_token }` for `RequestApproval`.
- The database migration and `crates/buzz-db/src/workflow.rs` provide a
  `workflow_approvals` record with pending/granted/denied/expired states,
  hashed decision tokens, approver rules, expiry, and guarded state changes.
- Relay command kinds `46030` and `46031` grant or deny an approval
  transactionally, validate authority and expiry, and arrange workflow
  resumption.
- Outcome kinds `46010` through `46012` are registered. The desktop already
  has approval cards and mutations, Home treats approval requests as
  `needs_action`, and kind `46010` is eligible for urgent push.

The missing seam is real: the workflow executor still has a `WF-08` TODO where
it should create the approval record and publish kind `46010`. Its current
finalization path treats a suspended result as failure instead of leaving the
run durably waiting. No production emitter completing that end-to-end path was
found.

### What this changes

Queue item #2 should **not** create session approvals by reusing workflow kind
`46010` or by treating a coding session as a workflow. Those events carry
workflow-specific identity and lifecycle semantics.

Instead, session/provider permission requests should have their own signed
event kinds and authority rules while reusing or extracting the generic
mechanics Buzz already has:

- pending decision records with expiry and idempotent resolution;
- authorized grant/deny commands;
- a `needs_action` projection;
- Home/inbox presentation;
- push and notification routing;
- reusable decision-card interaction patterns.

Provider ACP permission requests still require a real asynchronous response
channel back to the waiting adapter. That provider bridge does not exist in
the workflow implementation and remains new work.

## 2. Unread, mentions, notifications, and attention

### What already exists

Buzz already has a per-viewer attention stack:

- NIP-RS kind `30078` stores read positions by opaque context identifier.
- `crates/buzz-db/src/feed.rs` projects actionable events into Home using
  `event_mentions`, channel visibility, and `needs_action`.
- `p` tags already drive mention targeting and visibility.
- Desktop Home/inbox, notification settings, notification sounds, and the
  channel activity popover already consume these concepts.
- The coding-session lane is already integrated into normal visibility,
  unread, notification, and cross-community-observer rules rather than being
  rendered twice as chat.

NIP-RS currently documents `msg:` and `thread:` context conventions while
allowing arbitrary context strings. Opaque contexts are not pruned by the
current message/thread cleanup policy.

### What this changes

Queue item #5 should **not** begin with a new session-attention event store.
Use a defined per-session NIP-RS context such as `session:<session-ref>` for
read position, then explicitly define retention/pruning for that context.

Session approvals, directed handoffs, and requests for human input should join
the existing Home `needs_action` and push rails using their own event kinds and
`p` tags. The feed query and urgent-kind allowlist will need additive changes;
the user-facing inbox machinery does not need to be replaced.

Pinned, snoozed, and archived session state is distinct from unread state and
may still need a small per-viewer projection later. It should not block
provider continuity.

## 3. Managed-agent identity versus execution identity

### What already exists

Buzz has a coherent identity stack for durable agents:

- kind `30175` is a reusable persona definition;
- kind `30177` is an owner-authored, sanitized managed-agent instance
  projection linked to an agent public key;
- kind `30174` is agent-authored, owner-readable encrypted memory;
- NIP-OA delegation gives the agent its own signing identity while preserving
  owner authority and virtual membership;
- `buzz-acp::is_owner_or_sibling` recognizes the owner and agents delegated by
  the same owner for agent-to-agent interaction.

The coding-session provider currently publishes `agent_ref: None`. The
execution sandboxing findings correctly reject borrowing the provider host's
key or exposing provider credentials to an ACP adapter.

### What this changes

Do **not** invent a "session agent" identity or make an opaque ACP process the
durable participant. When a session participant needs durable identity, use a
managed-agent instance linked to a persona and reference it from session
metadata through `agentRef`. The runtime may come and go; the participant
identity remains.

Two provenance classes must stay visibly distinct:

1. provider-host-signed transcript facts report what the isolated ACP runtime
   did; and
2. agent-signed messages represent the durable participant speaking as itself.

Before shared steering becomes broadly available, each execution still needs
scoped runtime configuration and MCP/tool access. Managed identity is not
permission to leak a host's full environment.

## 4. ACP continuity and the existing pool

### What already exists

`buzz-acp` maintains a useful in-process pool and queue: it maps active Buzz
contexts to ACP session identifiers and serializes work. That improves reuse
while one harness process is alive.

It is **not** durable resume:

- the mapping is process memory;
- a replaced process loses it;
- `crates/buzz-acp/src/acp.rs` currently exposes new/prompt/cancel/config but
  no load or resume operation;
- initialization retains steering support but does not preserve the full
  negotiated session capability set;
- the coding-session actor always calls `session/new`;
- `SessionStartup` receives an ACP session id, but the persisted session record
  does not retain it as a resume cursor;
- `buzz-agent` explicitly advertises `loadSession: false`.

Provider capability differs. The locally installed Codex ACP adapter advertises
load support and implements both `session/load` and `session/resume`. Current
ACP defines resume as continuing without replaying history, while load may
return prior messages. Support must be determined from initialization
capabilities, not inferred from provider name or package version.

### What this changes

Queue item #1 remains first, with these design constraints:

- persist the opaque ACP session id only in host-private provider state; never
  publish it in session metadata or transcript events;
- retain the negotiated load/resume capabilities;
- prefer ACP `session/resume` when advertised because Buzz already owns the
  shared transcript; use `session/load` only as a compatibility fallback and
  deduplicate any replayed updates;
- if the provider cannot resume, rejects the cursor, or the cursor is missing,
  start a fresh ACP session as a **new generation** and report that provider
  context was not recovered;
- every successful reattachment increments generation and publishes a new
  immutable execution target; commands addressed to an older generation stay
  fenced out;
- stopping an execution is durable intent, not merely ACP cancel for the
  current turn;
- distinguish Buzz lifecycle action names (`session.resume`, `session.stop`)
  from the ACP transport method named `session/resume`.

The provider supervisor may restart the sidecar, but recovery should be lazy or
explicit per live execution rather than eagerly awakening every historical
session. A stopped session must never auto-resume.

## 5. Hosted Git versus workspace checkpoints

### What already exists

Buzz relay Git stores content-addressed packs and manifests and uses compare-
and-swap when updating the repository pointer. Smart HTTP materializes an
ephemeral bare repository for receive/upload operations. Manifests retain safe
`refs/*`; the relay does not maintain a user's working tree.

The local T3 Code reference implements checkpoints in the working repository:

- `apps/server/src/checkpointing/Utils.ts` places them under
  `refs/t3/checkpoints/...`;
- `apps/server/src/vcs/GitVcsDriver.ts` uses a temporary index via
  `GIT_INDEX_FILE`, `read-tree`, `write-tree`, `commit-tree`, and `update-ref`;
- this captures dirty workspace content without moving the user's branch,
  HEAD, or real index.

That is a logical flow worth borrowing under T3 Code's MIT license. Its local,
single-user orchestration architecture is not a model for Buzz's signed,
multi-client session protocol.

### What this changes

Roadmap Step 5 should be a two-layer design:

1. create local hidden-ref snapshots with a temporary index for faithful,
   fast workspace capture and restore;
2. optionally publish selected checkpoint refs through Buzz hosted Git for
   authorized cross-member handoff, retention, or recovery.

The relay can transport and retain Git objects, but it should not be asked to
construct or restore a participant's local dirty workspace. Publication must
respect repository authority and session privacy, and hidden-ref acceptance
must be covered by a conformance test before relying on it.

## 6. Relay rate limits and coding-session ingress

### What already exists

The desktop already has a relay rate-limit gate and reconnect/replay controls,
and the relay has both a connection semaphore and per-public-key request
limits. Coding-session ingestion was recently moved behind the shared relay
back-pressure path.

The remaining risk is subscription multiplication: independent session hooks
can each perform history fetch plus live subscribe, creating bursts even when
every individual caller honors the same gate.

### What this changes

Do not add session-specific retry loops. Consolidate coding-session event
ingress into one community-scoped subscription/store and let views select from
that shared truth. Any new module-level store must be reset by
`resetCommunityState()` when the relay/community boundary changes.

This remains below resume, permissions, schema compatibility, and transcript
truth in urgency.

## 7. Revised queue

1. **Resume and generations.** Persist a private ACP cursor, retain negotiated
   capabilities, reattach through resume/load when supported, increment the
   execution generation, and add durable Buzz resume/stop intent.
2. **Schema compatibility for lifecycle expansion.** Treat this as part of
   item 1, not a later cleanup. Older signed events must continue to decode;
   older consumers must fail closed or ignore unknown actions deliberately.
3. **Provider permissions.** Add session-specific signed requests and
   resolutions, using the existing decision/attention rails and a new ACP
   response bridge.
4. **Transcript truth.** Complete stable call identity, parent relationships,
   append-only reasoning summaries, and replay deduplication—especially before
   enabling ACP load fallback.
5. **Durable participant identity and scoped execution profiles.** Reuse
   personas, managed agents, and NIP-OA; add `agentRef` only when the execution
   sandbox is honest.
6. **Session attention.** Add NIP-RS session contexts and project actionable
   session events into existing Home/push surfaces.
7. **Shared ingress consolidation.** Remove duplicate history/live
   subscriptions and reset the store across community changes.
8. **Workspace checkpoints.** Local hidden refs first; optional relay-backed
   publication second.

## 8. Immediate acceptance criteria for queue item #1

A resume implementation is not complete until all of these are true:

- killing and restarting the supervised session-provider process does not
  permanently kill an open Buzz session;
- the provider state survives restart without exposing its ACP cursor on the
  relay;
- a resumed execution has the same umbrella `sessionRef` and execution id but
  a strictly higher generation;
- stale commands for the old generation cannot reach the new ACP runtime;
- an explicit stop survives provider restart and prevents automatic recovery;
- unsupported or rejected resume produces an attributable discontinuity and a
  fresh generation rather than pretending context was recovered;
- Codex resume is exercised end to end; providers without resume support have
  a tested fallback;
- wire changes are documented in the relevant `docs/nips/*.md` file;
- sensitive state files have restrictive permissions and never enter signed
  events, logs, or adapter environment variables.

## Bottom line

Buzz has native rails for shared truth, authority, identity, attention,
decisions, and Git transport. The work is to join those rails at the session
boundary while keeping provenance and capability honest. The missing structural
primitive is durable execution continuity across provider-process death; that
is the next code change.
