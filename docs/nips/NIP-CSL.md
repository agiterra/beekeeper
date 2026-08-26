# NIP-CSL: Coding-Session Lifecycle Commands

`kind:44221` is a durable, channel-scoped lifecycle command from a Buzz
operator to a coding-session provider adapter. It creates a new session without
exposing a host path, environment, secret, provider session identifier, or
generation in signed operator intent.

This contract complements [NIP-CSC](NIP-CSC.md): NIP-CSL creates a session;
NIP-CSC sends turns to an existing exact session generation. Both are storage
and fan-out events. The relay validates and stores them but never routes them
through `command_executor` or executes them.

Provider-neutral rendering after creation uses the signed
[NIP-CST transcript contract](NIP-CST.md) (`kind:44225`).

> **Fork amendments.**
>
> 1. **Native kinds only.** No kind-9 compatibility fallback exists anywhere in
>    this fork — not for 44221, and not for the 44223/44224 lifecycle facts
>    below. Every publication uses exactly one native kind; kind-44223 metadata
>    may publish repeatedly as append-only observations on state transitions.
> 2. **`projectRef` is optional.** A session may stand alone, owned by the
>    channel it is published into rather than by a project. See below.
> 3. **`projectRef` coordinates are `30621:` only.** The donor's example used
>    the `30178:` team-catalog kind; sessions here bind to NIP-MP projects
>    (`kind:30621`) and nothing else.
> 4. **`sessionRef` groups executions into an umbrella session.** A nullable,
>    client-minted UUID added after v1 shipped. Authority-aware creates also
>    carry `genesisRef`, the exact founder event id. The action has exactly the
>    historical 8-key, 9-key `sessionRef`, or 10-key linked form. See below.
> 5. **Continuation is generation-fenced.** `session.resume` and
>    `session.stop` address an exact published `cs-target`. Resume never carries
>    the provider's opaque ACP cursor; that cursor remains host-private. A
>    successful reattachment publishes a new generation, while stop is durable
>    intent that survives provider restart.
> 6. **Liveness is an ephemeral lease, not metadata freshness.** Kind 24223
>    proves recent provider reachability for one exact generation. It does not
>    replace durable metadata and is never written to Postgres.
> 7. **A turn gets its own receipts.** `turn_queued`, `turn_started`,
>    `turn_degraded`, `turn_dropped`, `turn_refused`, and
>    `interrupt_delivered` — keyed by `commandId` and `status`
>    together, never confirming or ending a generation on their own. Their
>    `commandId` normally names a `kind:44220` turn command, but for the
>    initial turn embedded in a `kind:44221` `session.create` it names that
>    **create**. See "Fork amendment: turn-stage receipts" below.

## Wire contract

