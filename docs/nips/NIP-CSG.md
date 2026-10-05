# NIP-CSG — Coding-session genesis, goal, name, and closure revisions

This document defines the human-authored umbrella-session facts. Genesis
(`kind:44226`) establishes the founder; goal revisions (`kind:44227`) describe
what that umbrella session is doing; name revisions (`kind:44229`) provide its
short navigation label; closure revisions (`kind:44230`) record whether the
umbrella is organizationally closed or open. All are public, durable, and
scoped to a NIP-29 channel by an `h` tag. Providers do not author or interpret
goal, name, or closure events. One provider-authored record lives here too,
because it is read beside the name: the generated title (`kind:44252`, below),
which a provider signs as itself and which never becomes a name revision.

## Goal revision (`kind:44227`)

A goal revision is a regular append-only event. It deliberately uses a `d` tag
for session lookup without using a parameterized-replaceable kind: every prior
revision remains queryable.

The content is raw prose, not JSON. Its UTF-8 encoding is at most 4096 bytes and
must contain at least one non-whitespace character. The event has exactly these
three ordered, two-field tags:

```json
[
  ["h", "<channel UUID>"],
  ["d", "<lowercase canonical sessionRef UUID>"],
  ["csgl-v", "csgl1-1"]
]
```

The author signs each revision. During the founder-only interim authority
phase, clients preflight goal edits against the founder resolved from the
session's explicitly referenced genesis event. The relay still applies strict
channel membership and exact envelope validation; it does not infer session
authority from relay history.

Consumers group valid events by `(h, d)` and choose the greatest tuple
`(created_at, event id)` as the current goal. Nostr timestamps have one-second
precision, so the lexicographically greatest lowercase event id is the
deterministic tie-breaker. Revision-history UI is deferred, but history
retention is part of this wire contract.

The UI must not describe the goal as "permanent" until the separately reserved
product wording decision is made.

## Name revision (`kind:44229`)

A name revision is a regular append-only event. Like a goal, it uses a `d` tag
for session lookup without opting into parameterized replacement, preserving
every earlier name.

The content is a short, single-line navigation label. Its UTF-8 encoding is at
most 256 bytes and must contain at least one non-whitespace character. The event
has exactly these three ordered, two-field tags:

```json
[
  ["h", "<channel UUID>"],
  ["d", "<lowercase canonical sessionRef UUID>"],
  ["csnm-v", "csnm1-1"]
]
```

During the founder-only interim authority phase, clients preflight name edits
against the founder resolved from the session's explicitly referenced genesis
event. The relay enforces channel membership and the exact envelope but does
not project founder authority from history.

Consumers group valid events by `(h, d)` and choose the greatest tuple
`(created_at, event id)` as the current name. A missing name falls back to the
founding execution title for compatibility; provider-authored 44223 titles
remain per-generation facts and never become authoritative session names.

## Generated title (`kind:44252`)

A generated title is a short title a model wrote for an umbrella session nobody
has named, from the founder's first message. It is **not** a name revision and
is never published as one: `kind:44229` is human-authored by definition, and a
model's words signed as the founder would put them on the wire as the person's
name. The provider instance that ran the founder's first turn signs the title
with its own key — the same key that signs that execution's 44223, 44224 and
44225.

It is a regular append-only event, scoped by `h` and grouped by `d` without
parameterized replacement. The event has exactly these four ordered,
two-field tags:

```json
[
  ["h", "<channel UUID>"],
  ["d", "<lowercase canonical sessionRef UUID>"],
  ["cstl-v", "cstl1-1"],
  ["cs-target", "<the signing execution's coding-session/v1 target key>"]
]
```

`cs-target` is the structured key NIP-CSC defines
(`coding_session_target_key`): driver, instance id, session id and generation,
re-encoding exactly.

Content is strict JSON, at most 2048 bytes, with exactly these keys:

```json
{
  "schema": "buzz-coding-session-title/v1",
  "title": "Login redirect fix",
  "model": "claude-haiku-4-5",
  "basis": "first-message",
  "sourceCommand": null,
  "createEventId": "<64-character lowercase 44221 event id>"
}
```

- `title` obeys the 44229 content rule: one line, at most 256 UTF-8 bytes, at
  least one non-whitespace character.
- `model` is the model id the provider used: non-empty, at most 128 bytes, no
  control characters.
- `basis` is exactly `first-message` in v1.
- `sourceCommand` is the 44220 turn command whose text was summarised, or
  `null` for a create's initial turn. The key is required; only the value may
  be null.
- `createEventId` is the 44221 create of the execution that ran the turn.
- Any other key is refused, so no workdir, path or prompt text travels.

**The relay validates structure only** — the tags, the target key and the
content above — under the strict coding-session membership gate every session
kind uses. It checks no signer standing, the division 44229 and 44245 draw.

**Readers judge standing, in one resolver.** Every reader resolves an
umbrella's display name with the same three ranked tiers
(`buzz_core::coding_session_title::resolve_session_display_name`, mirrored in
TypeScript and Dart and pinned by `conformance/session-display-name/`):

1. **person** — the latest valid founder-signed 44229, as above;
2. **generated** — only when tier 1 is empty: the **earliest** valid 44252 for
   `(h, d)` whose signer equals the provider authority of the execution its
   `cs-target` names, where that execution is in this umbrella (the reader
   lists every generation it holds). Earliest, by `(created_at, event id)`
   ascending, so a title never flips once shown. This tier also yields the
   `model` and the signer, which a reader shows as the title's attribution;
3. **fallback** — the founding execution's title, then "Untitled session".

Tiers are never compared by time: a person's name always wins, even an older
one, so a rename beats a title by construction rather than by winning a race.
A title from a signer without standing — another execution's provider, or a
key with no execution in the umbrella — is ignored and counted in the
resolver's diagnostics. That is an isolation rule, not a security rule.

A generated title is deletable with its session: a whole-session `kind:5`
reaches it by its `d` tag, as it reaches 44229.

## Closure revision (`kind:44230`)

A closure revision is a regular append-only event about the shared umbrella,
not a command to a provider. `closed` moves the umbrella to its settled shelf;
`open` makes it available for continuation. Neither action starts, stops, or
resurrects an execution, and reopening by itself consumes no execution slot.

Content is strict public JSON with exactly these fields:

```json
{
  "action": "closed",
  "genesisRef": "<64-character lowercase genesis event id>",
  "sessionRef": "<lowercase canonical session UUID>",
  "v": 1
}
```

`action` is exactly `closed` or `open`. The event has exactly these four
ordered, two-field tags:

```json
[
  ["h", "<channel UUID>"],
  ["d", "<sessionRef>"],
  ["cscl-v", "cscl1-1"],
  ["cscl-genesis", "<genesisRef>"]
]
```

The relay resolves `genesisRef` by event id and verifies that it names a valid
44226 event in the same channel whose `sessionRef` agrees with the closure.
The genesis signer is the session owner; authority is action-specific:

- `closed` is accepted only from that owner.
- `open` in a project session-transport channel is accepted from any member of
  the project's current ACL (the project owner or a currently invited member).
- `open` in a standalone channel is accepted only from the session owner.

Project authority comes from the transport channel's current project gate, not
from a stale channel-membership row. A non-project channel never broadens reopen
authority merely because another user can write ordinary channel messages.

Consumers group valid revisions by `(h, d, cscl-genesis)` and choose the
greatest tuple `(created_at, event id)`. With no closure revision, the legacy
default is `open`. Closure events cannot be deleted through NIP-09 or NIP-29
moderator deletion: changing the fold requires another signed revision, so an
older state can never silently reappear when a newer fact is erased.
