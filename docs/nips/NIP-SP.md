NIP-SP
======

Session Preview
---------------

`draft` `optional` `relay`

**Depends on**: NIP-01, NIP-29 (`h`), NIP-SW (watch, frames, snapshots), NIP-CSL (`cs-target`).

## Abstract

A coding session's Browser preview runs in the desktop app on the machine running the agent. This NIP defines how that
desktop **announces** the preview to the session (`kind:30626`) and how a page address travels without leaking the
machine's local URL. Watching and snapshots are NIP-SW with `surface=preview` and `d` = the umbrella sessionRef.

| Kind | Name | Class | Signer |
|---|---|---|---|
| 30626 | preview announce | addressable, `d` = sessionRef | desktop identity of the hosting machine |
| 44253 | surface snapshot, `surface=preview` | regular stored (NIP-SW) | whoever caused the snapshot |
| 24320 | surface watch, `surface=preview` | ephemeral (NIP-SW) | any channel member |
| 24321 | surface frame, `surface=preview` | ephemeral (NIP-SW) | the preview's owner (below) |

Implementation: `crates/beekeeper-core/src/session_preview.rs`; builder
`beekeeper_sdk::surface::build_session_preview_announce`.

## Announce — `kind:30626`

```json
{
  "kind": 30626,
  "content": "",
  "tags": [
    ["h", "<session channel uuid>"],
    ["d", "<sessionRef>"],
    ["spa-v", "1"],
    ["status", "open"],
    ["cs-target", "coding-session/v1|…"],
    ["provider", "<machine's provider/host pubkey>"],
    ["page", "local:/settings"],
    ["title", "Settings"],
    ["viewport", "1280x800"],
    ["stream", "frames"],
    ["input", "synthetic"]
  ]
}
```

Tag order is exact (NIP-SW). `status` is `open` or `closed`; any other value is refused. `cs-target` names the
execution that opened the preview and is absent when a person did. `provider` is optional (absent → "machine not
recorded"). `page` and `title` are required on `open` and **forbidden on `closed`**: a close says only that the
preview closed, never what it last showed. `title` is one line of at most 200 bytes and may be empty. `stream` is `frames` (live NIP-SW frames while
watched) or `snapshots` (dated snapshots only). `input` is always `synthetic`: the driver's events have
`isTrusted=false`, and every result says so. Content is empty.

Republished on open, close, share toggle and origin change, debounced to at least 5 s. Turning sharing off publishes
nothing new: the hosting desktop sends exactly one `closed` announce if it had announced the preview open, and no
frames or snapshots while sharing stays off.

**Owner fold** (`session_preview::resolve_preview_owner`): take each signer's newest valid announce for the
`(h, d)`; among those with `status=open`, the **earliest** `created_at` wins (tie: the lower event id). A desktop that
sees an earlier open announce from another signer refuses to open its own and names the owner. The relay uses the same
fold to decide whose `surface=preview` frames to accept (NIP-SW § Announcer authority).

## Page redaction

The full URL never leaves the machine. `page` is either:

- `local:/<path>` — a loopback, private-network or `file:` page: the path only, with no host, port, query or fragment
  (a `file:` page publishes only its last path segment, or a path relative to the session worktree); or
- `http(s)://<public host>[/<path>]` — no port, credentials, query or fragment. A loopback or private host
  (`localhost`, `*.local`, `127/8`, `10/8`, `172.16/12`, `192.168/16`, link-local, `::1`, `fc00::/7`) is refused here
  and must use `local:`.

`page` also passes NIP-SW's host-local rule, so `local:/Users/…` is refused. Producers call
`session_preview::redact_page_url(full_url)`, which returns the publishable value or nothing.

## Relay

`MessagesWrite`, `h` required, the strict coding-session membership gate, exact structure and the host-local rule.
Stored as an addressable event keyed by `(pubkey, 30626, d)`.

## Kind number

30626 comes from the session-view parity registry; `git grep -n -w 30626` on 2026-10-07 matched nothing.