Event content has exactly this JSON shape. Nullable fields remain present with
an explicit `null`, and additional or missing fields are invalid:

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "client-idempotency-id",
  "action": {
    "type": "session.create",
    "projectRef": "30621:<lowercase-64-hex-owner>:<project-d>",
    "repoRef": "30617:owner:repository",
    "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    "genesisRef": "64-lowercase-hex-genesis-event-id",
    "providerInstanceRef": "capability-advertised-instance",
    "providerAuthorityPubkey": "64-lowercase-hex-catalog-signer",
    "model": "provider-neutral-model-id",
    "title": "Operator-facing session title",
    "initialTurn": "Inspect the project and begin."
  }
}
```

The same v1 envelope also admits exactly these two lifecycle action shapes:

```json
{
  "schema": "buzz-coding-session-lifecycle-command/v1",
  "commandId": "client-idempotency-id",
  "action": {
    "type": "session.resume",
    "session": {
      "driver": "codex-acp",
      "instanceId": "provider-instance-id",
      "sessionId": "provider-minted-buzz-session-id",
      "generation": 1
    },
    "providerAuthorityPubkey": "64-lowercase-hex-catalog-signer"
  }
}
```

`session.stop` has the identical three-key action with `type` set to
`session.stop`. Adding a discriminated action is an additive v1 evolution:
older consumers reject an unknown action and therefore fail closed; they must
not reinterpret it as `session.create`.

`session.resume` addresses the exact disconnected generation the operator
observed. The provider resolves its persisted ACP cursor, working directory,
runtime, and model locally. On success it keeps the Buzz `sessionId`, advances
`generation` by one, resets the per-generation transcript sequence, and emits
new metadata. ACP `session/resume` is preferred when advertised;
`session/load` is a compatibility fallback. If neither recovers context, a new
ACP session may still attach as the next generation, but its receipt and
transcript must say `CONTEXT_NOT_RECOVERED` /
`session_restarted_without_context` rather than claiming continuity.

Every ACP session-open method (`session/new`, `session/resume`, and
`session/load`) receives the same launcher-selected private, read-only context
MCP when a verified package is available. Native resume/load does not receive a
reconstructed-context system prompt because it already supplies
provider-native context for that execution; the MCP remains available as an
evidence surface for sibling work observed in the snapshot.

For a reconstructed `session/new`, the launcher MUST push a bounded
`coding-session-first-turn-brief/v1` before the model's first token, using the
adapter's system-prompt transport where supported and a first-turn preamble
only as fallback. The brief is deterministic, not model-authored: session
identity and safe goal/name, snapshot provenance, recent turn transport
outcomes, plan counts, tool attempt outcomes, and signed evidence ids. It MUST
NOT carry tool arguments/results, reasoning, host paths, credentials, or the
opaque ACP cursor. `ended_normally` means only that ACP ended the turn normally;
it MUST NOT be presented as semantic task completion.

Depth stays pull-based through `session_overview`, `session_history`, and
`search_session`. `session_overview` repeats the exact first-turn brief. A
complete package is complete only through its additive `completeAsOf`
watermark; later concurrent work may exist. Historical packages without that
field remain readable but MUST be described as having an unknown legacy
watermark. `sourceEventCount` counts the complete verification proof graph
(identity, authority, lifecycle, metadata, and transcript), while
`totalHistoryItems` counts transcript items only.

`session.stop` addresses the exact current generation. Once consumed, the
provider records the execution as closed before releasing its process. Restart
recovery must not attach or make resumable a closed record. This is different
from `thread.turn.interrupt`, which cancels only the in-flight turn and leaves
the execution available. The stopped generation's final metadata status is
`stopped`; consumers must treat that lifecycle status as terminal. It is
distinct from `completed`, which remains the degraded transcript-only inference
for a successful turn when metadata is absent.

`providerInstanceRef` and `providerAuthorityPubkey` are required. The provider
authority is exactly the lowercase 64-hex signer of the selected
[provider-catalog](NIP-CSPC.md) event; it addresses the command to one adapter
even when several share a channel. `projectRef`, `repoRef`, `sessionRef`, `model`,
`title`, and `initialTurn` are nullable; `genesisRef`, when its key is present,
must be a non-null lowercase 64-hex event id. A present string must be nonempty
after trimming (`sessionRef` carries its own stricter shape, below). Limits are
UTF-8 byte limits:

- `commandId`: 256 bytes
- all references, `model`, and `title`: 2 KiB (2,048 bytes) each
- `initialTurn`: 12 KiB (12,288 bytes)
- complete event content: 16 KiB (16,384 bytes)

### `projectRef`: optional, but structurally required

`projectRef` may be `null`, which creates a **standalone session**. Sessions
organize under projects when the project-containers feature is present and are
fully usable without it.

Optional is not the same as omissible. The key must still be written — an
explicit `null` — exactly like every other nullable field. A payload that
simply lacks the key is a truncation or a partial serialization, and reading it
as a deliberate standalone session would silently detach a session from the
project its operator chose. Decoding rejects it.

Optional is also not the same as unvalidated. A **present** `projectRef` must
be a canonical NIP-MP project coordinate:

```
30621:<lowercase-64-hex-owner>:<project-d>
```

parsed by splitting on the first two colons only, so a project whose `d` tag
contains a colon stays addressable. Owner hex must be lowercase: `#a` filter
matching is byte-exact, so an uppercase-owner coordinate would be invisible to
the queries readers actually issue. A `30178:` team-catalog coordinate, a
`30617:` repository coordinate, and a bare slug are all rejected.

