NIP-MP
======

Multi-Repository Projects
-------------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format, addressable events), NIP-34 (git repositories), NIP-09 (event deletion). Interacts with NIP-29 (the channel a project links to) and NIP-OA (owner attestation, for how agents inherit repo push access).

## Abstract

This NIP defines `kind:30621`, an addressable **project** event: a signed, named grouping of NIP-34 repository announcements (`kind:30617`). A project references its member repositories by coordinate, so one project may span repositories owned by different pubkeys, and one repository may belong to several projects.

A project is metadata only. Its signer gains no authority over any member repository — not to edit it, delete it, push to it, or administer it. Membership is an assertion about grouping, not a grant of permission.

## Motivation

Buzz renders one card per `kind:30617`, so "the platform" — a relay, a desktop app, and a mobile app — appears as three unrelated repositories. Real work spans repositories; the model does not.

[VISION_PROJECTS.md](../../VISION_PROJECTS.md) sets the bar as "standard kinds as substrate, custom kinds only where genuinely novel," and every other forge concept in Buzz clears it: repositories, patches, issues, statuses, and ref state are all standard NIP-34 kinds. Multi-repository grouping is the one semantic that cannot be:

- **Per-repository tags cannot express cross-owner grouping.** If membership lived in each `kind:30617`, a project spanning Alice's and Bob's repositories would require *both* Alice and Bob to publish a tag naming the group. Alice cannot enroll Bob's repository; she cannot sign for his key. Grouping would be possible only within a single owner's repositories, and would break the moment a repository changed hands or a fork joined.
- **Project-level metadata has no owner.** A project name, description, and linked channel describe the *group*, not any one repository. Scattered across per-repository tags they have no single writer, no replacement semantics, and no deletion story: removing a repository from the group means editing an event you may not control.
- **Existing list kinds do not fit.** NIP-51 sets (`kind:30004` curation sets and friends) are private-or-public user bookmarks over arbitrary content, not a shared, named, addressable container for a forge collection with its own channel binding and visibility. Overloading a curation set would make every project indistinguishable from a user's reading list.

One custom kind, held by one signer, with all group state in one replaceable event, resolves all three. The cost is bounded and stated plainly: `kind:30621` is Buzz-specific, so a third-party NIP-34 client sees the member repositories individually and ignores the grouping. Nothing degrades — the repositories remain standard, portable `kind:30617` events, discoverable and renderable exactly as before.

## Non-Goals

