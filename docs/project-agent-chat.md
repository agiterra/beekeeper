# Project agent chat: conversational Host path

This contract supersedes the inspection candidate. The archived inspection run
is conformance evidence, not the product. Ordinary language, clarification,
status and follow-up go to the installed lead model. The lead chooses whether
and how to delegate through existing `bee sessions hire/assign/report/verdict/
decide/handover/work` operations. No task parser or correctness oracle belongs
in the transport.

## One consuming path

`bee agents start` already installs a Host conversation route. The existing
session-provider conversation admission calls ACP's ordinary author policy,
persists the signed source event ID as command identity and its author/channel/
thread/target binding, then runs the existing ACP model session. Its durable
mailbox and outbox provide replay admission and exact signed reply recovery.
The Host projects assistant output into the original thread. Desktop is only
a chat client. Do not launch an additional ACP subscription or chat listener.
The obsolete `hire-host chat` command and project_chat parser/state machine are
removed. `hire-host run` consumes only model-authored SessionHire commands.

Initial project configuration is owner-only, one canonical lead and runner,
explicit runtime/model (Sol low for later live acceptance), shared project
context, accepted lead operator/seat grants in an owner-founded umbrella.
The owner transport stays in the management service; model tools receive only
the actor's existing scoped signer. Membership never establishes owner rights.
Conversation permission is not spending permission: the existing hire responder
checks accepted lead authority and canonical project association separately.

## Missing connections and decisions

The conversational execution path exists; the previous implementation bypassed
it. Wire the public role-install/start/found/grant operations to that path in
acceptance and document one repeatable setup. Supply source identity and thread
routing in the existing conversation preface so model-chosen operations can
retain provenance. The model uses existing transaction references and work
records, not invented task state. Provider terminal means execution ended; it
does not prove correctness. Replies distinguish worker claims, artifacts,
checks actually run, independent review and remaining uncertainty.

Owner-side hire supervision must retain cleanup obligations independently of
current lead permission. A bounded service deadline covers pending creation,
created-awaiting-grants and ready workers. Known targets are retained even
without ready grants. Expired uncertain creation stays fenced and is observed,
never re-created; a later Created receipt triggers exact owner stop. Revoking
the lead blocks new work but cannot disable owner cleanup. Stop confirmation
is required before resource release. Permanent output refusal is durable and
request-specific; transport uncertainty retries exact bytes. A refused output
cannot starve unrelated safe work or erase active-resource cleanup.

## Acceptance and lifecycle

Use the isolated relay/Host and scripted ACP fixture: ordinary-language initial
conversation, clarification/follow-up, scripted choice of bounded delegation,
canonical hire/assignment/report and same-thread qualified answer. Scripted
choices test protocol, not intelligence. Include unauthorized member, distinct
threads, source replay/restart, exact operation retry, created-awaiting-grants
expiry, late creation, permanent refusal and revoked-lead owner cleanup.
No Desktop, production publication, real model or real Host changes.

After final source freeze run focused checks and one bounded full CI (four
build/test workers, nice 10, one-hour limit), preserving separate new evidence.
Parent live acceptance will use Sol low for both actors: explicitly request one
independent runner to verify a concrete fixture claim, let the lead formulate the
assignment and assess returned checks, then answer a follow-up in the original
thread. This tests model-managed requested delegation, not spontaneous delegation. Capture source/operation/receipt/report IDs and actual
runtime/model. Do not call scripted delegation model judgment. No real spend
is authorized in this implementation task.

## Canonical setup and real acceptance (not executed here)

Use an already approved commissioned Host and its public model catalog. Install
`lead` and `runner` with `sessions hire-host install-role` using explicit
`gpt-6.1-sol[low]`, project/channel, canonical role source, and parallelism 2.
Start both through `agents start`, preserving the canonical prompt
`Follow the canonical role pack staged for this responsibility.` The lead's
ordinary route is the consumer: no extra lead model launcher or chat command.
The normal ACP owner-only policy also admits independently verified same-owner
siblings; it does not admit arbitrary channel members. Do not broaden it to
`anyone` as a workaround for missing grants.