### Fork amendment: `sessionRef` umbrella reference

One user-facing session may contain several provider executions — a Claude
execution and a Codex execution as co-participants in one surface. The grouping
identity is `sessionRef`: a client-minted canonical UUID (36 characters,
`8-4-4-4-12`, lowercase hex), deliberately distinct from every
provider-runtime identifier. A later create carrying the same `sessionRef`
**joins** the umbrella as a new execution with its own `cs-target` and its own
fact streams; nothing about receipts, metadata keys, generations, or
transcripts changes shape. `null` claims no umbrella — the pre-amendment
semantics, an implicit umbrella of one.

**Decode discipline — where this differs from `projectRef`.** `projectRef` was
in the schema from v1, so its key is structurally required and only its value
may be `null`. `sessionRef` was added to an already-deployed schema: signed
v1 events without the key exist and must stay valid forever. The action is
therefore **exactly the 8-key v1 set, exactly the 9-key set including
`sessionRef`, or exactly the 10-key set including both `sessionRef` and
`genesisRef`** — nothing between, nothing beyond. `genesisRef` without a
non-null `sessionRef` is invalid. New producers always write `sessionRef`
(explicit `null` or a UUID); authority-aware producers add `genesisRef`. The
8-key form is accepted only as historical replay.

Optional is still not unvalidated. The reference travels in no tag, so nothing
downstream normalizes it; two clients agree on umbrella membership only if the
bytes are byte-exact. A present `sessionRef` must therefore be canonical —
uppercase hex, braces, URN prefixes, and truncations are rejected rather than
coerced, because a non-canonical spelling would silently split an umbrella in
two.

No tag carries either reference. A `genesisRef` is resolved only by its event
id; consumers MUST NOT select authority by querying a genesis tag or by
choosing among events with the same `sessionRef`. The resolved genesis must be
signature-valid, kind 44226, scoped to the create's channel, and carry the
same `sessionRef`. Its signer is the founder. Creates without `genesisRef`
retain the interim legacy projection: founder is the founding create signer.

The provider echoes a claimed reference into `kind:44223` metadata as an
*optional* `sessionRef` key — emitted only when non-null, never as an explicit
`null` — so pre-amendment consumers' exact-key metadata check keeps accepting
every session that never claimed an umbrella. The echo is a projection
convenience for catalog grouping; the operator-signed create remains the
authoritative membership claim. If they ever disagree, consumers trust the
create and flag the record.

### Tags

Exactly these three two-field tags, in this order:

1. `["h", "<channel UUID>"]`
2. `["csl-v", "csl1-1"]`
3. `["csl-command", "<commandId>"]`

The `csl-command` value must equal the payload `commandId`. Order is
load-bearing — adapters read these positionally. No tag carries the project
reference, so a standalone session and a project-bound one produce identically
shaped envelopes.

The command author is the signed event pubkey. Consumers must never use content
as claimed operator attribution.

## Authority and provider resolution

The relay requires `messages:write`, a valid `h` channel scope, and an active
channel membership row. Open-channel visibility is not lifecycle authority.
Kind 44221 is never global and is not a relay-executed command.

`projectRef`, `repoRef`, and `providerInstanceRef` are signed logical
references. `providerAuthorityPubkey` is the selected catalog signer, not an
operator identity claim; a consumer must compare it to its own current signing
pubkey before reserving a command or causing provider side effects, and ignore
commands addressed to another authority.

**No host-local runtime state travels in signed content.** Not paths,
environment variables, secrets, process identifiers, or opaque ACP session
ids. The provider-neutral Buzz `cs-target` is the deliberate exception for
`session.resume` and `session.stop`: its public session id and generation fence
already identify signed lifecycle facts. The working directory in particular
is machine-local configuration the producer resolves for itself;
`deny_unknown_fields` on the action means a `cwd` smuggled into the payload is
a decode failure, and the relay rejects the event.

