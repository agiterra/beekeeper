NIP-ST
======

Shared Terminals
----------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format, addressable and ephemeral events), NIP-MP (`kind:30621` projects and the Buzz access extension's project ACL). Interacts with NIP-42 (relay auth).

## Abstract

This NIP lets the members of a project **see the terminal sessions its members have open**, and lets a session owner grant chosen pubkeys the right to **type into** one. It defines four kinds:

- `kind:30623` **session announce** (addressable, `d` = session id): an open built-in-shell session, listed in its project's Terminals view, carrying the session's per-invitee roster.
- `kind:24310` **watch** (ephemeral, observer → owner): "I am watching session X" — open, keepalive, stop, or resync.
- `kind:24311` **frame** (ephemeral, owner → observers): the terminal picture itself, as base64 raw terminal bytes an observer-side terminal writes verbatim.
- `kind:24312` **input** (ephemeral, roster collaborator → owner): raw input bytes for the owner's PTY — see [Input](#input--kind24312-ephemeral).

Terminal output leaves the owner's machine **only while at least one member is actually watching**, and only for sessions the owner has assigned to a real project and left shared (sharing is per-session and revocable). Observing is read-only by default: the owner's client processes nothing from observers except the watch actions above. Typing requires an explicit per-session grant — a `collaborator` entry on the announce's roster, written only by the owner.

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
- `p` (0 to 64): one **roster** entry each — see below.

### Roster tags

The announce carries the session's per-invitee roster as `p` tags:

```json
["p", "<invitee pubkey hex>", "", "<role>"]
```

- Arity is **exactly 4** — a role-less roster entry is rejected, not defaulted to a tier. Element 3 is an unparsed relay hint slot, normally empty.
- The role vocabulary is pinned: `collaborator` (may watch **and** type via `kind:24312`) or `viewer` (watch-only). Unknown roles are rejected.
- The pubkey is lowercase 64-hex; duplicates are rejected; the **owner is never listed** — they sign the announce, and the signature is their standing.
- The roster is capped at 64 entries.

Roster membership is independent of project membership: an owner may invite someone entirely outside the project (the roster admits them to watching and — for collaborators — typing), or share with a project without any roster at all. A stored announce is **visible to its roster members even outside a private project**: a per-session invite grants the announce itself, or the invitee could hold a grant they cannot discover.

The owner republishes on rename, resize (debounced), resume, close, share-toggle, roster change, and project reassignment, and reconciles at startup (publishing `status:closed` for sessions that no longer exist) so a crashed client's announces converge. Clients MUST NOT treat `status:open` alone as "live": a session is live when frames arrive; a watch that produces no frames within ~10 s should render as "owner not streaming".

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

## Input — `kind:24312` (ephemeral)

```json
{
  "kind": 24312,
  "tags": [
    ["p", "<session owner hex>"],
    ["d", "<session id>"],
    ["a", "30621:<owner-hex>:<project-dtag>"]
  ],
  "content": "<base64 raw input bytes>"
}
```

- `p` (required, singleton): the session owner — the routing target and the only party the event is delivered to.
- `d` (required, singleton): the session id; `a` (required, singleton): the project coordinate, which MUST match the announce's own (the relay refuses input re-routed under a coordinate the announce never claimed).
- Content is base64 raw input bytes, capped at **8 KiB** of base64 (≈6 KiB raw); senders chunk larger pastes client-side under that budget. Rate limit: **20 events per second** per (community, sender).

The relay accepts an input event **only from the session owner or a roster `collaborator`** on the announce head — never mere project members, never viewers — and only while the announce's `status` is `open`. Every lookup failure refuses (fail closed): input for a session the relay has no announce for grants nothing. Accepted input is delivered **only to the owner's connections**; no observer ever sees another party's keystrokes.

The relay gate is **delivery control, not the trust boundary**. The owner's host independently re-verifies, from the input event's own signature, that the sender is a roster collaborator on the current announce before any byte reaches the PTY — dual enforcement, so neither a misbehaving relay nor a stale relay-side roster can put bytes into a shell the owner's own record does not authorize.

## Relay behavior

All four kinds carry the project coordinate in their single `a` tag, and the relay enforces the NIP-MP project ACL against it at every surface:

- **Ingest (write gate)**: publishing 30623/24310/24311 scoped to a *private* project requires the author to be admitted (project owner or invited member) — except that a **watch** from a roster member (any role) is admitted even without project membership, since a per-session invite grants watching. Gate-lookup failures reject (fail closed). The ephemeral kinds additionally get: signature verification, a ±5-minute freshness window, strict singleton-tag validation, and per-pubkey one-second ceilings (60 frames/s, 10 watches/s, 20 inputs/s per community). Those ceilings bound bursts only: every one of these events is also charged to the publisher's shared per-key message quota (NIP-11 `limitation.rate_limits.messages_per_key_per_min`, 60/min by default, shared with chat and every other EVENT the key sends), so the **sustained** frame rate a broadcaster can reach is about one frame per second, and the broadcaster must throttle to fit — a frame refused as `rate-limited: message quota exceeded; retry in {n}s` was over that quota, not the per-kind ceiling. **Input (24312) takes the roster gate instead of the project gate**: owner-or-collaborator only, per [Input](#input--kind24312-ephemeral).
- **Fan-out (delivery gate)**: at the shared fan-out chokepoint, events of these kinds scoped to a private project are delivered only to the author's and admitted members' connections; unknown-pubkey connections and gate-lookup failures deliver nothing. 24312 is narrower still: delivered only to the target owner's own connections, regardless of project access.
- **Reads (30623 only; the ephemeral kinds are never stored)**: a stored announce whose coordinate resolves to a private project the reader is not admitted to is withheld from REQ/COUNT/query results, exactly like the project's repositories — unless the reader is on the announce's roster (any role), which grants the announce itself even outside the project.

Public-project sessions are visible to any workspace member, matching the visibility of every other public-project content surface. Watching therefore admits project members **and** roster members of any role; typing admits only the owner and roster collaborators.

The relay stays **stateless about watchers**: counting them is the owner's concern (the owner must receive 24310 events to stream at all).

## Client behavior

- Owners stream only sessions that are (a) assigned to a real project and (b) marked shared — sharing defaults on for project-assigned sessions and is revocable per session; flipping it off publishes `status:closed` and an `end` frame.
- Owners SHOULD surface who is watching (derived from live 24310 traffic).
- Observers subscribe to frames with `{"kinds":[24311],"authors":["<owner>"],"#d":["<session id>"]}` — the `authors` constraint makes frame spoofing by other members ineffective by construction.
- Observer terminals MUST NOT wire an input path unless the viewer holds a `collaborator` roster entry on the current announce; a plain observer's sole published kind is `kind:24310`. A collaborator's client sends keystrokes as `kind:24312` and treats a relay refusal as revocation.
- Owner hosts MUST re-verify every input event's signature and roster standing locally before writing its bytes to the PTY ([Input](#input--kind24312-ephemeral)); the relay gate alone is never sufficient.

## Security considerations

A terminal can display secrets, and with `kind:24312` an invited collaborator can also **run commands as the owner** — a roster grant is a deliberate, per-session delegation of the owner's shell, not a convenience toggle. The mitigations are: sharing is per-session and revocable; frames flow only while someone watches; the who-is-watching indicator makes observation visible; announces carry no cwd/shell path; private projects gate the observe kinds server-side; and input is dual-enforced — the relay's owner-or-collaborator gate is delivery control, while the owner host independently re-verifies the sender's signature against its own announce roster before any byte reaches the PTY, so neither a misbehaving relay nor a stale relay-side projection can inject input the owner's record does not authorize. Revoking a collaborator republishes the announce; both gates observe the new head. Sensitive work belongs in a private project or an unshared session; write access belongs only with pubkeys trusted to hold the keyboard.
