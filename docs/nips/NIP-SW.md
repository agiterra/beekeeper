NIP-SW
======

Surface Watch, Frames and Snapshots
-----------------------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (event format, ephemeral events), NIP-29 (`h` channel scoping), NIP-CSL (`cs-target` keys, the
generation resolver). **Used by**: NIP-SP (session preview), NIP-SDV (session device).

## Abstract

A coding session has visual **surfaces** a teammate may want to watch: the Browser preview the agent drives (NIP-SP)
and the device it runs an app on (NIP-SDV). This NIP defines one shared contract for both, so there is one watch kind,
one frame kind, one snapshot record, one relay handler and one viewer:

- `kind:24320` **surface watch** (ephemeral, channel member → producer): "I am watching", keepalive, stop, resync, or
  "take a snapshot".
- `kind:24321` **surface frame** (ephemeral, producer → session channel): the picture, a base64 JPEG.
- `kind:44253` **surface snapshot** (regular, stored): a dated, durable picture on relay media, with who took it, on
  which machine, at whose request and at which commit.

A `surface` tag says which surface an event is about: `preview` or `device`. Any other value is refused. `d` names
the surface instance: the umbrella **sessionRef** for a preview, the opaque 16-hex **`sdv-slot`** for a device
(NIP-SDV).

All three kinds are scoped to the session's channel with `h`. Tag order is exact: tags appear in the order listed,
optional tags are skipped (never reordered), and no other tag may appear. Integers are canonical decimal (no sign, no
leading zeros), at most 2^53−1. `WxH` is two integers 1..=8192 joined by a lowercase `x`.

Implementation: `crates/beekeeper-core/src/surface_watch.rs`, `surface_snapshot.rs`; relay
`crates/beekeeper-relay/src/handlers/surface_watch.rs`; builders `beekeeper_sdk::surface`.

## Never on the relay

Host-local facts never travel: a simulator UDID, a daemon URL, port or token, slot-file and screenshot paths, a
localhost host:port and query, hostnames, DerivedData and bundle paths. They stay in the producer's own files.

The relay, every builder and every core validator **refuse** an event of these kinds (and of NIP-SP's and NIP-SDV's
kinds) when any tag value contains a UUID-shaped run (8-4-4-4-12 hex, either case) or one of `/Users/`, `/home/`,
`/var/folders/`, `/private/var/`, `DerivedData`, `CoreSimulator`, `file://`. Exempt from the UUID half only: `h` (a
channel UUID), a sessionRef `d`, and minted identifiers that may embed a runtime's UUID (`cs-target`, `csl-command`,
`sdv-cmd`). Producers redact first (`surface_watch::redact_free_text`); the refusal is a backstop.

## Watch — `kind:24320`

```json
{
  "kind": 24320,
  "content": "{\"action\":\"watch\"}",
  "tags": [
    ["h", "<session channel uuid>"],
    ["surface", "device"],
    ["d", "<sdv-slot or sessionRef>"],
    ["p", "<producer pubkey>"]
  ]
}
```

Content is exactly `{"action": "watch" | "stop" | "resync" | "snapshot"}`, at most 1 KiB. `p` names the producer and
is never the author (Nostr clients drop a self `p` tag at signing).