`commandId` is the idempotency key for lifecycle consumers. A consumer should
bind a successful creation result to the signed project and channel before it
publishes provider-neutral session projections.

## Lifecycle facts

The provider publishes generation metadata observations as `kind:44223` and
lifecycle receipts as `kind:44224`, using only those native kinds. A receipt is
one immutable outcome for one command. Metadata is append-only observation
history: the provider publishes a new event on a state transition, and a
generation may therefore have several events with the same semantic grouping
key.

Metadata tags, in order: `h`, `csm-v` (`csm1-1`), `cs-target`, `csm-key`.
Receipt tags, in order: `h`, `cslr-v` (`cslr1-1`), `csl-command`, `csl-key`.

Semantic keys are the same length-prefixed encoding NIP-CSC describes:

```
coding-session-metadata/v1|<driver><instanceId><sessionId><generation>
coding-session-lifecycle-receipt/v1|<commandId>
coding-session-lifecycle-receipt/v1|<commandId>|<status>
```

A **lifecycle** receipt (`created`, `created_with_failed_initial_turn`,
`failed`, `resumed`, `resumed_without_context`, `stopped`) is keyed by
`commandId` alone: one lifecycle command has exactly one outcome, so a second
receipt for the same command is a duplicate to drop, never a revision to
apply. A **turn** receipt (below) is keyed by `commandId` *and* `status`,
because one `kind:44220` turn command legitimately produces several of them in
sequence (for example `turn_degraded`, then `turn_queued`, then
`turn_started`) and keying by `commandId` alone would drop everything after
the first. Consumers retain all metadata rows and fold the newest valid
observation per exact generation by `(created_at, event id)`. A new generation
fences resume identity; it is not required for an ordinary status transition
or corrected observation.

The receipt keeps the same exact five-key v1 object. In addition to the create
statuses, lifecycle continuation uses:

- `resumed`: `session` is the new-generation target and `error` is `null`;
- `resumed_without_context`: `session` is the new-generation target and
  `error` is `{ "code": "CONTEXT_NOT_RECOVERED", "message": "..." }`;
- `stopped`: `session` is the stopped exact target and `error` is `null`;
- `failed`: unchanged, with `session: null` and a stable error object.

Authority refusals use stable codes: `GENESIS_NOT_FOUND` when an exact genesis
cannot be resolved and verified, and `UNAUTHORIZED_OPERATOR` when a turn,
interrupt, stop, or resume signer is not the cached founder. Both are durable
receipts; the provider must never execute or silently discard these cases.

An old consumer that does not recognize a new status rejects that receipt; it
must never coerce the outcome into `created`. The new generation's metadata and
transcript remain independently verifiable facts.

### Fork amendment: turn-stage receipts

A [NIP-CSC](NIP-CSC.md) turn gets its own receipts, distinct from the six
lifecycle statuses above, so an operator or a sibling agent can watch a turn
land without polling the transcript. Six statuses:

- `turn_queued` — the command was accepted into the session's mailbox and has
  not started yet.
- `turn_started` — the provider began running the turn. **This is the only
  turn status with a sixth key**, `turnId`: the provider's own identifier for
  the started turn.
- `turn_degraded` — the command asked for `deliver: "steer"` and this
  execution's runtime advertised no native steering, so it will be delivered
  at the next turn boundary instead (`STEER_UNSUPPORTED`). The turn is not
  refused, not merged into the running turn, and not lost: a `turn_queued`
  follows. See the downgrade rule in [NIP-CSC](NIP-CSC.md).
- `turn_dropped` — the provider will never run this command and nobody was
  refused: the mailbox was full (`QUEUE_FULL`), or the session has no live
  execution to deliver into (`NO_LIVE_EXECUTION`). Both are **terminal**: the
  command is never consumed (it did not run) and it is recorded as refused, so
  the answer is given once and no redelivery repeats it. A dropped turn is not
  re-delivered by resuming the session — `session.resume` mints a new
  generation, and a replayed command still addresses the old one, so it would
  be refused as `STALE_GENERATION`. The sender has to send it again. What this
  contract guarantees is that a turn is never *silently* lost, not that every
  accepted turn eventually runs.