This NIP does not define shared or delegated project editing — a project is replaceable only by its own signer (see [Authority](#authority)).
This NIP does not define any authorization over member repositories **through a project's own `a` tags**. Forward-referencing a repository is not a permission grant. Authorization flows only the other way, from the repository's own `["project", …]` back-reference — see [Repository access](#repository-access).
This NIP does not define project-level branch protection, CI, or workflow configuration.
This NIP does not define nested projects. A project's members are repositories, never other projects.
This NIP does not require relays to verify that a member coordinate resolves to an existing repository — a project may reference a repository that does not exist yet, or no longer does.

## Terminology

This document uses MUST, MUST NOT, SHOULD, SHOULD NOT, MAY, and RECOMMENDED as defined in RFC 2119.

- **project**: A `kind:30621` event. Also called the *container*.
- **member**: A repository referenced by a project, named by an `a` tag holding a repository coordinate.
- **coordinate**: The NIP-01 address of a repository announcement, `30617:<owner-pubkey-hex>:<repo-d-tag>`.
- **explicit project**: A project that exists as a `kind:30621` event.
- **implicit project**: The single-repository card a client renders for a `kind:30617` that no listing-eligible explicit project claims. Not an event — a rendering fallback.
- **listing eligible**: A project a client is currently rendering in its project collection. See [Listing eligibility](#listing-eligibility).

## Kinds

| Kind | Name | Signer | Class | Purpose |
|------|------|--------|-------|---------|
| `30621` | Project | user | addressable | A named grouping of `kind:30617` repository announcements |
| `9010` | Put member | user | regular | Buzz roles extension: add project members or change their roles — see [Relay-managed membership](#relay-managed-membership-buzz-extension) |
| `9011` | Remove member | user | regular | Buzz roles extension: remove project members |
| `39010` | Roster projection | relay | addressable | Buzz roles extension: relay-signed authoritative roster, `d` = the project coordinate |

`kind:30621` is an addressable event per NIP-01 (`30000 <= n < 40000`), addressed by `(pubkey, 30621, d)`. Two signers may use the same `d` value; those are two distinct projects. Addressable events were formerly specified as "parameterized replaceable events" in NIP-33, which upstream has since folded into NIP-01; this document cites NIP-01 throughout.

### Kind allocation

`30621` sits in the NIP-34 git block (`30617` repository announcement, `30618` repository state), which is where a reader looks for a forge concept. Checks performed before freezing the number:

| Registry | Checked | Result |
|----------|---------|--------|
| Upstream nostr NIPs event-kind table (`nostr-protocol/nips` `README.md`, at commit `6d2979b3f503a8539c983efbcdcf901bbcf9ed23`) | `30610`–`30629` | Only `30617` and `30618` are assigned. `30621` is unassigned. |
| nostrbook.dev kind registry (`https://nostrbook.dev/kinds/<n>`) | `30617`, `30618`, `30620`, `30621`, `30622` | `30617` and `30618` documented (HTTP 200). `30620`, `30621`, `30622` all HTTP 404 — no entry. |
| This repository (`crates/buzz-core/src/kind.rs`) | full range | `30620` is `KIND_WORKFLOW_DEF`, `30622` is `KIND_DM_VISIBILITY` (NIP-DV). `30621` is the one free number between them. |

Both external registries are advisory, not authoritative allocators: neither reserves numbers, and an unregistered kind may still be in use by an unpublished client. A future upstream assignment of `30621` would be a collision Buzz absorbs the same way it already does for its other custom kinds — the number is Buzz-specific, and interoperability rests on the member `kind:30617` events, which remain standard.

## Event Format

```jsonc
{
  "kind": 30621,
  "pubkey": "<project-signer-pubkey-hex>",
  "content": "",
  "tags": [
    ["d", "platform"],
    ["name", "Platform"],
    ["description", "Relay, desktop, and mobile for the platform team."],
    ["a", "30617:<owner-a-pubkey-hex>:buzz"],
    ["a", "30617:<owner-b-pubkey-hex>:buzz-infra"],
    ["buzz-channel", "<channel-uuid>"],
    ["buzz-visibility", "listed"]
  ]
}
```

| Tag | Cardinality | Meaning |
|-----|-------------|---------|
| `d` | exactly 1, non-empty | Project slug. The NIP-01 addressable identifier. |
| `name` | 0 or 1 | Human-readable display name. Clients fall back to `d` when absent. |
| `description` | 0 or 1 | Free text describing the project. |
| `a` | 0 to 64 | One member repository coordinate each. Order is not significant. |
| `buzz-channel` | 0 or 1 | UUID of the channel this project's discussion lives in. Metadata only — see [Authority](#authority). At most 256 bytes. |
| `buzz-visibility` | 0 or 1 | `listed` (default) or `unlisted`. Feeds [listing eligibility](#listing-eligibility). At most 256 bytes. |
| `buzz-access` | 0 or 1 | `public` (default) or `private`. Buzz access extension — see [Access levels](#access-levels-buzz-extension). |
| `icon` | 0 or 1 | Display emoji for the project (unicode or `:shortcode:`). Client-interpreted. At most 256 bytes. |
| `color` | 0 or 1 | Display tint for the project as lowercase `#rrggbb`. Client-interpreted. At most 256 bytes. |
| `p` | 0 to 256 | One invited-member pubkey each (lowercase 64-hex, optional NIP-01 relay hint, optional role in element 4 — see [Member roles](#member-roles)). Meaningful only with `buzz-access` `private`. |

`content` carries no meaning. Writers SHOULD emit the empty string. Readers and relays MUST ignore whatever it holds: a non-empty `content` is not a rejection cause, and no consumer may parse semantics from it. Reserving it costs nothing and keeps a future writer that fills it from invalidating its events for today's readers.

Unrecognized tags MUST be ignored rather than rejected, so a newer writer can add metadata without invalidating its events for older readers.

### Metadata interpretation

Ingest bounds metadata cardinality and length; it interprets no metadata value. `buzz-channel` and `buzz-visibility` are opaque strings to a relay, exactly as they are on `kind:30617`. Interpretation is a client concern, and every client MUST resolve it the same way:

- `name` absent → clients display the `d` value.
- `buzz-visibility` absent or holding any value other than `listed` or `unlisted` → treated as `listed`. An unrecognized token MUST NOT hide a project: a typo in a metadata field is not a privacy signal, and treating it as one would make a project vanish for reasons its author cannot see.
- `buzz-channel` absent, or naming a channel the viewer cannot resolve or read → the project renders without a channel link. It MUST NOT be dropped from the collection, and the unresolvable value MUST NOT be surfaced as a broken link.
- `icon` and `color` are display hints. A `color` value that is not `#rrggbb` (case-insensitive) MUST be treated as unset — never a rejection, never an error surfaced to the reader.

### Access levels (Buzz extension)

`buzz-access` sets who may **read** the project container and, on the Buzz relay, its contained channels and forums. It is the one metadata tag the relay interprets rather than treats as opaque, because it is an access-control input, not a display hint.

- Absent or `public` — the container is community-readable. This is the default and describes every pre-extension event.
- `private` — the relay withholds the container from every reader except the **author** and the project's current **roster** ([Relay-managed membership](#relay-managed-membership-buzz-extension)): head-sourced `p` tags until the first membership op is accepted, ops-sourced thereafter. Enforcement covers every read surface: REQ historical delivery, live fan-out, COUNT, `ids` lookup, the HTTP bridge, and FTS search. Channels bound to a private project (via `project_ref`) are additionally excluded from non-members' accessible-channel sets, hiding their messages and forums and refusing writes.

A relay MUST read the container gate from the same roster every other surface uses, and MUST NOT read it from the head's `p` tags once the roster is ops-sourced. The head is signed by the creator alone and no op can rewrite it, so a `p`-tag gate diverges in both directions the moment a roster op lands: a member a `9010` seated cannot see the container that names their project, and a member a `9011` removed keeps reading it forever. Both are failures of this section, not of [Roster source](#relay-managed-membership-buzz-extension).

The owner is an implicit member and never appears in `p` tags. `buzz-visibility` is orthogonal: it filters what a client *lists* among events the reader already received; `buzz-access` controls what the relay *delivers* at all.

Two deliberate asymmetries with the rest of this NIP:

- **Unknown values are rejected at ingest** (`access-value`), unlike `buzz-visibility`'s fall-back-to-default rule. A display-hint typo is harmless; an access typo that silently fell open to public would be a privacy leak.
- **A project's member list never gates repositories.** Consistent with [Authority](#authority), forward-referencing another owner's repository via an `a` tag grants and removes nothing on it. Repository gating exists (below), but it is opted into exclusively by the **repository owner's own** `project` back-reference on their `kind:30617`.

Because only the owner can replace the event, head-carried invitations are owner-curated republishes ([Relay-managed membership](#relay-managed-membership-buzz-extension) supersedes them once the first membership op is accepted). A relay hint in element 3 of a `p` tag is permitted and ignored, mirroring member `a` tags. Vanilla relays that do not implement this extension deliver private containers like any addressable event — writers targeting such relays must not rely on `buzz-access`.

Two additional rules complete the access grammar:

- **The `general` project is always public** (`access-general-forced-public`). The community's shared default project — `d` = `general` — can never carry `["buzz-access", "private"]`: it is the container everything falls back into, so a hand-built head must not be able to hide it. Ingest rejects the combination outright.
- **Clients SHOULD default new projects to private.** An access decision is easy to loosen and painful to walk back: content published into a public project has already been delivered. Buzz's own clients (and `bee projects create`) default to `private` and require an explicit choice to create a public project.

Invites target **agents exactly like users**: an invited member is a pubkey, and whether that pubkey belongs to a person or an agent is invisible to this grammar. An agent invited into a project reads and writes under the same role rules as any member. Agent-side consent — an agent acknowledging or declining an invitation before acting inside a project — is future work; today an invite is effective the moment the roster carries it.

Beekeeper's desktop applies this to the agents it manages for a project: an identity it mints for a project's role, or one its owner associates with the project, is put on the roster as `collaborator` by the owner's own kind 9010, so the seat's key reads and writes the project like any member. Dissociation removes it (kind 9011). Association alone never confers access; the roster op does.

#### Member roles

Element 4 of an invited-member `p` tag carries the member's **role**, mirroring the NIP-29 `39002` tag grammar:

```jsonc
["p", "<pubkey-hex>", "<optional-relay-hint>", "<role>"]
```

The tag's arity is 2 to 4 (`invite-tag-arity`). The pinned role vocabulary is `owner`, `collaborator`, `viewer` — an unknown role is rejected at ingest (`invite-role`) rather than defaulted, because a role typo must not silently grant or deny. A **role-less invite** (arity 2 or 3) is a legacy **collaborator**: pre-role members could already write, so the compatibility reading preserves what they had.

| Capability | Owner | Collaborator | Viewer |
|------------|-------|--------------|--------|
| Read the container and its contents | ✓ | ✓ | ✓ |
| Write into project contents (channels, forums, own sessions, repos) | ✓ | ✓ | — |
| `clone`/`fetch` a member repository over git ([Repository access](#repository-access)) | ✓ | ✓ | ✓ |
| `push` to a member repository | as `owner` | as `member` | — |
| Manage the roster (kinds `9010`/`9011`) | ✓ | — | — |
| Edit head metadata (name, description, channel, access) | — | — | — |

Head metadata stays **creator-only in v1** — a documented limitation, not an oversight. The head is an addressable event keyed on the creator's pubkey, so NIP-01 replacement admits no other writer; extending metadata editing to roster owners would require a relay-managed metadata surface like the roster ops below, and is deferred.

The creator (the pubkey in the project's address) is always an **implicit owner** and never appears in `p` tags or the roster.

### Relay-managed membership (Buzz extension)

Head-carried `p` tags require an owner republish per roster change and cannot express delegation. Two relay-processed op kinds move the roster out of the head, modeled on the NIP-29 `9000`-series: user-signed, validated, applied to the project ACL, then projected into a relay-signed roster event.

**Roster source.** A project's roster is **head-sourced** until the first membership op is accepted, and **ops-sourced** thereafter. The flip is one-way: once any op has been accepted, the head's `p` tags are ignored for access decisions — a creator republishing their head with a different `p` set changes nothing, so a stale republish (or a compromised creator device replaying an old head) cannot evict members the ops added. The head's `p` tags remain valid envelope grammar either way.

**`kind:9010` put-member** — add members or change their roles:

```jsonc
{
  "kind": 9010,
  "tags": [
    ["a", "30621:<owner-hex>:<dtag>"],
    ["p", "<member-pubkey-hex>", "", "<role>"]
  ],
  "content": ""
}
```

- Exactly one `a` tag holding the **canonical** project coordinate (lowercase-hex owner — the ACL projection joins on string equality).
- 1 to 64 `p` targets per op, each **exactly** arity 4 with a pinned-vocabulary role. The role is **required**, not defaulted: an op is a deliberate grant, and a missing role must not silently pick a tier. Duplicate targets are rejected.
- Re-putting an existing member changes their role.

**`kind:9011` remove-member** — same `a` tag rule; 1 to 64 `p` targets of arity 2 or 3 (no role — removal has no tier).

**Authorization.** The signer must be the project **creator** or a roster **owner**. The creator can never be a target of either op — they are the project's address, so adding, re-roling, or removing them is meaningless and is refused outright. Ops against an unknown project are refused (an op cannot create a project). The roster is capped at **256 members**; each op carries at most **64 targets**.

**`kind:39010` roster projection** — the authoritative roster read. After every accepted op (and after head-sourced roster changes), the relay emits a relay-signed addressable event whose `d` tag is the project coordinate, with one `["p", <hex>, "", <role>]` tag per member:

```jsonc
{ "kinds": [39010], "#d": ["30621:<owner-hex>:<dtag>"] }
```

Clients MUST read the roster from the latest `39010`, falling back to the head's `p` tags only when no projection exists (the roster is still head-sourced). `39010` is never client-submitted — the relay rejects a submitted one. Both op kinds and the projection are withheld from readers outside a private project by the same coordinate predicate that hides the container's other content.

#### Repository access

A repository joins a project by carrying `["project", "30621:<owner>:<project-d>"]` on its own `kind:30617` announcement — the repo owner's assertion, so gating on it never lets a stranger's project hide or expose someone else's repository.

**Event surface (private projects only).** When that coordinate resolves to a **private** project, the Buzz relay:

- **Hides the repository's event surface** from readers outside the project: the `kind:30617` announcement, the relay-signed `kind:30618` ref state, and every NIP-34 child event that `a`-tags the repository (`1617` patches, `1618`/`1619` PRs, `1621` issues, `1630`–`1633` status), across the same read surfaces as the container gate. An event's own author always sees it; the repository owner is always admitted.
- **Gates writes**: `kind:30618` and the child kinds targeting the repository are rejected (`restricted:`) unless the author is the repository owner, the project owner, or an invited member. The relay's own key is exempt so relay-signed ref state can be emitted.

A public project applies neither: its contents are visible, so there is no gate to apply.

**Git transport (every project, at any visibility).** The roster is a git ACL in its own right, sitting alongside the repository's `buzz-channel` binding. A repository may carry either tag, both, or neither, and the relay resolves the caller's effective role as the **more permissive** of the two grants:

| Project role | `clone` / `fetch` | `push` |
|--------------|-------------------|--------|
| Owner (creator or roster `owner`) | ✓ | as NIP-29 `owner` |
| Collaborator | ✓ | as NIP-29 `member` |
| Viewer | ✓ | — |
| Not on the roster | — | — |

Neither ACL narrows the other: a channel `admin` is not demoted by also being a project `collaborator`, and a project `owner` is not demoted by also being a channel `guest`. `buzz-protect` rules apply to every pusher regardless of which path granted the role, owners included.

Two consequences are deliberate and stated rather than left to be discovered:

- **Visibility never grants.** A *public* project's repositories are no more cloneable than a private one's — only a roster row (or the implicit creator row) admits anyone. Public visibility governs the event surface above, not git transport. A relay MUST NOT infer a transport grant from visibility.
- **A repository with neither tag is unreachable.** It has no ACL, so the relay denies every clone/fetch/push (`no_channel_binding`) until its owner links a project or binds a channel. This is the vanilla-NIP-34-announcement case, and it is the *only* case that earns that remediation: a repository inside a project is legitimately unbound, and telling its pusher to bind a channel would be advice for a problem they do not have.

This is where a Buzz client stops asking for an "access channel" when a repository is created inside a project. The roster is already the membership mechanism; collecting a channel on top of it created a second, parallel ACL a user had to keep in sync by hand, and a repository whose two ACLs disagreed had no honest thing to display.

The `project` tag on `kind:30617` is validated at ingest, fail-closed like `access-value`: singleton, exactly two elements, and a well-formed coordinate (`repo-project-ref`) — a malformed value is rejected rather than silently ignored, because silently ignoring would publish a repository its author believes is private. Linking a repository **into a private project** additionally requires the announcement author to be admitted to that project (`repo-project-membership`); public or unresolvable coordinates stay soft references, matching channel `project_ref` semantics.

An unresolvable link fails open: a `project` coordinate naming a deleted or unknown project gates nothing, and a NIP-09 deletion of either the project head or the repository announcement reverts the repository's events to their own access rules — mirroring the container semantics above.

### Member coordinates

A member `a` tag follows NIP-01's `a` tag grammar: `["a", "<coordinate>"]` or `["a", "<coordinate>", "<relay-url>"]`. Ingest validates the tag's arity — exactly two or three elements — and the coordinate in element 1. The relay URL is opaque: it is never parsed and never a rejection cause by content. A fourth element has no meaning in this grammar and is rejected rather than ignored, so a writer cannot smuggle unbounded data into a position no consumer reads.

The optional third element is a **relay hint**: a recommended relay where the member announcement may be found. Clients MAY use it when resolving a member that step 6 of [the fold](#the-fold) would otherwise mark unavailable, and MUST treat it as advice rather than authority — a hint is unauthenticated, supplied by the project signer rather than the repository owner, so a resolution through it MUST still verify that the retrieved event is the coordinate's own signed `kind:30617`. A hint MUST NOT be required: a project whose members are all on the reading relay resolves fully without one, and a client that ignores hints entirely is conformant.

A member `a` tag coordinate MUST be exactly `30617:<owner>:<repo-d>` where:

- the kind segment is the literal `30617`. A project groups repository *announcements*; a coordinate naming any other kind (notably `30618` repository state) is malformed.
- `<owner>` is 64 lowercase hex characters. Uppercase is rejected: `#a` filter matching is byte-exact, so an uppercase-owner head would be invisible to the lowercase-coordinate queries every reader issues.
- `<repo-d>` is non-empty and is the `d` tag of the member repository announcement, taken **verbatim**.

Parsing splits on the first two colons only; everything after the second colon is `<repo-d>`. A repository whose `d` tag contains a colon is therefore addressable. Splitting on every colon would make such a repository permanently unaddressable by any project.

Buzz-hosted repositories cannot currently produce such a coordinate: their `d` values are validated as `[a-zA-Z0-9._-]{1,64}` (`crates/buzz-relay/src/handlers/side_effects.rs`, `crates/buzz-sdk/src/builders.rs`). The tolerance is for the repositories this NIP does not control — NIP-34 announcements from other clients, and any future relaxation of Buzz's own rule — and it matches how Buzz already parses coordinates in NIP-09 deletion handling, so a project coordinate and a deletion coordinate can never disagree about where a repository's `d` value begins.

Coordinate identity is the whole string. Two members sharing a `<repo-d>` under different owners — the NIP-34 fork case — are distinct members, not duplicates.

A project MAY reference a coordinate that resolves to nothing: a repository not yet announced, deleted, or announced on another relay. Clients render those members as explicitly unavailable ([Client Behavior](#client-behavior), step 6).

## Semantics

### Authority

The project signer's authority begins and ends at the container.

- **Over the container**: total. Only the signer can replace their `(pubkey, 30621, d)` coordinate. Deletion additionally admits the signer's registered NIP-OA owner — see [Deletion](#deletion).
- **Over member repositories**: none. No edit, no delete, no push, no administration, no ability to change a member repository's own metadata or protections. Adding Bob's repository to Alice's project changes nothing about Bob's repository or who may push to it. It is Alice's signed assertion that the two belong together, and it is attributable to her key.

Clients MUST preserve each member repository's own owner provenance in the UI. A repository rendered inside a project must not appear to be owned or governed by the project signer.

`buzz-channel` on a project is **metadata only**. Git push policy reads the `buzz-channel` of the repository's own `kind:30617` (`crates/buzz-relay/src/api/git/policy.rs`); a project neither overrides that binding nor supplies one to a member that lacks it. A project's channel binding therefore cannot widen or narrow push access to anything.

The project **roster** is a different matter, and the distinction is the whole of this section: a project's `a` tags are its signer's claim about *other people's* repositories and grant nothing, while a repository's own `project` back-reference is its owner's claim about *their own* repository and does grant — to the roster, per [Repository access](#repository-access). Both directions must exist for the asymmetry to be safe. If forward references granted, Alice could pull Bob's repository into her project and hand her roster push access to it; because only the back-reference grants, the only key that can place a repository behind a roster is the key that already controls it.

### Editing model

Editing is **owner-only**: publish a replacement `kind:30621` with the same `d` and a newer `created_at`. Adding, removing, or reordering members and changing metadata are all one operation — replacing the container. This falls out of the addressable-event model with no relay-side permission machinery; NIP-01 replacement already refuses to let one pubkey overwrite another's coordinate.

Delegated or maintainer editing is deliberately out of scope for this version. Adding it later needs no change to this event shape — only a new rule about who may replace a coordinate.

### Zero-member projects

A project with no `a` tags is valid. It is the natural state after removing a final member, and it carries only bounded metadata either way. Deleting the container — with its name, description, and channel binding — because its last repository was removed would be a destructive surprise for a reversible action.

Clients SHOULD require at least one member when *creating* a project, since an empty new project is almost always a mistake, and MUST render an existing empty project as an empty container rather than hiding it or treating it as malformed.

### Multiple membership

A repository may be a member of any number of projects. It renders inside each ([Client Behavior](#client-behavior), step 4). Membership is not exclusive and not a move: nothing about the repository event changes when it joins or leaves a project.

### Deletion

Deleting a project (NIP-09 `kind:5` naming the project coordinate) deletes the `kind:30621` only. Member repositories are untouched — their `kind:30617` events, refs, channels, and protections all survive, and each falls back to an implicit card unless another listing-eligible project claims it.

**Who may delete.** The project signer always may. On the Buzz relay, so may the signer's registered NIP-OA owner: `validate_standard_deletion_event` resolves the deletion's effective author and accepts it when that actor is the target pubkey's registered owner (`crates/buzz-relay/src/handlers/side_effects.rs`). This is a **Buzz relay extension to NIP-09**, applied uniformly to every kind rather than specially to projects — it is what lets a human clean up events published by an agent they own. Vanilla NIP-09 relays accept only the signer, so a project deleted through the owner path on Buzz will still be live on a relay that lacks the extension.

Replacement admits no such widening: it is signer-only on every relay, because NIP-01 keys the coordinate on the pubkey itself rather than on a permission check.

A deletion whose `created_at` precedes the live head does not remove it — see [Relay Processing Algorithm](#relay-processing-algorithm).

There is no cascade, in either direction. Deleting a member repository does not modify the project; the project keeps a coordinate that no longer resolves, and clients render it as unavailable.

## Relay Processing Algorithm

A relay accepting `kind:30621` MUST validate the envelope at ingest. The rule names below are the identifiers the shared fixtures use.

1. **`d-cardinality`** — exactly one `d` tag. Zero or several is rejected. Under NIP-01 a missing `d` is treated as empty, which collapses every such event into the `(pubkey, 30621, "")` slot where unrelated projects silently overwrite each other; several `d` tags make the address reader-dependent.
2. **`d-empty`** — the `d` value is non-empty. Same collapse hazard. Its length is bounded by the relay's existing generic `d`-tag limit (`buzz_db::event::D_TAG_MAX_LEN`, 1024 bytes); this NIP adds no second bound.
3. **`member-cap`** — at most 64 member `a` tags, counting **every** `a` tag rather than distinct coordinates. Counting distinct coordinates would leave parse volume bounded only by the relay frame limit (512 KiB by default, `crates/buzz-relay/src/config.rs`), since a duplicate-heavy event could carry thousands of tags naming one coordinate. The cap is inclusive: 64 is accepted, 65 is not.
4. **`member-tag-arity`** — every member `a` tag has exactly two or three elements, per NIP-01's `a` tag grammar. A one-element tag names no coordinate; a fourth element has no defined meaning, and ignoring it would let a writer park unbounded unvalidated data in a position no consumer reads. This is a separate rule from the next one because the failure is different: the tag's shape is wrong, not the coordinate it holds.
5. **`member-coordinate-malformed`** — every member `a` tag's coordinate (element 1) parses per [Member coordinates](#member-coordinates). The relay hint in element 3 is not parsed and MUST NOT be a rejection cause by its content.
6. **`member-duplicate`** — no two member `a` tags hold the same coordinate, compared as exact strings on the canonical form. Comparison is on the coordinate alone, so two tags naming one coordinate with different relay hints are duplicates.
7. **`metadata-cardinality`** — at most one each of `name`, `description`, `buzz-channel`, `buzz-visibility`. Duplicates would make the effective value reader-dependent.
8. **`metadata-length`** — `name` at most 256 bytes; `description` at most 2048 bytes; `buzz-channel` at most 256 bytes; `buzz-visibility` at most 256 bytes. The two `buzz-` bounds are generous by design: neither value has a semantic length, and the bound exists only so an unbounded string cannot ride into storage on a tag ingest does not interpret.

The Buzz access and roles extensions add seven rules over `buzz-access` and invited-member `p` tags:

9. **`access-value`** — a `buzz-access` tag holds exactly `private` or `public`. Unknown values are rejected rather than defaulted; see [Access levels](#access-levels-buzz-extension). Duplicate `buzz-access` tags fall under `metadata-cardinality` (rule 7).
10. **`access-general-forced-public`** — the community's shared default project (`d` = `general`) can never carry `buzz-access` `private`; see [Access levels](#access-levels-buzz-extension).
11. **`invite-cap`** — at most 256 invited-member `p` tags, counting every raw tag, same rationale as `member-cap`. Inclusive: 256 accepted, 257 not.
12. **`invite-tag-arity`** — every `p` tag has two to four elements (pubkey, optional relay hint, optional role). A fifth element has no defined meaning and is rejected rather than ignored.
13. **`invite-role`** — a present fourth element is a pinned [role](#member-roles) (`owner`, `collaborator`, `viewer`). A role typo must not silently grant or deny.
14. **`invite-malformed`** — every `p` value is 64 lowercase hex characters. The roster projection stores the decoded bytes and the read gate compares byte-exact against the authenticated reader's pubkey, so an uppercase invite would never match.
15. **`invite-duplicate`** — no two `p` tags hold the same pubkey.

Rules 3 through 6 are evaluated in that order, so an oversized tag list is refused on count before any per-tag parse or set proportional to it is built; rules 11 through 15 follow the same count-before-parse discipline.

The Buzz validator enforces all fifteen rules. The shared fixtures in [`NIP-MP.fixtures.json`](NIP-MP.fixtures.json) are wired as its test oracle: the relay's unit test suite runs every case against `validate_project_envelope` and asserts each `expect` outcome.

**Duplicates are rejected, never normalized.** A relay cannot dedupe tags inside a signed event: rewriting the tag array changes the event id and invalidates the signature. The choices are reject, or accept and require every present and future consumer to apply a first-wins interpretation rule. Rejecting keeps every stored head canonical and spares all consumers a defensive parse.

**No membership authorization.** The relay MUST NOT check whether the signer owns, maintains, or has any relationship to a member repository. Referencing another owner's repository is legal and is the point of the kind. Because membership grants nothing ([Authority](#authority)), there is nothing to authorize. (The inverse direction is different: a repository's own `kind:30617` back-reference **into a private project** is authorized at 30617 ingest — `repo-project-ref` / `repo-project-membership`, [Repository access](#repository-access) — because that link places the repository behind the project's ACL.)

**Routing.** `kind:30621` is global-only, like every other NIP-34 kind in Buzz: it is addressed by `(pubkey, kind, d)` and is never channel-scoped. A stray `h` tag MUST NOT scope it to a channel — the `buzz-channel` tag is a metadata reference, not a routing directive.

**Scope.** Writes require the `repos:write` scope, matching `kind:30617` and `kind:30618`. A project is repository metadata; a client authorized to announce repositories is authorized to group them.

**Replacement** follows NIP-01 with no special cases: newest `created_at` wins per `(pubkey, 30621, d)`, and one pubkey can never overwrite another's coordinate.

**Deletion** follows NIP-09 with two Buzz-wide behaviors that are not project-specific:

- A `kind:5` naming the coordinate deletes it when signed by the project signer **or** by that signer's registered NIP-OA owner ([Deletion](#deletion)).
- The deletion applies only to versions whose `created_at` is at or before the deletion's own, per NIP-09. A delayed or replayed tombstone signed before the current head MUST NOT remove it; the relay MUST compare timestamps at the coordinate (`soft_delete_by_coordinate`, `crates/buzz-db/src/event.rs`, whose inclusive `created_at <= <deletion>` bound is introduced alongside this specification in [#3171](https://github.com/block/buzz/pull/3171)).

## Client Behavior

### Listing eligibility

A project is **listing eligible** for a client when that client is currently rendering it in its project collection. A project is not listing eligible when:

- its `buzz-visibility` is `unlisted`, or
- the viewer has hidden it locally, or
- it has been deleted, or its latest head is otherwise not being rendered.

Only listing-eligible projects claim members. This keeps visibility deterministic in the case that otherwise breaks: an unlisted project must not make a repository the viewer can plainly see disappear from the collection, because the container that claims it is not on screen to hold it.

### Claim authority

A project **claims** a member — suppressing that repository's implicit card, per step 3 of the fold — only when the project is listing eligible *and* its signer is authorized by the member repository itself: the signer is the repository's owner (the pubkey in the member coordinate), or is listed in a `maintainers` tag on the repository's own live `kind:30617`.

Authority is therefore read from the member repository's *content*, not merely its existence: a client that has resolved only a coordinate, and not the head it names, cannot yet decide whether a project claims it. `maintainers` is the standard NIP-34 multi-value tag; Buzz's own announcement builder does not emit it today, so in practice every current claim reduces to signer-is-owner, and the `maintainers` clause is what keeps a co-maintained repository working the day that changes.

Without this rule, membership would carry exactly the authority [Authority](#authority) says it does not. Anyone may publish a project naming anyone's repository, so an unauthorized project that suppressed implicit cards would let a stranger pull someone else's repository out of the collection and into a container the owner never consented to — a signed assertion silently becoming control over another owner's discovery surface.

An unauthorized project still renders, and still renders its members inside itself: cross-owner grouping works, which is the entire point of the kind. What it cannot do is *remove* a repository from where its owner expects to find it. The visible consequence is that a repository in a stranger's project renders in both places — inside that project and as its own card — which is the correct reading of an unendorsed grouping claim.

### The fold

Given the set of repositories and projects to render, a client MUST derive the collection as follows.

1. **Enumerate exhaustively when possible.** Retrieve the latest live head of every `kind:30621` and `kind:30617` coordinate, plus the `kind:5` deletions bearing on them, using paginated queries. A fixed `limit` MUST NOT be used: with a limit of 200, repository 201 vanishes from the collection, which is precisely the compatibility guarantee this NIP owes existing repositories. What "to exhaustion" means depends on the cursor the relay offers — see [Pagination](#pagination). On a relay that does not provide an exhaustive mode, a client MUST mark the collection possibly incomplete rather than present a partial result as complete.
2. **Resolve members.** For each project, resolve each member coordinate to its repository head, and determine whether the project [claims](#claim-authority) each one.
3. **Suppress claimed implicit cards.** A live repository claimed by at least one project does not also render as an implicit single-repository card.
4. **Render multiple membership.** A repository belonging to several listing-eligible projects renders inside each of them, claimed or not.
5. **Fall back.** A repository claimed by no project renders as an implicit single-repository card — including when an unauthorized project also renders it as a member.
6. **Mark unresolvable members.** A member coordinate that resolves to nothing — never announced, deleted, or not present on this relay — renders inside its project as explicitly unavailable. It MUST NOT become a phantom standalone card, and it MUST NOT be silently dropped: silence makes a project look smaller than its author declared.
7. **Hiding a container never hides repositories.** Locally hiding a project makes it not listing eligible, so it claims nothing and by step 5 its members return as implicit cards. Hiding a grouping is a statement about the grouping. A repository disappears from the collection only when the viewer hides that repository or it is deleted — and a repository the viewer has hidden is hidden everywhere, including inside every project that lists it, so hiding one cannot be undone by someone else's grouping.

The fold is deterministic: same heads in, same collection out, independent of arrival order or query shape. **Placement, not order, is what the fold fixes** — the collection of containers, the members rendered inside each container, and the implicit cards are all compared as sets, since member order is not significant in the event ([Event Format](#event-format)) and a client is free to sort its own presentation. Every live, unhidden repository renders in at least one place — inside a project that claims it, or as its own card — and no repository renders twice within one container.

### Required fold cases

The fold cannot be expressed as accept/reject of a single event, so it has its own fixture file rather than living in the ingest [conformance fixtures](#conformance-fixtures). A client implementing the fold MUST cover at least these cases, each of which is a distinct branch above:

| Case | Expected collection |
|------|---------------------|
| Owner's own project lists their repository | Repository renders inside the project only |
| Stranger's project lists someone else's repository | Repository renders inside that project *and* as its own card |
| Project signer is in the member repository's `maintainers` tag | Repository renders inside the project only |
| Repository is a member of two projects that both claim it | Repository renders inside both; no implicit card |
| Repository removed from every project | Repository renders as an implicit card |
| Project is `unlisted`, or locally hidden | Project absent from the collection; its members render as implicit cards |
| Viewer has hidden a member repository | Repository absent from the collection *and* from inside every project listing it |
| Member coordinate resolves to nothing | Member renders inside its project as unavailable; no standalone card |
| Project head deleted | Project absent; its members render as implicit cards |
| One authorized and one unauthorized project both list the same repository | Repository renders inside both projects; no implicit card, because one claim suffices to suppress it |
| More repositories and projects than one page holds, with several sharing one `created_at` | Every repository and project renders |

[`NIP-MP.fold-fixtures.json`](NIP-MP.fold-fixtures.json) mechanizes this table — see [Conformance Fixtures](#conformance-fixtures).

### Pagination

Step 1's "to exhaustion" describes the target result, not a single algorithm: what a client must do — and whether it can fully reach it — depends on the cursor its relay offers. Both modes below are conformant; a client MUST implement whichever its relay supports, MUST NOT present a mode-1 loop's output as complete on a mode-2 relay, and on a relay that provides neither mode 1 nor the relay contract below, MUST mark the collection possibly incomplete — presenting that marked partial collection is conformant, not a violation of step 1's enumeration requirement.

**The relay contract both modes rest on.** Every "short response = done" inference — whether from a composite cursor or a drained bucket — is a property of the relay, not of NIP-01, where `limit` is advisory: relays "SHOULD use the `limit` value to guide how many events are returned in the initial response. Returning fewer events is acceptable" (NIP-01). A conforming relay may answer a request for 100 with 50 events and no indication that it withheld the rest, and the client cannot tell that from exhaustion. Exhaustive enumeration is possible only on a relay that satisfies **all three** of these conditions, which a client can evaluate independently:

1. The relay **applies the complete filter before enforcing any limit.** A relay that post-filters after limiting can return a short (even empty) response while older matching events sit beyond the limited window, so short responses carry no exhaustion signal on such a relay.
2. The relay **exposes the exact effective page limit it enforces.** The effective page limit is the smaller of the requested `limit` and any relay-imposed cap, since a clamped request answered in full is short without being exhausted. If the advertised cap differs from the enforced one, "shorter than the effective limit" is undecidable by the client.
3. The relay **saturates pages**: after applying the complete filter and cursor, it returns `min(effective page limit, remaining matching events)` events — equivalently, whenever at least the effective limit's worth of matches remain, the page is full, so a short page contains all remaining matches. A cap bounds from above; without saturation, a relay may return fewer than the cap even when matches are still available, and a short page proves nothing. An authoritative relay-provided continuation or end signal computed after complete filtering is an equivalent substitute for this response-length inference.

A relay satisfying any proper subset of these conditions does not provide the guarantee. Absent the guarantee, a client MUST mark the collection possibly incomplete regardless of any response sizes; the modes below serve to reduce silent loss rather than eliminate it. `limit` below means the effective page limit.

**Mode 1 — composite cursor (exhaustive under the relay contract).** On a relay that exposes a keyset cursor over `(created_at, event id)`, a client MUST page by it. As an example of the cursor mechanics, Buzz implements the keyset as `created_at < until OR (created_at = until AND id > before_id)` (`crates/buzz-db/src/event.rs:48-52`), resolving the sort to `(created_at DESC, id ASC)`. Buzz exposes this cursor on its authenticated HTTP bridge endpoint (`crates/buzz-relay/src/api/bridge.rs`); it is not available on the NIP-01 websocket REQ path, where `before_id` is silently discarded — `protocol.rs` deserializes each REQ filter into a standard `nostr::Filter`, whose deserializer drops unknown fields, so a client sending `before_id` on a REQ receives no error and falls back to `until`-only paging without knowing it. A NIP-01 websocket client reading `kind:30621` from Buzz is therefore in mode 2, not mode 1; mode selection requires evaluating the relay contract per transport. Within the relay contract, the uniqueness of the `(created_at, id)` pair means each page resumes exactly where the last ended with no skips or re-reads, and a short page is an unambiguous end signal. Cursor uniqueness adds tie-safety; it does not substitute for the relay contract — a relay that post-filters after limiting can return an empty page under this cursor while older matching events remain beyond the candidate window.

**Mode 2 — `until` only (boundary-bucket drain; exhaustive only under the relay contract).** A vanilla NIP-01 filter offers no id tiebreak, so the only cursor is `until`. Neither naive step is safe: `until = oldest_seen_created_at - 1` skips every unread event in that second, and `until = oldest_seen_created_at` re-requests the whole bucket, which never advances once one `created_at` bucket exceeds the relay's page size. A mode-2 client MUST therefore drain the boundary second explicitly before stepping past it.

1. A page returning fewer than `limit` events means the query is exhausted — stop.
2. After a **full** page, let `oldest` be the smallest `created_at` it returned. Query that second exactly — `since = until = oldest` — and merge the result into what is already held, deduplicating by event id. That single bucket query has two outcomes.
3. If it returns `limit` events, second `oldest` may hold more than the relay will return in one response, so the collection MUST be marked possibly incomplete. Count `limit` inclusively: a bucket holding exactly `limit` events is indistinguishable from a larger one, and over-reporting a doubt is the safe direction.
4. If instead it returns fewer than `limit` events, the second is fully drained. Set `until = oldest - 1` and continue from step 1.

A client that cannot drain a bucket has lost exhaustiveness for that second and MUST keep the collection marked possibly incomplete; it MAY still set `until = oldest - 1` to gather the older events rather than stall, but MUST NOT clear the mark by doing so.

The naive form fails on a page whose oldest second is only partly returned, which a same-`created_at` test on the page as a whole does not see. With `limit = 3` over `(100,a) (99,b) (99,c) (99,d) (98,e)`, the first page is `(100,a) (99,b) (99,c)` — two distinct timestamps, so no all-tied heuristic fires — and advancing to `until = 98` silently drops `(99,d)`. Draining second `99` first retrieves it.

Enumeration is therefore exhaustive when the relay satisfies the contract above and every equal-`created_at` bucket fits in one response; under those conditions truncation is detected exactly rather than guessed at. On detecting it — or on any relay that does not meet the contract — a client MUST mark the collection as possibly incomplete rather than present a partial collection as complete. Silently presenting a truncated collection is the failure this NIP exists to prevent: a repository missing from the list is indistinguishable from one that was never announced.

**Query shapes.** The relay contract applies only where the relay can apply it — and that depends on the query shape. A relay that post-filters some constraints (such as `#a` tag matching applied after the SQL `LIMIT`) cannot guarantee short-response exhaustion for queries that use those constraints. A client MUST therefore issue fold queries in shapes whose full filter the relay applies before limiting. Where a needed constraint is not applied pre-limit on the target relay, the client MUST widen the query to constraints that are — for example, enumerating all `kind:5` events by `kinds` alone, or `kinds` + `authors`, rather than adding an `#a` filter the relay post-applies — and match the remaining criteria client-side. This keeps the relay contract's short-response guarantee intact for every query the fold issues.

### Collection growth

Step 1's exhaustive enumeration is a correctness floor, not a scaling strategy: it says a client MUST NOT silently truncate its collection, because a repository absent from the list is indistinguishable from one that does not exist. It is not a mandate to hold the relay's entire repository set in memory on every load.

At Buzz's current scale (hundreds of repositories per community) exhaustive enumeration is the whole story. Past that, the way out is a narrower question — a server-side collection query, a scoped or searched subset, or resolving a project's members on demand — not a fixed client-side `limit`. Any such surface MUST report its own truncation so a client can say "showing N of M" rather than quietly presenting a partial collection as complete.

### Route resolution

A project route resolves to a container; a repository route resolves to a repository. Every repository-scoped operation — clone, fetch, issues, pull requests, activity, mutation, deletion — MUST take an explicit repository coordinate. None may infer its target from container state, or a two-repository project will silently operate on the wrong member.

Legacy `<owner>:<dtag>` repository routes remain valid and resolve to that repository, presented as a single-repository container.

## Conformance Fixtures

Two fixture files carry the machine-checkable contract. `NIP-MP.fixtures.json` is already wired as the relay ingest consumer; the remaining consumers listed below are Phase 2 work.

### Ingest

[`NIP-MP.fixtures.json`](NIP-MP.fixtures.json) holds the shared valid/invalid case set: 16 accepted and 29 rejected events covering minimal and full projects, zero members, the 64-member boundary from both sides, cross-owner and same-`d`-different-owner members, colon-bearing repository `d` values, relay hints, non-empty `content`, access levels and role-bearing invites, and each rejection rule above.

The relay validator, the Rust builder, and the TypeScript builder are required to test against this one file, so a divergence between them is a test failure rather than a production surprise.

Each case carries an **unsigned** template — `kind`, `content`, `tags`. Consumers sign it with their own test key. Signed literals would be inert: the id and signature are fixed by the exact serialization, so any consumer that re-serializes would need to recompute both anyway. Rejection cases name their `reject_rules`, so an implementation cannot pass by rejecting a bad event for an unrelated reason.

### Fold

[`NIP-MP.fold-fixtures.json`](NIP-MP.fold-fixtures.json) holds the oracle for [the fold](#the-fold): 12 cases covering every row of the [required fold cases](#required-fold-cases) table. Every client implementing the fold is required to test against this one file. The fold is where the [claim authority](#claim-authority) rule lives, so without a shared oracle two clients could each satisfy the prose and still render different collections from identical heads.

Its cases are **semantic, not signed envelopes**. A repository or project is named by its coordinate plus the inputs the fold actually reads — signer, members, `maintainers`, visibility, viewer-hidden, deletion. Signing would test the ingest contract a second time and obscure what is under test: this file assumes every input is an already-accepted head and pins only the placement derived from it. Each case gives `expect.containers` (each rendered project with the members rendered inside it) and `expect.implicit_cards` (the repositories that additionally render as their own cards). Every collection in `expect` is compared as a set — the containers, each container's `members`, and the implicit cards alike — because the fold fixes placement and not order.

## Security Considerations

**Unauthorized grouping claims are the accepted trade.** Anyone may publish a project referencing anyone's repositories. That claim is a signed statement attributable to its author and grants nothing ([Authority](#authority)) — the same trust model as NIP-51 lists, which likewise reference content their author does not own. A client MUST NOT present membership in a stranger's project as endorsement by, or authority over, the member repository's owner, and MUST show the project signer alongside a project it did not author.

**Resolution fan-out is bounded.** Each project resolves at most 64 coordinates, and the cap counts raw tags, so no single event can force unbounded resolution work regardless of how its tag list is shaped.

**Push policy is untouched.** A project cannot grant, widen, or narrow push access to any repository. Push policy reads only the repository's own `kind:30617`. This is a design invariant, not an implementation detail: if a project ever became an input to push authorization, publishing a project naming someone else's repository would become a privilege-escalation primitive.

## Relation to Other NIPs

- **NIP-34**: Supplies the member repositories. Members are `kind:30617` announcements referenced by coordinate; a NIP-34 client that does not know `kind:30621` still discovers and renders each repository normally.
- **NIP-01**: Supplies the addressable-event class, the `a` tag grammar, addressing, replacement, and the owner-only editing model. Owner-only editing is not enforcement code in Buzz — it is what NIP-01 replacement already means.
- **NIP-09**: Supplies container deletion, which deletes the container only. Buzz extends it in two ways that are not project-specific: an agent's registered NIP-OA owner may also delete, and a tombstone applies only at or before its own `created_at` ([Deletion](#deletion)).
- **NIP-29**: Supplies the channel a project's `buzz-channel` names. The reference is metadata; project state is never channel-scoped.
- **NIP-51**: The closest existing precedent — a signed, addressable list referencing content the author need not own. Not reused because a project is a shared named forge container with its own channel binding and visibility, not a user's private-or-public bookmark set.
- **NIP-OA**: Consulted for container deletion only — an agent's registered owner may delete the agent's project ([Deletion](#deletion)). Push access is unaffected: agents inherit repository push access from their owner through the repository's own protections, and a project is never consulted.
