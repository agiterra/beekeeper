# Project Pulse Session Lease Implementation Plan

Status: implemented and gated on `wip/project-pulse`; real provider/Desktop
manual acceptance remains.

This plan replaces the 30-minute metadata-freshness definition of Active work.
It does not replace durable kind 44223 observations and does not close umbrella
sessions when providers disappear.

## 1. Product contract

Project Pulse must distinguish facts from uncertainty:

- `provider_reachable`: an open umbrella has a current authority-valid live
  lease for a non-terminal exact generation.
- `open_unverified`: an umbrella is open but no current generation is verified
  live.
- `closed`: the durable umbrella closure fold says closed.

The UI groups these as **Provider-reachable sessions**, **Open · liveness
unverified**, and **Closed / last observed**. Empty copy is **No sessions are
currently verified live.** It never says nobody is working and never infers
safe-to-proceed from absence.

A lease proves that a provider owned a live actor when it renewed. It does not
prove human attention or code-area conflict. `wait` requires a valid live lease
and direct dependency/conflict evidence. Open unverified overlap maps to
`consult`.

## 2. Wire contract

Add ephemeral kind `24223`, `KIND_CODING_SESSION_LEASE`, mirroring durable
metadata kind 44223.

Content is strict JSON with duplicate-key rejection and no unknown fields:

```json
{
  "schema": "buzz-coding-session-lease/v1",
  "target": {
    "driver": "codex-acp",
    "instanceId": "provider-instance",
    "sessionId": "provider-session-id",
    "generation": 1
  },
  "state": "live",
  "leaseSequence": 42
}
```

`state` is exactly `live | released`. `leaseSequence` is a positive
JavaScript-safe integer, monotonically reserved and persisted before signing.

Ordered tags are exactly:

```text
["h", "<channel UUID>"]
["cslease-v", "cslease1-1"]
["cs-target", "<coding_session_target_key(target)>"]
["csl-command", "<commandId that minted this exact generation>"]
["cslease-seq", "<leaseSequence in canonical decimal>"]
```

The core validator owns content byte caps, strict target validation, closed tag
grammar, multiplicity/order, tag/content equality, sequence bounds, and the
first-acceptance replay window. Events more than 180 seconds old are rejected;
only a small configured future skew is accepted. Relay TTL never uses provider
time.

## 3. Authority proof

Lease authority is bound to the lifecycle command/receipt chain, never to
metadata authorship.

For `(community, channel, csl-command, cs-target)` the database resolver:

1. Loads accepted kind-44221 lifecycle command candidates, including
   soft-deleted rows, and requires exactly one strictly valid candidate.
2. Requires `session.create` or `session.resume`; `session.stop` cannot mint a
   generation.
3. Loads accepted kind-44224 receipt candidates for the same tuple, including
   soft-deleted rows, and requires exactly one strictly valid successful
   receipt signed by the command's `providerAuthorityPubkey`.
4. For create, requires generation 1 and `created` or
   `created_with_failed_initial_turn`.
5. For resume, requires the same driver/instance/session, generation + 1, and
   `resumed` or `resumed_without_context`.
6. Requires receipt target equal the lease target and lease signer equal the
   provider authority.
7. Scans the channel's complete lifecycle command/receipt fact set for any
   second successful proof that mints the same exact target under another
   command ID or authority, and rejects the target if one exists.

Ambiguity fails closed. Only successful immutable resolutions may be cached;
negative results are not long-cached because a lease may race its receipt.

## 4. Relay and Redis register

The relay handles kind 24223 before the generic ephemeral path:

```text
signature and authenticated-pubkey match
→ freshness and strict envelope
→ messages:write and token-channel admission
→ strict channel/project transport write admission
→ immutable authority resolution
→ atomic Redis apply
→ channel Redis pub/sub and guarded local fan-out
→ Nostr OK
```

Leases are never inserted into Postgres. HTTP `POST /events` rejects all
ephemeral kinds; lease publication is WebSocket-only.

Redis stores one full provider-signed event per exact target plus relay facts:

```text
targetKey, sequence, state, eventId, eventJson,
acceptedAt, expiresAt, authorityCommandEventId, authorityReceiptEventId
```