- `turn_refused` — the provider will never run this command: an unauthorized
  operator, or a target this provider owns that no longer accepts turns.
- `interrupt_delivered` — a `thread.turn.interrupt` reached a live turn and
  the cancel was issued. An interrupt that finds no live turn gets
  `turn_refused` (`NO_TURN_IN_FLIGHT`) instead, and one from a signer who may
  not steer the session at all gets `turn_refused`
  (`UNAUTHORIZED_OPERATOR`). A `thread.turn.interrupt` is **not** founder-only:
  any signer who may steer the execution may send one. Only the
  `deliver: "interrupt"` *class* on a `thread.turn.start` is founder-only, and
  a granted operator can reach the same effect in two commands (interrupt,
  then start). That gap is recorded here rather than papered over; closing it
  is authority work, not delivery work.

Every turn status except `turn_started` keeps the exact five-key v1 object
(`schema`, `commandId`, `status`, `session`, `error`) — no `turnId` key at
all, present or `null`. `turn_started` has exactly six keys: the five plus
`turnId`.

`commandId` names the signed command that caused the turn, and that command is
one of **two** kinds. Normally it is a `kind:44220` `thread.turn.start` or
`thread.turn.interrupt`. For the initial turn embedded in a `kind:44221`
`session.create`'s `initialTurn` there is no `kind:44220`, so the provider
publishes that turn's stage receipts under the **create's own** `commandId` —
which is what makes the first prompt joinable to the command that asked for it
(see [NIP-CST](NIP-CST.md)). A consumer MUST resolve a turn receipt's
`commandId` against both kinds and MUST NOT reject a `turn_started` whose
`commandId` names a `kind:44221` as a malformed cross-kind receipt. It follows
that one create's `commandId` can carry both a lifecycle receipt (`created`)
and turn receipts; the `status` discriminates them, and the semantic key keeps
them distinct on the wire.

`session` is the target the command addressed — `driver`, `instanceId`,
`sessionId`, `generation` — for **every** turn status, including
`turn_refused`/`turn_dropped`: unlike a failed `session.create` receipt, a turn
receipt's session is never `null`, because by the time a turn receipt is
published the exact generation is known.

`error` is `null` for `turn_queued`, `turn_started`, and
`interrupt_delivered`, and a `{ "code", "message" }` object for
`turn_degraded`, `turn_dropped`, and `turn_refused`.

**The code set is open.** A validator accepts any nonblank code of at most 64
UTF-8 bytes containing no control characters, and MUST NOT pin `turn_dropped`,
`turn_degraded`, or `turn_refused` to a closed list — a provider that grows a
new reason must not be decoded as malformed by a client that predates it. The
codes in use today are documented, not enforced:

| status | code | means |
| --- | --- | --- |
| `turn_degraded` | `STEER_UNSUPPORTED` | this execution's runtime offers no native steering; delivered at the boundary |
| `turn_dropped` | `QUEUE_FULL` | the in-actor turn queue is at `SESSION_QUEUE_DEPTH` |
| `turn_dropped` | `NO_LIVE_EXECUTION` | the session is persisted but nothing is running to deliver into |
| `turn_refused` | `UNAUTHORIZED_OPERATOR` | the signer may not steer this session — including a non-founder asking for `deliver: "interrupt"` on a `thread.turn.start` |
| `turn_refused` | `UNKNOWN_TARGET` | this provider owns the session id but not that target |
| `turn_refused` | `STALE_GENERATION` | the addressed generation has been superseded |
| `turn_refused` | `SESSION_CLOSED` | the session no longer accepts turns |
| `turn_refused` | `NO_TURN_IN_FLIGHT` | a `thread.turn.interrupt` reached a live execution that had no turn running or awaiting start |
| `turn_refused` | `NO_LIVE_EXECUTION` | a `thread.turn.interrupt` addressed a session with no live process, so there was nothing to cancel |
| `turn_refused` | `QUEUE_FULL` | a `thread.turn.interrupt` could not be delivered because the execution's mailbox is full |

