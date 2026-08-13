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
>    below. Each fact is published exactly once, in exactly one kind.
> 2. **`projectRef` is optional.** A session may stand alone, owned by the
>    channel it is published into rather than by a project. See below.
> 3. **`projectRef` coordinates are `30621:` only.** The donor's example used
>    the `30178:` team-catalog kind; sessions here bind to NIP-MP projects
>    (`kind:30621`) and nothing else.
> 4. **`sessionRef` groups executions into an umbrella session.** A nullable,
>    client-minted UUID added after v1 shipped; the action is exactly the
>    historical 8-key form or exactly the 9-key form including it. See below.
> 5. **Continuation is generation-fenced.** `session.resume` and
>    `session.stop` address an exact published `cs-target`. Resume never carries
>    the provider's opaque ACP cursor; that cursor remains host-private. A
>    successful reattachment publishes a new generation, while stop is durable
>    intent that survives provider restart.

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
`title`, and `initialTurn` are nullable; a present string must be nonempty
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
therefore **exactly the 8-key v1 set, or exactly the 9-key set including
`sessionRef`** — nothing between, nothing beyond. New producers always write
the key (explicit `null` or a UUID); the 8-key form is accepted only as the
historical form. This is the versioned additive discipline the explicit-null
rule exists to protect: an omitted key on a *new* event is still
indistinguishable from truncation, so new clients never omit it — but the
decoder cannot reject the past.

Optional is still not unvalidated. The reference travels in no tag, so nothing
downstream normalizes it; two clients agree on umbrella membership only if the
bytes are byte-exact. A present `sessionRef` must therefore be canonical —
uppercase hex, braces, URN prefixes, and truncations are rejected rather than
coerced, because a non-canonical spelling would silently split an umbrella in
two.

No tag carries the reference, so umbrella and non-umbrella creates produce
identically shaped envelopes — the same property `projectRef` has. The relay
learns nothing new: it validates through this same decoder and remains a
validating store. Grouping, founder authority (the signer of the earliest
create bearing a `sessionRef`), and rendering are consumer concerns.

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

The provider publishes generation metadata as `kind:44223` and lifecycle
receipts as `kind:44224` — one event per fact, native kind only.

Metadata tags, in order: `h`, `csm-v` (`csm1-1`), `cs-target`, `csm-key`.
Receipt tags, in order: `h`, `cslr-v` (`cslr1-1`), `csl-command`, `csl-key`.

Semantic keys are the same length-prefixed encoding NIP-CSC describes:

```
coding-session-metadata/v1|<driver><instanceId><sessionId><generation>
coding-session-lifecycle-receipt/v1|<commandId>
```

A receipt is keyed by `commandId` alone: one lifecycle command has exactly one
outcome, so a second receipt for the same command is a duplicate to drop, never
a revision to apply. Metadata is immutable per generation — a correction is a
new generation, not a rewrite.

The receipt keeps the same exact five-key v1 object. In addition to the create
statuses, lifecycle continuation uses:

- `resumed`: `session` is the new-generation target and `error` is `null`;
- `resumed_without_context`: `session` is the new-generation target and
  `error` is `{ "code": "CONTEXT_NOT_RECOVERED", "message": "..." }`;
- `stopped`: `session` is the stopped exact target and `error` is `null`;
- `failed`: unchanged, with `session: null` and a stable error object.

An old consumer that does not recognize a new status rejects that receipt; it
must never coerce the outcome into `created`. The new generation's metadata and
transcript remain independently verifiable facts.

The ACP session id used as a resume cursor is sensitive host-local state. It
MUST NOT appear in commands, receipts, metadata, transcripts, adapter
environment variables, or logs. Provider state containing it MUST be
owner-readable only on platforms that expose filesystem permissions.

Both kinds are provider-authored, so the relay applies scope, `h` scope, strict
membership, and a size cap (32 KiB metadata, 16 KiB receipt) and nothing more.
It does not parse them: their content is a provider's account of what its own
session did, and a relay that validated it would be asserting authority over
facts it never observed. Consumers verify signature, trusted signer, channel
visibility, exact tags, target, and semantic key at their own ingress
boundary.

Provider outboxes fence each publication by semantic key, current provider
signing pubkey, and exact event kind, so a signing-key rotation cannot reuse an
event signed by the previous key.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constants | `crates/buzz-core/src/kind.rs` |
| Payload + `projectRef` / `sessionRef` validation | `crates/buzz-core/src/coding_session_lifecycle_command.rs` |
| Envelope validation, membership, size caps | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builders | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
