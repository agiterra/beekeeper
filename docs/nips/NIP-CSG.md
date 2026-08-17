# NIP-CSG — Coding-session genesis, goal, and name revisions

This document defines the human-authored umbrella-session facts. Genesis
(`kind:44226`) establishes the founder; goal revisions (`kind:44227`) describe
what that umbrella session is doing; name revisions (`kind:44229`) provide its
short navigation label. All are public, durable, and scoped to a NIP-29 channel
by an `h` tag. Providers do not author or interpret goal or name events.

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
