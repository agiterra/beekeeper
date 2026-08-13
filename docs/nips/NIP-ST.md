NIP-ST
======

Shared Terminals
----------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format, addressable and ephemeral events), NIP-MP (`kind:30621` projects and the Buzz access extension's project ACL). Interacts with NIP-42 (relay auth).

## Abstract

This NIP lets the members of a project **see — but never interact with — the terminal sessions its members have open**. It defines three kinds:

- `kind:30623` **session announce** (addressable, `d` = session id): an open built-in-shell session, listed in its project's Terminals view.
- `kind:24310` **watch** (ephemeral, observer → owner): "I am watching session X" — open, keepalive, stop, or resync.
- `kind:24311` **frame** (ephemeral, owner → observers): the terminal picture itself, as base64 raw terminal bytes an observer-side terminal writes verbatim.

Terminal output leaves the owner's machine **only while at least one member is actually watching**, and only for sessions the owner has assigned to a real project and left shared (sharing is per-session and revocable). Observing is strictly read-only: no input kind exists, and the owner's client processes nothing from observers except the watch actions above.

## Session announce — `kind:30623`

Published, replaced, and closed by the session owner. Addressed by `(pubkey, 30623, d)`, so the lifecycle is republish-latest.

```json
{
  "kind": 30623,
  "tags": [
    ["d", "<session id>"],
    ["a", "30621:<owner-hex>:<project-dtag>"],
    ["title", "build shell"],
    ["status", "open"],
    ["dims", "34x120"]
  ],
  "content": ""
}
```

- `d` (required): the session id — ≤64 chars of `[A-Za-z0-9-]`.
- `a` (required): the project coordinate the session is shared under. A session not assigned to a real project is never announced.
- `status` (required): `open` or `closed`. Relays MUST reject unknown values rather than defaulting — a typo must not publish a session.
- `title` (optional, ≤200 chars). The announce deliberately carries **no cwd and no shell path**.
- `dims` (optional): the owner's grid as `<rows>x<cols>`.

The owner republishes on rename, resize (debounced), resume, close, share-toggle, and project reassignment, and reconciles at startup (publishing `status:closed` for sessions that no longer exist) so a crashed client's announces converge. Clients MUST NOT treat `status:open` alone as "live": a session is live when frames arrive; a watch that produces no frames within ~10 s should render as "owner not streaming".

## Watch — `kind:24310` (ephemeral)

```json
{
  "kind": 24310,
  "tags": [
    ["p", "<session owner hex>"],
    ["d", "<session id>"],
    ["a", "30621:<owner-hex>:<project-dtag>"]
  ],
  "content": "{\"action\":\"watch\"}"
}
```

`action` is `watch` (open/keepalive), `stop`, or `resync` (the observer detected a sequence gap and needs a fresh snapshot). Observers send `watch` immediately and every 15 s while their observer view is mounted; owners expire a watcher 45 s after its last event and stop producing frames when no watchers remain. Content is capped at 1 KiB.

## Frame — `kind:24311` (ephemeral)

```json
{
  "kind": 24311,
  "tags": [
    ["d", "<session id>"],
    ["a", "30621:<owner-hex>:<project-dtag>"],
    ["t", "snap"],
    ["seq", "42"],
    ["epoch", "<uuid per broadcast process>"],
    ["dims", "34x120"]
  ],
  "content": "<base64 raw terminal bytes>"
}
```

- `t` (required): `tail` | `snap` | `diff` | `resize` | `end`. Relays MUST reject unknown types.
- `seq`: per-session monotonic counter; `epoch`: fresh per broadcast process, so observers can detect restarts.
- Content is capped at 96 KiB (base64).

**Attach**: when a watcher joins (or asks to `resync`), the owner emits up to 64 KiB of recent raw scrollback as `tail` frames (chunked, `chunk` tag `"<i>/<n>"`), then one `snap` — a full redraw of the live screen. **Steady state**: `diff` frames carry only what changed, coalesced to at most ~10 per second. On any `seq` discontinuity or `epoch` change the observer ignores `diff`s and requests `resync`; `resize` updates the observer's grid (observers follow the owner's dimensions); `end` closes the stream (session closed, exited, or unshared).

Frames are written verbatim into a read-only terminal emulator on the observer side — the wire format is "bytes a terminal renders", not a bespoke grid encoding.

## Relay behavior

All three kinds carry the project coordinate in their single `a` tag, and the relay enforces the NIP-MP project ACL against it at every surface:

- **Ingest (write gate)**: publishing any of the three kinds scoped to a *private* project requires the author to be admitted (project owner or invited member). Gate-lookup failures reject (fail closed). 24310/24311 additionally get: signature verification, a ±5-minute freshness window, strict singleton-tag validation, and per-pubkey rate limits (60 frames/s, 10 watches/s per community).
- **Fan-out (delivery gate)**: at the shared fan-out chokepoint, events of these kinds scoped to a private project are delivered only to the author's and admitted members' connections; unknown-pubkey connections and gate-lookup failures deliver nothing.
- **Reads (30623 only; 24310/24311 are never stored)**: a stored announce whose coordinate resolves to a private project the reader is not admitted to is withheld from REQ/COUNT/query results, exactly like the project's repositories.

Public-project sessions are visible to any workspace member, matching the visibility of every other public-project content surface.

The relay stays **stateless about watchers**: counting them is the owner's concern (the owner must receive 24310 events to stream at all).

## Client behavior

- Owners stream only sessions that are (a) assigned to a real project and (b) marked shared — sharing defaults on for project-assigned sessions and is revocable per session; flipping it off publishes `status:closed` and an `end` frame.
- Owners SHOULD surface who is watching (derived from live 24310 traffic).
- Observers subscribe to frames with `{"kinds":[24311],"authors":["<owner>"],"#d":["<session id>"]}` — the `authors` constraint makes frame spoofing by other members ineffective by construction.
- Observer terminals MUST NOT wire any input path (no keystroke handling, no paste); the sole thing an observer publishes is `kind:24310`.

## Security considerations

A terminal can display secrets. The mitigations are: sharing is per-session and revocable; frames flow only while someone watches; the who-is-watching indicator makes observation visible; announces carry no cwd/shell path; and private projects gate all three kinds server-side. Sensitive work belongs in a private project or an unshared session.
