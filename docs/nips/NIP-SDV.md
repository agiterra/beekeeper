NIP-SDV
=======

Session Device
--------------

`draft` `optional` `relay`

**Depends on**: NIP-01, NIP-29 (`h`), NIP-CSL (`cs-target`, lifecycle command ids, the generation resolver), NIP-SW
(watch, frames, snapshots with `surface=device`).

(NIP-DV is DM visibility; this one is SDV.)

## Abstract

A coding session may own one device (an iOS Simulator today). The **provider** — inside `beekeeper-host`, outside the
seat boundary — boots it, captures it and drives it. Seats and people ask with a signed stored **command**
(`kind:44254`); the provider answers with a signed stored **record** (`kind:44255`). Watching is NIP-SW with
`surface=device` and `d` = the slot. Screenshots are NIP-SW `kind:44253` snapshots.

Host-local facts — the UDID, the daemon URL, port and token, file paths — stay in a 0600 slot file on the machine. The
wire names a device by its **slot**: the first 16 lowercase hex of `sha256(provider pubkey (32 raw bytes) ‖ UDID as
simctl prints it)` (`session_device::device_slot_id`).

Implementation: `crates/beekeeper-core/src/session_device.rs`, `session_device_record.rs`; builders
`beekeeper_sdk::surface::{build_session_device_command, build_session_device_record}`.

## Command — `kind:44254`

```json
{
  "kind": 44254,
  "content": "{\"op\":\"open\",\"platform\":\"ios\",\"model\":\"iPhone 17\"}",
  "tags": [
    ["h", "<session channel uuid>"],
    ["sdv-v", "sdv1"],
    ["cs-target", "coding-session/v1|…"],
    ["sdv-cmd", "<caller-chosen stable id>"],
    ["sdv-slot", "<slot>"]
  ]
}
```

- `cs-target`: the generation the device is (to be) bound to.
- `sdv-cmd`: 1..=64 of `[A-Za-z0-9._-]`, chosen by the caller and stable across retries — never derived from the
  clock.
- `sdv-slot`: required for `close`, `screenshot` and `action`; refused on `open`.
- Content (strict JSON, unknown keys refused, ≤ 8 KiB): `op` is `open` | `close` | `screenshot` | `action`;
  `platform` (`ios` | `android`) and `model` (≤ 64 bytes) only on `open`; `shutdown` (bool) only on `close`; `action`
  `{type, args}` required iff `op=action` (`type` 1..=32 of `[a-z0-9_-]`, `args` a JSON object the provider validates).

Author: a seat of the targeted generation or a person in the session. **Standing is the provider's decision**; the
relay checks structure only.

## Record — `kind:44255`

```json
{
  "kind": 44255,
  "content": "{\"type\":\"state\",\"state\":\"open\",\"platform\":\"ios\",\"model\":\"iPhone 17\",\"osVersion\":\"27.0\",\"drivers\":[\"agent\",\"host-owner\"],\"capture\":{\"mode\":\"snapshot-poll\",\"maxIntervalMs\":3000}}",
  "tags": [
    ["h", "<session channel uuid>"],
    ["sdv-v", "sdv1"],
    ["cs-target", "coding-session/v1|…"],
    ["csl-command", "<lifecycle command id that minted the generation>"],
    ["sdv-type", "state"],
    ["sdv-slot", "<slot>"],
    ["sdv-cmd", "<the command this answers>"]
  ]
}
```

- `csl-command` is the lifecycle command id that minted the `cs-target` generation (the same value as the provider's
  `kind:24223` lease). It lets the relay resolve the record's signer as the generation's provider authority.
- `sdv-type` equals the content's `type`:
  - `availability` — `{platforms: {ios: {available, reason?}, android: {available, reason?}}, agentDevice:
    {installed, version?, reason?}}`. Once per generation. No `sdv-slot`, `sdv-cmd` or `e`. **No availability record
    is shown as "this machine's provider does not offer devices", never as "no devices".**
  - `state` — `{state: booting|open|closed|failed, platform, model, osVersion, drivers ⊆ [agent, host-owner],
    capture: {mode: "snapshot-poll", maxIntervalMs}, reason?}`. Requires `sdv-slot`; `sdv-cmd` optional (the command
    that caused it).
  - `refused` — `{code, reason}`, `code` ∈ `no_standing` | `no_device_open` | `platform_unavailable` |
    `toolchain_unavailable` | `capture_failed` | `invalid_command` | `busy`. Requires `sdv-cmd`.
  - `shot` — `{}` plus `["e", <44253 id>, "", "snapshot"]`. Requires `sdv-slot` and `sdv-cmd`.
- Content is strict JSON, ≤ 4 KiB; every string is one line of ≤ 512 bytes and passes NIP-SW's host-local rule.

**Every command gets exactly one terminal record** carrying its `sdv-cmd`: a `state` of `open`, `closed` or `failed`,
a `shot`, or a `refused`. `booting` is not terminal. A command with no terminal record is shown as "no answer from
<machine>", with its age.

The signer of the newest `state` record for a slot is that slot's frame authority (NIP-SW), provided its
(`csl-command`, `cs-target`) resolves to that signer.

## Relay

Both kinds: `MessagesWrite`, `h` required, the strict coding-session membership gate, exact structure and the
host-local rule. The relay decides no standing.

## Kind numbers

44254 and 44255 come from the session-view parity registry. `git grep -n -w` on 2026-10-07 matched 44254 nowhere and
44255 only in that registry's note in `kind.rs`.
