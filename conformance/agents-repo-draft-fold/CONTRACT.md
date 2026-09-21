# Agents-repository draft fold — v1 conformance contract

This directory is the byte-exact source of truth for the agents-repository
draft fold. The Rust fold in `buzz-core` (`agents_repo_draft_fold.rs`, used
by `bee agents-repo`), the TypeScript fold in Desktop
(`features/agents-repo/lib/agentsRepoDraftFold.ts`) and the Dart fold in
Mobile (`features/agents_repo/domain/agents_repo_draft_fold.dart`) must bind
to the same vectors. A rule implemented in only one fold is a defect.

The fold reports facts about the op log. Whether a head is *stale against
`main`* (its `base` is no longer the blob at its path), who may commit, and
what a commit does with a diverged path are the concerns of a reader that
holds the repository; they consume these facts but are not part of this
contract.

## Inputs and scope

For one canonical kind-30621 project coordinate `30621:<owner-hex>:<dtag>`
and one canonical kind-30617 repository coordinate `30617:<owner-hex>:<id>`
— the agents repository the project's kind:30624 pins — the fold takes every
kind 44249 op selected by `#a`, nothing else. Each input event is read as:

```json
{ "id": "<64 hex>", "pubkey": "<64 hex>", "created_at": 0, "kind": 44249,
  "tags": [["a", "<project>"], ["ad-repo", "<repository>"], ...],
  "content": "<op JSON>" }
```

Signatures are not the fold's concern (the relay verified them at ingest).

## The op vocabulary (kind 44249 content, `buzz-agents-repo-draft/v1`)

The key set is exact per op: every listed key must be present (nullables as
`null`, never absent) and no other may be.

| `op` | keys | means |
|---|---|---|
| `file.put` | `path`, `text`, `base`, `baseCommit`, `prev`, `message` | the whole new text of `path` |
| `file.move` | `path`, `to`, `base`, `baseCommit`, `prev`, `message` | `path` moves to `to`, its archive counterpart |
| `file.delete` | `path`, `base`, `baseCommit`, `prev`, `message` | `path` is removed |
| `commit.record` | `commit`, `paths`, `drafts`, `message` | `commit` on `main` carries the named `drafts` |

`base` is the blob sha (40 hex) of the file on `main` the author started
from, or `null` for a file not there. `baseCommit` is advisory: the `main`
commit the author read, or `null`. `prev` is the event id of the draft head
the author edited from, or `null`. `message` is one line ≤ 512 bytes or
`null`. `text` is ≤ 60,000 bytes; content is ≤ 65,536. Paths follow the
agents repository layout (`README.md`, `team.yml`, `actions.yml`,
`roles/<slug>.md`, `roles/archive/<slug>.md`,
`roles/<slug>/skills/<skill>/…`, `skills/<skill>/…`, `plans/<slug>.md`,
`plans/archive/<slug>.md`; `archive` is never a slug). The event's tags are
`a`, `ad-v` (`ad1-1`), `ad-op`, `ad-repo`, and one `ad-path` per path the op
names. An `h` tag is a rejection.

Wire validation is `crates/buzz-core/src/agents_repo_draft.rs`; the fold's
decode is the same rule, and an event that fails it is counted in `ignored`,
not folded.

## Rules, in order

1. **Decode.** An event whose kind is not 44249, whose tags do not carry
   exactly one `a` naming this project (compared after normalizing hex
   case — ingest never stores a variant, and a reader that sees one anyway
   must not drop it), whose tags do not carry exactly one canonical
   `ad-repo`, or whose content fails the op grammar is counted in `ignored`
   and dropped. A well-formed op whose `ad-repo` is not this fold's
   repository is counted in `otherRepo` and dropped: it belongs to a
   repository the project no longer pins, and saying so beats losing it.
   Duplicate ids keep the first occurrence and count nothing.
2. **Order.** Ops sort by `(created_at, id)` ascending — the string `id`
   compared bytewise. That pair is the only clock.
3. **Close.** `closed` is the union of every `commit.record`'s `drafts`,
   whether or not the fold ever saw those ids. Records are emitted in
   `commits`, newest key first.
4. **Heads.** A file op *names* a path `P` when its `path` is `P`, or it is
   a `file.move` whose `to` is `P` (so a move is a candidate head at both
   ends: delete-shaped at `path`, put-shaped with the source blob at `to`).
   Per path, the ops naming it that are not in `closed` are its *open* ops.
   The greatest key is the `head`; the rest are `superseded`, oldest first.
   A path with no open op is absent from the digest.
5. **Divergence.** `diverged` is `true` when `superseded` is non-empty and
   the id of its last entry is not the head's `prev`: the head did not
   build on the newest other open op, so some open text is not in the
   head. It is reported, never resolved. A head whose `prev` names a closed
   or unseen op over an empty `superseded` is not diverged.
6. **Order out.** Paths sort bytewise. `updatedAt` on a path is the head's
   `created_at`.

## Digest shape (`buzz-agents-repo-draft-digest/v1`)

```json
{
  "schema": "buzz-agents-repo-draft-digest/v1",
  "project": "30621:<owner>:<dtag>",
  "repo": "30617:<owner>:<id>",
  "ignored": 0,
  "otherRepo": 0,
  "paths": [
    { "path": "roles/lead.md", "head": <row>, "superseded": [ <row>… ],
      "diverged": false, "updatedAt": 0 }
  ],
  "commits": [
    { "id": "<64 hex>", "commit": "<40 hex>", "by": "<64 hex>",
      "createdAt": 0, "paths": ["…"], "drafts": ["<64 hex>"], "message": null }
  ]
}
```

A row (head or superseded):

```json
{ "id": "<64 hex>", "author": "<64 hex>", "createdAt": 0, "op": "file.put",
  "path": "roles/lead.md", "to": null, "text": "…", "base": null,
  "baseCommit": null, "prev": null, "message": null }
```

Every nullable member is emitted as `null`, never omitted. `ignored` and
`otherRepo` are the fold's honesty counters: a client shows them when they
are not zero rather than presenting a listing that silently dropped
somebody's draft. Whether the read that produced the input was
**truncated** (the relay pages at 1000 events) is the caller's fact, carried
beside the digest, not inside it.

## Regenerating

`python3 conformance/agents-repo-draft-fold/generate-fold-vectors.py` from
the repository root rewrites `fixtures/fold-vectors.json` from the
hand-stated expectations in that script. Edit the expectation, then make all
three folds pass it — never the other way round.
