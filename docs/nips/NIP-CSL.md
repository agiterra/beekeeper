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
    "providerInstanceRef": "capability-advertised-instance",
    "providerAuthorityPubkey": "64-lowercase-hex-catalog-signer",
    "model": "provider-neutral-model-id",
    "title": "Operator-facing session title",
    "initialTurn": "Inspect the project and begin."
  }
}
```

`providerInstanceRef` and `providerAuthorityPubkey` are required. The provider
authority is exactly the lowercase 64-hex signer of the selected
[provider-catalog](NIP-CSPC.md) event; it addresses the command to one adapter
even when several share a channel. `projectRef`, `repoRef`, `model`, `title`,
and `initialTurn` are nullable; a present string must be nonempty after
trimming. Limits are UTF-8 byte limits:

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

**No host-local state travels in signed content.** Not paths, not environment
variables, not secrets, not session IDs, not generations. The working directory
in particular is machine-local configuration the producer resolves for itself;
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
| Payload + `projectRef` validation | `crates/buzz-core/src/coding_session_lifecycle_command.rs` |
| Envelope validation, membership, size caps | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builders | `crates/buzz-sdk/src/builders.rs` |
| Semantic keys | `crates/buzz-sdk/src/coding_session.rs` |
