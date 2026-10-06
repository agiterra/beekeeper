# Project artifact pin fold — v1 conformance contract

This directory is the byte-exact source of truth for the project artifact pin
fold. The Rust fold in `buzz-core`
(`project_artifact_pin_fold.rs`, behind `bee pins`), the TypeScript fold in
Desktop (`features/agents-repo/lib/artifactPinFold.ts`) and the Dart fold in
Mobile (`features/agents_repo/domain/artifact_pin_fold.dart`) must bind to the
same vectors. They sit beside the draft modules rather than in a feature of
their own because a pin's target is drawn from the same path grammar, and the
conformance binders load these modules with no resolver — a relative import
with an explicit extension, never an alias. A rule implemented in only one fold is a defect, and nothing
else in CI would catch it: each fold's own tests use its own table.

The fold reports facts about the op log. Whether a pinned target still exists
on `main`, who may write a pin, and whether the reader has chosen to hide the
pinned rows are the concerns of a reader that holds the repository and the
person's own preferences; they consume these facts and are not part of this
contract.

## Inputs and scope

For one canonical kind-30621 project coordinate `30621:<owner-hex>:<dtag>` and
one canonical kind-30617 repository coordinate `30617:<owner-hex>:<id>` — the
agents repository the project's kind:30624 pins — the fold takes every kind
44251 op selected by `#a`, nothing else. Each input event is read as:

```json
{ "id": "<64 hex>", "pubkey": "<64 hex>", "created_at": 0, "kind": 44251,
  "tags": [["a", "<project>"], ["ar-repo", "<repository>"], ...],
  "content": "<op JSON>" }
```

Signatures are not the fold's concern (the relay verified them at ingest).

## The op vocabulary (kind 44251 content, `buzz-project-artifact-pin/v1`)

The key set is exact per op: every listed key must be present and no other may
be. Nothing here is nullable.

| `op` | keys | means |
|---|---|---|
| `pin.set` | `target`, `targetKind`, `pinned`, `rank` | `target` is (or is no longer) in every member's sidebar, entering the order at `rank` |
| `pin.rank` | `target`, `rank` | `target` moves to `rank` |

`targetKind` is `file` or `folder`. A **file** target is any path the agents
repository's grammar admits (`conformance/agents-repo-draft-path/`) except a
`document-folder` keep — the keep is how an empty directory exists in git, and
pinning it instead of its folder would put a row called `.gitkeep` in the
sidebar. A **folder** target is `docs/<segment>/…` with one to seven segments,
each a document name; it is deliberately *not* a path the grammar admits,
because there is no such object in git, only the prefix its files share. Seven,
not eight, so a file under the deepest pinnable folder still fits the path cap.

`rank` is a fractional-indexing order key over the base-62 alphabet, the same
one NIP-TD uses (`conformance/project-todo-fold/fixtures/rank-vectors.json`).

The event's tags are `a`, `ar-v` (`ar1-1`), `ar-op`, `ar-repo` and `ar-target`,
each exactly once. An `h` tag is a rejection: a pin belongs to a project, never
to a room.

Wire validation is `crates/beekeeper-core/src/project_artifact_pin.rs`; each fold's
decode is the same rule, and an event that fails it is counted in `ignored`,
not folded.

## Rules, in order

1. **Decode.** An event whose kind is not 44251, whose tags do not carry
   exactly one `a` naming this project (compared after normalizing hex case —
   ingest never stores a variant, and a reader that sees one anyway must not
   drop it), whose tags do not carry exactly one canonical `ar-repo`, or whose
   content fails the op grammar is counted in `ignored` and dropped. A
   well-formed op whose `ar-repo` is not this fold's repository is counted in
   `otherRepo` and dropped: it belongs to a repository the project no longer
   pins, and saying so beats losing it. Duplicate ids keep the first occurrence
   and count nothing.
2. **Order.** Ops sort by `(created_at, id)` ascending — the string `id`
   compared bytewise. That pair is the only clock.
3. **Exist.** A target is in the digest when at least one `pin.set` names it.
   A `pin.rank` alone cannot conjure a row: it says where something sits, not
   that it belongs in the list, so a reorder of a target nobody ever pinned —
   or one whose `pin.set` was deleted under NIP-09 — is counted in
   `ranksWithoutPin` rather than dropped in silence. This is decided over the
   **whole bag**, not the order it arrived in: a `pin.rank` that appears before
   its `pin.set` in the input still counts.
4. **Fields.** Per target, per field, the write with the greatest
   `(created_at, id)` wins. `pinned` and `targetKind` come from `pin.set`
   alone. `rank` is contested by both ops, with a `pin.set`'s own rank taking
   part under the set's key — so a `pin.rank` stamped *before* the set that
   introduced the target (a skewed clock) loses to it, exactly as NIP-TD
   resolves `item.add` against a later field op.
5. **Order out.** Rows sort by `(rank, target)`, both bytewise. Equal ranks are
   legal and break on the target. `updatedAt` is the greatest applied op key
   for that target; `by` is the author of the winning `pin.set` — who pinned
   it, not who last nudged its order.

An **unpinned** row stays in the digest with `pinned: false`. A caller drawing
a sidebar filters those out; a caller drawing the pin control needs to know the
target was pinned once and is not now, and a row that vanished would make the
control guess.

## Digest shape (`buzz-project-artifact-pin-digest/v1`)

```json
{
  "schema": "buzz-project-artifact-pin-digest/v1",
  "project": "30621:<owner>:<dtag>",
  "repo": "30617:<owner>:<id>",
  "ignored": 0,
  "otherRepo": 0,
  "ranksWithoutPin": 0,
  "pins": [
    { "target": "docs/mockups/login.html", "targetKind": "file",
      "pinned": true, "rank": "a0", "by": "<64 hex>", "updatedAt": 0 }
  ]
}
```

`ignored`, `otherRepo` and `ranksWithoutPin` are the fold's honesty counters: a
client shows them when they are not zero rather than presenting a sidebar that
silently dropped somebody's pin. Whether the read that produced the input was
**truncated** (the relay pages at 1000 events) is the caller's fact, carried
beside the digest, not inside it.

## Regenerating

`python3 conformance/project-artifact-pin-fold/generate-fold-vectors.py` from
the repository root rewrites `fixtures/fold-vectors.json` from the hand-stated
expectations in that script. Edit the expectation, then make all three folds
pass it — never the other way round.