Use `sessions found` once with a retained operation UUID, then `sessions create`
for its project-bound execution, with no initial turn. Grant the installed lead
collaborator/operator and lead seat authority using `sessions grant` and
`sessions grant-seat`. These existing accepted-chain prerequisites are separate
from conversation permission. Run the owner-side `sessions hire-host run` with
that exact genesis, project/channel, runner actor, explicit runtime/model,
restricted role policy and `--lead <installed-lead-pubkey> --deadline-secs 300 --isolated-cutover`.
The owner service idempotently supplies the public project/umbrella coordinates
to the existing Host conversation preface. It checks installed project role;
the provider validates owner signature, channel, actor and accepted project.
This is context, not a new grant. A different existing binding is refused. Keep the
same service home after restart. Only the Host starts ACP processes. The owner
service must remain supervised for its deadline to take effect; this is not a
hard timer enforced while that service is offline. Expiry or revoked lead
permission never authorizes deleting an uncertain resource or reusing its slot.

For the bounded live scenario, the owner explicitly asks: “Use one independent
runner to verify this harmless fixture’s documented marker, then read its report,
assess the evidence and uncertainty, and answer here. No edits or network
research.” Then ask “Which evidence did you assess, and what still needs
independent review?” in the same thread. The coordinator chooses the assignment
and tools; the harness must not generate its hire, assignment or assessment.
The remaining allowance is seven native Sol-low turns across all actors and setup;
no automatic rerun or increase. General product guidance still permits the lead
to decide whether delegation is useful for other requests.
It can discover role/runtime/authority through public `bee` operations; the
umbrella coordinates are setup context, not authority or secrets. The scripted
fixture receives umbrella coordinates through this same Host preface; its
project file contains only fixture model/runner selection. Its Python branches
are not shipped policy.

Use a second thread for a conversational summary that needs no worker. Capture
signed requests, lead hires, owner creates, grants, assignment, runner report,
actual model selection, terminal and original-thread replies. Repeat exact
source events and restart only the isolated owner responder; verify no second
hire/report. Require owner stop receipt at the bound. A provider terminal cannot
replace an independent review or an applicable repository gate. No real model
run, install, Host restart or production publication has occurred in this task.

The retired candidate's `management/project-chat` journals are not migrated or
replayed: that candidate never ran in production. Preserve experimental homes
as evidence. New work uses existing Host conversation and hire journals only.
Loss of Host state, cross-host ownership and requester-private memory are not
solved by this change. After an uncertain model/tool failure, inspect existing
work/command records and resume their exact IDs; do not blindly reissue a new
hire command. Source replay is fenced by Host consumption, while canonical
hire/wake retry is fenced by its retained signed operation and target.

## Report return path correction (2026-10-30)

The conversational lead has an ordinary actor execution, with no role or umbrella
metadata. Its accepted lead grant in the delegation umbrella is authority, not
an execution address. `sessions report --wake-to lead` resolves role-bearing
executions and cannot address it. An exact session wake alone also lacks the
conversation origin needed to project an answer into chat.

Use the existing ordinary channel path: the lead includes its actor pubkey,
channel, original thread and request in both hire brief and assignment. The
worker publishes its structured report without a role wake, then sends one
ordinary signed message mentioning that actor and replying to the source
thread, containing the accepted report ID and assignment ID. The lead reads
and assesses the report and evidence; publication is not delivery or correctness.
If publication is uncertain, inspect/retry the existing signed event rather than
create another report or notification. Exact event replay uses existing Host
consumption. No new outbox, grant, synthetic seat or report auto-consumer is added.

The hire brief says a canonical assignment wake will follow; the worker can end
its initial turn without requesting the assignment again. The coordinator must
still publish that assignment and its existing wake. This is generic coordination
context, not a task-specific prompt or a claim that a model follows instructions.
The next bounded model run must prove post-report lead assessment explicitly.

## Exact selection and refusal feedback contract (2026-10-30 follow-up)

The owner listener's configured worker actor, permitted roles, provider alias and
exact model (including effort suffix) are public selection context for the lead.
They come from the same configuration as enforcement, not a guessed catalog default.
Omitting model retains the selected identity default; an explicit mismatch remains
a terminal refusal. Context grants no authority and cannot override live validation.

A hire from chat needs an explicit signed return reference: optional `replyTo`
on `session.hire`, exposed as `sessions hire --reply-to <source-event-id>`.
This additive field is a routing claim, not authority. The owner Host must match
it to its already admitted conversation origin for this requester, channel and
configured project/genesis. It must never infer a thread from brief prose or
choose the last conversation. Old hires retain existing seat delivery; unknown
or mismatched chat origins retain a named durable unresolved-delivery reason.