**Publish points** (provider-side):

- `turn_queued` — when a `TurnDecision::Start` is accepted into the session's
  mailbox.
- `turn_degraded` — when a `deliver: "steer"` command has been accepted into
  the mailbox of an execution that cannot take a mid-turn steer, immediately
  before that command's `turn_queued`. Never before the delivery is known to
  have succeeded: a degrade in front of a delivery that then fails publishes
  two receipts contradicting each other about one command. Its `message` MUST
  be a function of the command, not of what a particular process learned at
  `initialize` — a redelivery answered by a different process must not publish
  a second payload under the same `(commandId, turn_degraded)` semantic key.
- `turn_dropped` — when the mailbox itself is full (`QueueFull`), when the
  in-actor turn queue overflows (`SESSION_QUEUE_DEPTH`), or when the addressed
  session has no live execution to deliver into (`NO_LIVE_EXECUTION`). The
  queue-overflow case already publishes a `turn_dropped` transcript item, and
  this receipt is additive to that item, not a replacement for it. The
  `NO_LIVE_EXECUTION` case must not consume the command: dropping a turn *and*
  marking it delivered is the silent loss this contract exists to remove.
- `turn_started` — when the run loop actually begins the turn, carrying the
  `turnId` the provider mints for it. **This is also the point the command is
  consumed** — never on receipt — so a provider that dies with turns waiting
  replays them from its watermark on restart, in `(created_at, id)` order,
  and each one runs exactly once.
- `interrupt_delivered` — when a `thread.turn.interrupt` caused a cancel to be
  issued to a turn the provider is running or has taken custody of. A provider
  answers this from what it holds, not from a lagging fold of its own session
  reports: a turn and an interrupt sent back to back must not be answered
  "nothing to cancel" by a cancel that in fact lands.
- `turn_refused` with `NO_TURN_IN_FLIGHT`, `NO_LIVE_EXECUTION`, or
  `QUEUE_FULL` — the three ways a `thread.turn.interrupt` finds nothing to
  cancel or cannot be handed over. All three are terminal and recorded as
  refused.
- `turn_refused` — for every `TurnDecision::Fail` (`UNAUTHORIZED_OPERATOR`),
  and for a `TurnDecision::Ignore` whose reason names a target this provider
  owns: `UnknownTarget`, `StaleGeneration`, `SessionClosed`. An `Ignore` for
  `NotAddressed`, `AlreadyConsumed`, `PastHorizon`, or a malformed command
  stays silent — no receipt — because those reasons say the command was never
  this provider's to answer, and publishing one would be cross-provider
  chatter about someone else's command.

A provider that refuses a command records that command id durably alongside
its consumed ids (pruned at the same horizon), so a relay redelivery of the
same 44220 does not republish a byte-identical `turn_refused` under the same
semantic key.

**A turn receipt never creates, confirms, or ends a generation.** Only the six
lifecycle statuses do that (`created`, `created_with_failed_initial_turn`,
`failed`, `resumed`, `resumed_without_context`, `stopped`). Any fold that reads
`kind:44224` by `commandId` to decide whether a generation exists, is
confirmed, or has ended — the catalog, create observations, `bee sessions`
generation resolution — MUST ignore every turn status for that purpose. A turn receipt is evidence
about one turn, never about the generation's existence or lifecycle.

The pending-turn UI settles a queued command on the `user_prompt` transcript
echo whose `commandId` equals the pending command's `commandId` (see
[NIP-CST](NIP-CST.md)), not on `turn_started` alone — the receipt says the
provider began a turn; the echo says what it began. `turn_queued` updates a
pending row to say the provider has queued it, and exempts that row from the
client's unanswered-row expiry — a turn queued behind an hour of work is still
coming, and the row ages visibly instead of vanishing. `turn_degraded`
relabels the row to say the steer was downgraded to a boundary delivery and
leaves the words sent. `turn_dropped` and `turn_refused` remove the row,
restore the draft, and surface `error.code`/`error.message`.

