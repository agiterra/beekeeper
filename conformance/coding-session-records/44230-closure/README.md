# kind:44230 — coding-session closure revision

Changes the shared organizational state of one umbrella without addressing,
stopping or reviving any provider execution. Append-only; consumers fold the
history by `(created_at, event id)`.

## Closed key set — exactly four keys

`{action, genesisRef, sessionRef, v}`, and no fifth.

- `action` is a closed three-variant enum: `closed`, `open`, `archived`.
  `archived` implies closed and files the umbrella away; reopening it is an
  ordinary `open`.
- `genesisRef` is a lowercase 64-hex event id — the authority root, never a
  `sessionRef` scan and never a provider-authored execution fact.
- `sessionRef` is a canonical lowercase hyphenated UUID.
- `v` is exactly `1`. Content is bounded at 512 bytes.

## Where the readers disagree today

Mobile's decoder knows only `closed` and `open`: an archived umbrella decodes
as corruption there (`archived` vector). `"v": 1.0` is accepted by both
JavaScript and Dart and refused by serde, which is a property of the two number
models rather than of anyone's code — nothing signs it, and it is pinned so
nobody is surprised by it later.