A terminal refusal is journaled before its single owner-signed ordinary chat
notice. The notice names its hire ID, carries the verified original root/source
and requester mention, and discloses that it is Host-generated. Store exact
signed bytes before publication; ACK loss retries those bytes. Existing chat
admission/replay records a verified Host answer without opening a model turn. CLI wait recognizes only a signed
founder notice bound to its exact hire, not an arbitrary chat refusal. Relay
acceptance is disclosed separately from actual conversation execution.

## Review constraints: verified source, Host provenance, recovery (2026-10-30)

- F1: a saved Origin is only routing preparation. A returnable source requires
  separately persisted successful mailbox/runtime admission, with its verified
  source event/author, actor, exact execution, channel, project, genesis and
  owner pinned at admission. Refused/dropped/unverified, legacy, missing-author
  or unbound sources cannot be upgraded by today's configuration. Re-admission
  must not silently replace the historical scope. Receipt promotion follows
  successful existing turn gates; refusal invalidates eligibility.
- F2: the existing Host-answer tag is a claim until verified against the owner,
  exact signed hire, original admitted source and exact reply markers. Persist
  typed hire/source/requester provenance with the origin. Use existing
  record-only Host-answer behavior: a refusal is visible in the source chat and
  read by the waiting CLI, with `hostAnswer: true` in transcript/context, but it
  is not another model turn, human override or mailbox work item. Held replay
  retains that classification. Prose cannot classify an event; forged or
  unresolved claims refuse rather than fall back to human input. No new public
  action or authority is required beyond the existing replyTo and Host tag.
- F3: optional lead context refresh is separate from owner recovery. Any refresh
  error fences fresh selection/publication, preserves conflicting context, and
  still drives saved obligations through exact owner reconciliation/stop. Do
  not label context failure as authority revocation. Test malformed/conflicting
  context and atomic write failure with an independently valid cleanup journal.

Acceptance here is deterministic combined refusal-to-thread notice, verified
Host transcript attribution and restart/replay without another turn, plus
cleanup despite context refresh failure. No live spend is authorized.

Implementation of the review constraints uses optional local `admission` and
`hostNotice` records on the existing origin. A candidate admission is pinned
only after author, definition, accepted execution and ProjectContext validation;
only a queued/started/injected receipt promotes it. Refused/dropped receipts
invalidate it. The original signed source is retained and checked at lookup.
Legacy origins remain readable but ineligible for owner refusal routing.

The owner notice carries `buzz-host-answer=hire` and `hire-ref`. Reception checks
the exact signed hire, its requester and replyTo, historical owner/project/genesis,
and original NIP-10 reply markers. Its existing `hostAnswer:true` transcript item
contains a structured provenance frame, including original source author distinct
from the Host owner. The exact signed transcript is saved before outbox enqueue;
replay preserves its event ID and consumes the notice without model dispatch.
This supersedes the earlier claim that refusal feedback should open a lead turn:
the waiting hire CLI supplies the decision to the already running conversation.
An idle lead is not automatically woken by a Host refusal.

On context refresh failure, the listener retains a named readiness error, fences
new requests, and reconciles saved operations through the same owner stop path.
It preserves the failed/conflicting context bytes. Cleanup is deliberately
immediate rather than relying on the now unavailable lead context until deadline.
There is no new wire action, seat, grant or scheduler in these repairs.

A Host notice whose exact signed hire is not yet readable stays unconsumed with
`HOST_NOTICE_LOOKUP_PENDING`; replay retries the same event. Verify its historical
owner/source before that lookup so an ordinary participant cannot create this
transport wait by forging a Host tag. A mismatched verified relationship is
refused rather than converted to human input.

## Automatic Host-notice recovery constraints (verified-return R1)

Retain the exact signed notice only after verifying historical source admission,
owner, actor/channel, canonical hire reference and original reply markers. Save
before the first external lookup or transcript effect. The existing conversation
backfill maintenance slot owns retry: one bounded off-loop lookup at a time,
alternating with route backfill; no additional timer, model or scheduler.
Retain at most 64 notices of at most 16 KiB each; retry at most 32 times with
persisted exponential 2–300 second backoff. Exhaustion is a named parked state,
not consumption or success; no pending obligation is evicted to admit another.
A saved typed transcript is retried with its exact ID after save/enqueue failures.
Maintenance and restart must recover without caller redelivery. Invalid/foreign
claims are not captured for repeated work. Existing consumed/refused ledgers
end recovery, and the durable outbox owns accepted transcript publication.

The parent's explicit independent-runner acceptance wording above is preserved.
It supersedes the optional-delegation scenario in the historical review-repair
report; the remaining live allowance is seven turns, owned by the parent.
