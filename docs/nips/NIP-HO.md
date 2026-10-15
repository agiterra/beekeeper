# NIP-HO — Coding-session handover records

`draft` `optional` `client` `relay`

`kind:44247` carries two records under one kind. A **checkpoint** is what the
work *is*, written durably enough that another authorized participant can
pick it up: the accepted task, the decisions taken, the revision it sits on,
the artifacts that carry the bytes, what the tests said, what is unresolved,
the next useful action, and what could **not** be preserved. A
**continuation** is the record of somebody having done that: which claim they
acted on, whether they resumed the original execution natively or
reconstructed the work elsewhere, and where it now runs.

> **Naming.** The module, the SDK builder and the tag prefix call this
> contract **NIP-CSH** (`csh-v`, `csh-type`, `csh-genesis`). The registry
> comment on `KIND_CODING_SESSION_HANDOVER` cites `docs/HANDOVER_IMPL.md` §2,
> a document that is not in this repository. This page is written from the
> code — [`crates/beekeeper-core/src/coding_session_handover.rs`](../../crates/beekeeper-core/src/coding_session_handover.rs)
> and [`crates/beekeeper-sdk/src/coding_session_handover.rs`](../../crates/beekeeper-sdk/src/coding_session_handover.rs) —
> and is the page those references should point at. Where it and the code
> disagree, the code is the contract.

## Signer and standing

Anyone may sign a 44247; the relay admits it on channel membership and
validates **structure only**, exactly as it does for 44244, 44245 and 44246.
Whether the author was the umbrella's founder, a live operator, or a seated
actor at the time, and whether a continuation's `claimRef` is the claim in
force, is decided by the consuming fold
(`coding_session_handover_fold`) against the accepted
[NIP-CSAT](NIP-CSAT.md) authority chain. A relay that adjudicated standing
would be asserting authority it cannot verify.

## Envelope

A regular stored event, channel-scoped. Exactly **five** ordered two-field
tags, and the decoder refuses any other count:

1. `h`: canonical lowercase channel UUID
2. `d`: the umbrella's `sessionRef` (must equal the payload's)
3. `csh-v`: `csh1`
4. `csh-genesis`: the umbrella's genesis event id (must equal the payload's)
5. `csh-type`: `checkpoint` or `continuation` (must equal the payload's)

Content ≤ 32 KiB (`MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES`), exact keys,
`deny_unknown_fields` throughout:

```json
{
  "schema": "buzz-coding-session-handover/v1",
  "sessionRef": "<lowercase uuid>",
  "genesisRef": "<64 lowercase hex>",
  "type": "checkpoint",
  "body": { ... }
}
```

`type` and the shape of `body` must agree; a `checkpoint` type over a
continuation body is refused.

## Checkpoint body

```json
{
  "prevCheckpointRef": null,
  "task": "Rebase the lane onto main and run the gate.",
  "assignmentRefs": ["<64 hex 44244 ids>"],
  "decisions": [{"eventId": "<64 hex>", "summary": "one line"}],
  "revision": {
    "repoRef": "30617:owner:repo", "baseSha": "<40 hex>", "headSha": "<40 hex>",
    "branch": "work/lane", "dirty": true, "preserved": "partial"
  },
  "artifacts": [
    {"kind": "wip-ref", "repoRef": "30617:owner:repo", "ref": "refs/wip/lane", "sha": "<40 hex>"},
    {"kind": "patch",   "repoRef": "30617:owner:repo", "eventId": "<64 hex>", "baseSha": "<40 hex>", "bytes": 4096},
    {"kind": "blob",    "repoRef": "30617:owner:repo", "hash": "<64 hex>", "baseSha": "<40 hex>", "bytes": 1048576}
  ],
  "tests": [{"name": "unit", "command": "cargo test -p x", "outcome": "passed"}],
  "unresolved": ["whether the fixture should move"],
  "nextAction": "Run just check and push.",
  "missing": ["desktop/.env.local (untracked, not captured)"]
}
```

| field | rule |
| --- | --- |
| `prevCheckpointRef` | **always present**, `null` for the author's first checkpoint of this umbrella, else the event id of the author's previous one. The author's own statement of what this replaces; the only ordering the fold trusts within one author (three checkpoints in one second sort by hash otherwise). |
| `task` | ≤ 4 KiB |
| `assignmentRefs` | ≤ 16 event ids of kind 44244 assignments this work answers |
| `decisions` | ≤ 32, each `{eventId, summary ≤ 512 B}` |
| `revision` | `repoRef`/`baseSha`/`headSha`/`branch` nullable; `dirty` bool; `preserved` ∈ `all`, `partial`, `none` |
| `artifacts` | ≤ 16; `kind` ∈ `wip-ref` (`ref`, `sha`), `patch` (NIP-34 `eventId`, `baseSha`, `bytes`), `blob` (Blossom `hash`, `baseSha`, `bytes`); per-kind fields present exactly for that kind |
| `tests` | ≤ 32 rows; `outcome` ∈ `passed`, `failed`, `not-run` — `not-run` is never a stand-in for passed |
| `unresolved`, `missing` | ≤ 32 and ≤ 16 lines of ≤ 512 B |
| `nextAction` | ≤ 2 KiB |

Two cross-field honesty rules the decoder enforces on its own:

- `preserved: "none"` with a `patch` or `blob` artifact is a contradiction and
  is refused — say `partial` or `all`, or drop the artifact.
- `preserved` other than `all` with an empty `missing` list is refused: if
  something was not preserved, name it.

`preserved` is stated by the author, never inferred by a reader. `dirty` says
uncommitted changes existed; it does not say whether they survived.

## Continuation body

```json
{
  "claimRef": "<64 hex: the accepted 44228 takeover/transfer>",
  "mode": "reconstructed",
  "checkpointRef": "<64 hex 44247, or null>",
  "target": {"driver": "codex-acp", "instanceId": "…", "sessionId": "…", "generation": 1},
  "recovered": ["branch work/lane at <sha>", "patch applied cleanly"],
  "missing": ["native context: the cursor never leaves the original disk"],
  "note": null
}
```

- `mode` ∈ `native-resume` (the original execution resumed on its own
  provider) or `reconstructed` (a new execution created from the artifacts).
  Never conflated: a reconstruction does not carry the native context and the
  label says so.
- `checkpointRef` is `null` when no checkpoint existed and the claimant
  proceeded anyway — and said so.
- `target` is the NIP-CSC exact target now carrying the work.
- `recovered` ≤ 32 lines, `missing` ≤ 16 lines, `note` ≤ 2 KiB or `null`.

## Building one

`beekeeper_sdk::coding_session_handover::build_coding_session_handover(channel, payload)`
emits the exact five-tag envelope, runs the strict decoder over its own bytes
before returning them, and leaves signing to the caller;
`parse_coding_session_handover(&event)` is the reader. The relay runs
`validate_coding_session_handover_envelope` at ingest and nothing more.

## How it is used

A provider that goes absent publishes a checkpoint; an authorized participant
publishes a 44228 `takeover` or `transfer` ([NIP-CSAT](NIP-CSAT.md)), acts on
it, and publishes a continuation naming that claim. The provider that comes
back is **fenced**: every command it would answer for the handed-over
umbrella is refused `HANDOVER_FENCED` ([NIP-CSE](NIP-CSE.md)) until a fresh
accepted claim says otherwise, so two divergent executions of one task cannot
both believe they are real. The composition proof is
`scripts/handover-acceptance.sh` (`just test-handover`).

## Reading from outside

```json
["REQ","handover",{"kinds":[44247],"#h":["<channel>"],"#d":["<sessionRef>"]}]
```

Verify the signature, then the five tags against the content, then decide
standing from the 44228 chain. Do not pick "the latest checkpoint" by
`created_at`: follow `prevCheckpointRef` within one author.
`conformance/foreign-client/foreign-native-read.mjs` is that read with
signature verification; [`docs/INTEGRATING.md`](../INTEGRATING.md) is the
page that puts it beside the other kinds a foreign client meets.
