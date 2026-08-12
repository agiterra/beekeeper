# NIP-CSPC: Coding-Session Provider Catalog

`kind:44222` is a provider-authored, channel-scoped advertisement of the
coding-session drivers, models, and capabilities one provider instance can
serve. Operators pick a target from it; the resulting
[NIP-CSL](NIP-CSL.md) create command names the catalog's signer as its
`providerAuthorityPubkey`, which is how a command addressed to one adapter is
ignored by every other adapter sharing the channel.

Related: [NIP-CSC](NIP-CSC.md) (turn commands), [NIP-CSL](NIP-CSL.md)
(create + lifecycle facts), [NIP-CST](NIP-CST.md) (transcript items).

> **Fork amendments.**
>
> 1. **`kind:44222` is native.** The donor kept the provider catalog
>    TypeScript-only, riding it in on kind 9 with a `cspc-v` tag as the
>    discriminator. This fork owns its relay, so the catalog gets a real kind
>    and there is no kind-9 form to recognize.
> 2. **`providers[]` moves to the top level.** The donor nested every provider
>    inside a project, which made a provider undiscoverable unless a project
>    existed. Since sessions here can be standalone
>    ([NIP-CSL](NIP-CSL.md)), the catalog's primary content is a
>    project-independent `providers[]`, and `projects[]` is an optional
>    refinement.

## Wire contract

Content is exactly this JSON shape:

```json
{
  "schema": "buzz-coding-session-provider-catalog/v1",
  "revision": 3,
  "providers": [
    {
      "providerInstanceRef": "claude-primary",
      "driver": "claude-agent-acp",
      "runtime": "claude-code",
      "defaultModel": "claude-sonnet-4-6",
      "allowedModels": ["claude-sonnet-4-6", "claude-opus-4-1"],
      "capabilities": {
        "threadTurnStart": true,
        "threadTurnInterrupt": true,
        "threadSteer": false,
        "context": true,
        "diff": true,
        "plan": true
      }
    }
  ],
  "projects": [
    {
      "projectRef": "30621:<lowercase-64-hex-owner>:<project-d>",
      "repoRef": "30617:owner:repository",
      "providers": ["claude-primary"]
    }
  ]
}
```

`revision` is a positive JavaScript-safe integer, monotonic per (channel,
signer); consumers keep the highest revision they have seen from a given signer
in a given channel and discard the rest. The kind is **not** replaceable — the
relay stores every advertisement — so a provider's capability history stays
auditable rather than being silently overwritten.

`providers[]` is required and project-independent: it is the full set of
session targets this signer offers in this channel. Each entry is identified by
`providerInstanceRef`, unique within the catalog.

`projects[]` is optional. When present, each entry narrows which of the
top-level providers may serve a given project (and optionally a repository
within it) by naming their `providerInstanceRef` values; `repoRef` may be
`null`. A `providerInstanceRef` that does not appear in the top-level
`providers[]` makes the catalog malformed. A catalog with no `projects[]` at
all offers every listed provider to every session in the channel — which is the
standalone-session case.

Bounds:

- complete event content: 256 KiB
- all references and model identifiers: 2 KiB each
- `providers[]`: 32 entries
- `allowedModels[]`: 64 entries
- `projects[]`: 512 entries

### Canonical form

The catalog must be **canonically serialized**, and a consumer rejects any
event whose re-serialized parse does not equal the signed content byte for
byte. Canonical means:

- object keys in the order shown above
- `providers[]` sorted by `providerInstanceRef`
- `allowedModels[0]` equal to `defaultModel`, with the remainder sorted
- `projects[]` sorted by `(projectRef, repoRef ?? "")`, no duplicate pair
- each project's provider-ref list sorted, no duplicates

Canonical form is what makes `cspc-key` (below) meaningful: two providers
advertising the same capabilities produce the same bytes, and any difference in
bytes is a real difference in what is being offered.

### Tags

Exactly these four two-field tags, in this order:

1. `["h", "<channel UUID>"]`
2. `["cspc-v", "cspc1-1"]`
3. `["cspc-revision", "<decimal revision>"]`
4. `["cspc-key", "<semantic key>"]`

`cspc-revision` must equal the `revision` inside the content. The semantic key
is the length-prefixed structured encoding described in
[NIP-CSC](NIP-CSC.md), over the channel id, the revision, and a lowercase-hex
SHA-256 of the exact signed content:

```
coding-session-provider-catalog/v1|<channelId><revision><sha256(content)>
```

Digesting the content, not just the revision, is deliberate: a provider that
bumped its revision without changing anything, or reused a revision with
different content, is then visibly distinguishable to a consumer rather than
silently collapsing into one entry.

## Authority

The relay requires `messages:write`, a valid `h` channel scope, **active
channel membership with no open-channel fallback**, and a 256 KiB content cap.
It does not parse the catalog. A catalog is a provider's claim about its own
capabilities; relay-side validation would be the relay vouching for a claim it
cannot verify.

Trust is the consumer's, at its ingress boundary: Nostr signature, configured
trusted signer set, visible channel, exact ordered tags, revision agreement,
semantic-key agreement, and canonical-form agreement — all before any target
from the catalog is offered to an operator. Catalogs from untrusted signers are
counted and discarded, never rendered.

Two trusted signers advertising conflicting coordinates in one channel is a
conflict, not a merge: consumers surface it rather than picking a winner.

## Implementation

| Concern | Location |
| --- | --- |
| Kind constant | `crates/buzz-core/src/kind.rs` |
| Membership, size cap | `crates/buzz-relay/src/handlers/ingest.rs` |
| Builder | `crates/buzz-sdk/src/builders.rs` |
| Semantic key | `crates/buzz-sdk/src/coding_session.rs` |

Donor reference for the content shape (pre-amendment):
`desktop/src/features/coding-sessions/lib/codingSessionProviderCatalog.ts`.