The ACP session id used as a resume cursor is sensitive host-local state. It
MUST NOT appear in commands, receipts, metadata, transcripts, adapter
environment variables, or logs. Provider state containing it MUST be
owner-readable only on platforms that expose filesystem permissions.

Both kinds are provider-authored, so the relay applies scope, `h` scope, strict
membership, and a size cap (32 KiB metadata, 16 KiB receipt) and nothing more.
It does not parse the **durable 44223/44224 provider-fact content**: that content
is the provider's account of what its own session did, and a relay that
validated those observations would be asserting authority over facts it never
observed. Consumers verify signature, trusted signer, channel visibility,
exact tags, target, and semantic key at their own ingress boundary. This rule
does not apply to kind-24223 leases: their deliberately narrow liveness
envelope is parsed and authority-checked by the relay before it updates the
ephemeral register.

Provider outboxes fence each publication by semantic key, current provider
signing pubkey, and exact event kind, so a signing-key rotation cannot reuse an
event signed by the previous key.

## Ephemeral generation leases

Kind `24223` (`KIND_CODING_SESSION_LEASE`) is a provider-signed, channel-scoped
ephemeral assertion about one exact generation. It is not durable session state
and does not prove human attention, current code-area conflict, or continuous
transport connectivity. A current `live` lease proves only that the authorized
provider owned a live actor when it most recently renewed.

Strict content has exactly four fields, with no unknown or duplicate keys:

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

`state` is exactly `live` or `released`. `leaseSequence` is a positive
JavaScript-safe integer, monotonically reserved and persisted before signing.
Gaps are permitted; reuse is forbidden. Signed content is capped at 2 KiB;
target and lifecycle-command identifiers retain the 256-byte NIP-CSC limit.

Tags are closed, exactly two fields each, and appear in this exact order:

```text
["h", "<canonical channel UUID>"]
["cslease-v", "cslease1-1"]
["cs-target", "<coding_session_target_key(target)>"]
["csl-command", "<commandId that minted this exact generation>"]
["cslease-seq", "<canonical decimal leaseSequence>"]
```

The relay rejects malformed content, tag/content target or sequence mismatch,
non-canonical values, an event more than 180 seconds old on first acceptance,
or a timestamp more than 30 seconds ahead of relay time. These timestamp rules
are replay admission only. The 180-second Redis TTL starts from the relay's
acceptance time (Redis `TIME`), never the provider timestamp. Providers renew
eligible `live` actors every 60 seconds and publish a higher-sequence
`released` tombstone before a clean terminal transition.

Lease signing authority comes from the accepted lifecycle chain, never from
metadata authorship. For the tagged channel, `csl-command`, and `cs-target`, the
relay requires exactly one strictly valid accepted kind-44221 create/resume
command and exactly one successful kind-44224 receipt. The receipt target must
equal the lease target, and both the receipt signer and lease signer must equal
the command's `providerAuthorityPubkey`. Missing, conflicting, stop-minted, or
otherwise ambiguous evidence fails closed.

Leases are WebSocket-published only and are never inserted into Postgres. Redis
retains the full original signed event plus relay acceptance/expiry and
command/receipt provenance. Public visibility and the channel expiry index use
the 180-second lease TTL. The per-target monotonic register is retained out of
band for 211 seconds (the 180-second replay window plus 30 seconds of allowed
future skew and a one-second boundary fence), so an expired `released` event
still rejects every delayed lower-sequence `live` event that could pass replay
admission. Higher sequence replaces lower; an exact duplicate is idempotent
without refreshing either lifetime; equal-sequence different-event conflicts
and lower sequences are rejected. Cold queries require explicit channel scope
and return the original provider-signed event only while its public lease is
unexpired.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constants | `crates/buzz-core/src/kind.rs` |
| Payload + `projectRef` / `sessionRef` validation | `crates/buzz-core/src/coding_session_lifecycle_command.rs` |
| Lease payload + strict envelope / replay validation | `crates/buzz-core/src/coding_session_lease.rs` |
| Envelope validation, membership, size caps | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builders | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