It also maintains a channel-scoped expiry index. The atomic update uses Redis
`TIME` and implements:

- no value: accept with 180-second public visibility and 211-second hidden
  register retention;
- higher sequence: replace and reset both lifetimes;
- equal sequence and same event ID: idempotent success without TTL refresh or
  repeated fan-out;
- equal sequence and different event ID: conflict;
- lower sequence: stale rejection.

`released` remains publicly visible for 180 seconds. Its monotonic register is
retained for 211 seconds: the 180-second replay window plus the allowed
30-second future skew and a one-second boundary fence. It must not delete the
register when public visibility expires because a delayed lower-sequence live
event could otherwise pass replay admission and resurrect the generation.

Cold HTTP and WebSocket queries require explicit `#h`, read the channel index,
drop expired entries, apply the requested Nostr filter, and return the original
signed provider event. Community-wide lease scans are rejected. The stored
`acceptedAt`/`expiresAt` and authority provenance remain available to the
future relay-signed kind-39011 digest; that digest must not erase the provider
assertion.

## 5. Provider and transport

Heartbeat cadence is 60 seconds; Redis TTL is 180 seconds. Events are separate
per exact generation and never batched initially.

`SessionRecord` gains a backward-compatible current-generation command id and
lease sequence state. Resume updates the generation command id and resets the
per-generation lease sequence. Sequence reservation is persisted before event
construction; gaps are allowed and reuse is forbidden.

Lease eligibility is `record not closed && SessionHandle::is_live()`. Idle and
waiting-for-input actors therefore continue renewing.

Lifecycle behavior:

- create/resume: establish actor and durable record; publish durable receipt,
  metadata, and required transcript facts through an acknowledged durable
  path; then emit the first live lease and start renewals;
- actor exit/stop: stop renewal, reserve and emit `released`, then publish the
  terminal durable transition;
- clean shutdown: stop renewal, best-effort release every live exact
  generation, bounded drain, close;
- crash: emit nothing; TTL is the fallback;
- reconnect: retain and send only the newest signed lease state per exact
  target; ordinary typing indicators remain droppable.

Network arrival order is not trusted. Higher sequence wins, release tombstones
fence late heartbeats, durable terminal generation state outranks leases, and
durable umbrella closure outranks all generation state.

The existing provider outbox is not relay-acknowledged: its sink currently
deletes a row after enqueueing a background command. Before claiming the
metadata-before-lease order, add an acknowledged publication command whose
future resolves only on a matching positive relay `OK`; connection loss,
timeout, rate gate, or negative `OK` returns an error and leaves the durable
outbox row pending. Separately add a latest-state ephemeral command that
coalesces by exact target and survives reconnect/rate gating.

## 6. Pulse fold and digest revision

Revise the still-prototype fold corpus deliberately to
`buzz-project-pulse-digest/v2`; do not append lease fields while claiming the
byte-exact v1 envelope.

Fold inputs add kind 24223. Per exact target, retain the highest valid lease
sequence; equal-sequence conflicts are invalid evidence. `released` and
expired leases do not establish reachability. A terminal newest 44223 status
(`stopped` or `disconnected`) outranks live lease evidence for that generation.

Version 2 groups `sessions[]` by umbrella session, not by exact generation.
`sessionKey` is the verified `sessionRef` when present and otherwise a stable
`implicit:<executionKey>` fallback. Each session contains ordered nested
`generations[]`; a resume must not produce a second umbrella card.

The outer session shape carries `sessionKey`, nullable `sessionRef`, nullable
`name` and `goal`, `lifecycle`, `coordinationState`, nullable
`latestObservationAt`, nullable `observedAgeSeconds`, `generations`, and
`sourceEventIds`.

Generation members expose the independent durable observation and lease facts,
preserving nullable keys:

```json
{
  "targetKey": "coding-session/v1|...",
  "executionKey": "coding-execution/v1|...",
  "providerAuthorityPubkey": "<provider pubkey>",
  "current": true,
  "reachability": "provider_reachable",
  "status": "idle",
  "statusAt": 1785590000,
  "branch": null,
  "observedCommit": null,
  "dirty": null,
  "leaseState": "live",
  "leaseIssuedAt": 1785600000,
  "leaseExpiresAt": 1785600180,
  "leaseSigner": "<provider pubkey>",
  "leaseSourceEventId": "<event id>",
  "leaseSequence": 42
}
```

Client-composed digests derive expiry conservatively from the provider-signed
event and only from lease events returned by the relay's unexpired snapshot.
Because first admission allows 30 seconds of future clock skew, their
reachability window is `created_at + 150` seconds, and cached Desktop folds
invalidate exactly at that boundary. This prevents a client-composed answer
from outliving the relay's 180-second acceptance-time lease. The future
relay-signed digest can use the authoritative Redis acceptance and expiry
instead of this conservative subtraction.
The future relay-digest source adds `leaseAcceptedAt`, authoritative
`leaseExpiresAt`, and lifecycle command/receipt event IDs from the Redis
register. Observation time, lease issue time, relay acceptance time, and expiry
must never be conflated.

Replace `activeSessions/staleSessions` with
`providerReachableSessions/openUnverifiedSessions/closedSessions`; remove the
30-minute active-window law. An unclosed session with no lease never falls into
ordinary last-seen history.

Both Rust and TypeScript folds bind to the same conformance vectors, including:

- live lease plus idle durable status is provider-reachable after hours;
- unclosed session without lease is open-unverified;
- released and expired leases are open-unverified;
- higher release beats delayed lower live;
- terminal generation metadata beats live lease;
- umbrella closure beats every lease;
- exact-generation isolation across resume;
- equal-sequence conflicting IDs yield unverified evidence;
- no lease never yields dead, nobody-working, or safe-to-proceed claims;
- null branch/commit/dirty/status facts remain null.

## 7. Immediate product copy

The UI must ship the honest grouping in the same prototype even if relay lease
integration is temporarily unavailable:

- current 30-minute result is labeled **Recently observed**, not Active work;
- stale unclosed sessions appear under **Open · liveness unverified**;
- empty copy is **No sessions are currently verified live**;
- absence never produces “safe to proceed.”

Once leases are available, the first group becomes **Provider-reachable
sessions**. Do not label it “currently connected”; a lease intentionally
survives transient connection loss.

## 8. Build workflow and ownership

The workflow is foundation → parallel lanes → integration gate → adversarial
review → one finalizer. Build lanes never commit.

1. Foundation A: core lease codec/kind and SDK builder, test first.
2. Foundation B: contract/vectors and the Rust/TypeScript folds together so the
   byte-exact v2 shape cannot drift.
3. Parallel lanes with strict file ownership:
   - authority/relay: DB proof resolver, Redis register, relay ingest/cold reads;
   - provider/transport: acknowledged durable publication, coalesced ephemeral
     publication, persisted sequences, lifecycle scheduling;
   - Pulse Rust: CLI fold, ACP digest, conformance contract/vectors;
   - Desktop: TypeScript fold/query/UI/copy/unit/E2E.
4. Gate: focused crate tests, Rust fmt/clippy/workspace tests, desktop checks and
   tests, conformance binders, file-size guard, then supported E2E.
5. Adversarial review: authorization/fail-closed, ordering/fold parity, and
   product honesty/transport loss.
6. Finalizer: apply verified findings, repeat gates, update
   `docs/SESSION_STATE.md`, and create signed logical commits only when green.

## 9. Acceptance invariants

- No lease event creates a Postgres row.
- A lease signer cannot bootstrap authority through metadata.
- Wrong channel/community/target/command/receipt/signer fails closed.
- Duplicate delivery does not extend liveness.
- Lower sequence cannot resurrect a released generation.
- A cold HTTP or WebSocket query returns the original provider-signed event.
- An idle live actor remains provider-reachable beyond 30 minutes.
- A crashed provider becomes open-unverified within 180 seconds.
- A terminal generation never renders provider-reachable.
- An open unverified umbrella remains visible and coordination-relevant.
- No surface says nobody is working or safe to proceed from absence alone.