- Watchers send `watch` on mount and every **15 s** while the surface is on screen, and `stop` when it leaves.
- The producer expires a watcher after **45 s** without a keepalive and **stops capturing when no watcher is live**.
- `resync` asks for a full frame now. `snapshot` asks the producer to publish one 44253 naming the watcher in
  `requested-by`; a producer honours one request per watcher per 10 s and answers a refusal by publishing nothing (the
  watcher's UI says "the host did not answer" after 15 s).

**Relay.** Signature, `created_at` within ±5 minutes, strict structure, the token's channel restriction, channel
membership (the ordinary `h` write path), at most 10 per second per key. **Delivered only to connections whose pubkey
is the `p` or the author**, under the channel ACL. The relay keeps no watcher state.

## Frame — `kind:24321`

```json
{
  "kind": 24321,
  "content": "/9j/4AAQSkZJRgABAQ…",
  "tags": [
    ["h", "<session channel uuid>"],
    ["surface", "preview"],
    ["d", "<sessionRef>"],
    ["t", "frame"],
    ["seq", "41"],
    ["epoch", "1791374400123"],
    ["cadence-ms", "2000"],
    ["dim", "1280x800"],
    ["captured-at", "1791374512345"],
    ["actor", "<pubkey>"],
    ["commit", "<40 hex>"]
  ]
}
```

- `t`: `frame` carries standard padded base64 of a JPEG (it begins `/9j/`), at most **200 KiB** of text; `paused`
  (sharing off or surface hidden) and `end` (surface closed) carry empty content.
- `seq` strictly increases within an `epoch`; a new `epoch` (producer restart, surface reopened) resets it.
- `cadence-ms` (500..=60000) is the pacing in force, so the UI can say "Live · every 3 s".
- `captured-at` is Unix milliseconds. `actor` (optional) is whoever sent input in the last five seconds. `commit`
  (optional) is the working-tree head when the producer knows it.

Producer budgets: preview ≤ 96 KiB, long edge ≤ 1280, 2 s base cadence; device ≤ 200 KiB, long edge ≤ 900, 3 s base;
both **change-only** (hash compare) and **≤ 20 per minute**, backing off on `rate-limited:`. Frames go out only while
a watcher is live.

**Announcer authority.** The relay accepts a frame only when its author is the announced producer of
`(h, surface, d)`:

- `preview`: the owner per NIP-SP's fold over the channel's `kind:30626` announces with that `d`;
- `device`: the signer of the newest `kind:44255` with `sdv-type=state` and `sdv-slot=d` in the channel, provided that
  record's (`csl-command`, `cs-target`) resolves to that signer through the coding-session generation resolver.

Positive answers are cached for at most 10 s. No authority, a lookup failure, or a different author refuses the frame
with `restricted: surface frame author is not the announced producer`. Clients also subscribe with
`authors=[authority]`, so the rule holds even against a relay that forgets it. Frames are rate-limited to 2 per second
per key and delivered to the channel under its ordinary ACL.

## Snapshot — `kind:44253`

```json
{
  "kind": 44253,
  "content": "Settings page after save",
  "tags": [
    ["h", "<session channel uuid>"],
    ["ssn-v", "1"],
    ["ssn-type", "snapshot"],
    ["surface", "device"],
    ["d", "<sdv-slot>"],
    ["x", "<sha256 of the blob>"],
    ["url", "https://<relay>/<sha256>.png"],
    ["m", "image/png"],
    ["dim", "1179x2556"],
    ["taken-at", "1791374512345"],
    ["provider", "<machine's provider/host pubkey>"],
    ["p", "<requester pubkey>", "", "requested-by"],
    ["commit", "<40 hex>", "dirty"],
    ["e", "<44254 command id>", "", "command"]
  ]
}
```

- Signer: the capturing producer (device: the provider; preview: the hosting desktop, or a seat via `bee`).
- `ssn-type`: `snapshot`, or `annotation` (then `e` with marker `annotation` names the 44220 steer it belongs to).
- `url` is an http(s) relay-media URL that names the blob by `x`, with no query. `m` is `image/png`, `image/jpeg` or
  `image/webp`; the blob is re-encoded without metadata before upload. `taken-at` is Unix milliseconds.
- `provider` is the machine as a key, never a hostname.
- Optional, in this order: `p` (4 fields, marker `requested-by`), `commit` (3 fields, `clean` | `dirty`), `e` (4 fields,
  marker `command` | `annotation`), then `page` (required iff `surface=preview`, NIP-SP grammar) and `title` (preview
  only, ≤ 200 bytes).
- Content is alt text, at most 1 KiB, and passes the host-local rule.

Readers show "commit not recorded" when `commit` is absent. Verdicts cite a snapshot as `snapshot:<44253 id>`
(`preview:<id>` is accepted as an alias); the UI renders the token as a thumbnail.

**Relay.** `MessagesWrite`, `h` required, the strict coding-session membership gate (active member or transport
writer, no open-channel fallback), structure and the host-local rule. Whether the signer was the surface's producer
is the reader's check.

## Kind numbers

24320, 24321 and 44253 come from the session-view parity registry (`SESSION_VIEW_PARITY_PLAN.md` § "Kind
allocation"). `git grep -n -w` on 2026-10-07 matched none of them outside the registry note. 24322 (device touch) is
reserved for a later slice and not allocated.
