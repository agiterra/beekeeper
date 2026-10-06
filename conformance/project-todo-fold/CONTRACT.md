# Project to-do fold — v1 conformance contract

This directory is the byte-exact source of truth for the project to-do fold
and the fractional rank beneath it. The Rust fold in `buzz-core`
(`project_todo_fold.rs`, used by `bee todos`), the TypeScript fold in Desktop
(`features/project-todos/lib/todoFold.ts`) and the Dart fold in Mobile
(`features/project_todos/domain/project_todo_fold.dart`) must bind to the
same vectors. A rule implemented in only one fold is a defect.

The fold reports facts. Rendering, write access and the "read-only: you are a
viewer" disclosure consume those facts but are not part of this contract.

## Inputs and scope

For one canonical kind-30621 project coordinate `30621:<owner-hex>:<dtag>`,
the fold takes every kind 44248 op selected by `#a` — nothing else. There is
no channel, no `h`, no session join. Each input event is read as:

```json
{ "id": "<64 hex>", "pubkey": "<64 hex>", "created_at": 0, "kind": 44248,
  "tags": [["a", "<coordinate>"], ...], "content": "<op JSON>" }
```

Signatures are not the fold's concern (the relay verified them at ingest).

## The op vocabulary (kind 44248 content, `buzz-project-todo/v1`)

Every op sets exactly one thing. The key set is exact per op: every listed
key must be present and no other may be; `assignee` and `due` are `null`,
never absent, when cleared.

| `op` | keys | sets |
|---|---|---|
| `list.create` | `listId`, `title`, `visibility` (`project` or `personal`) | the list into existence, and who may read it, for good |
| `list.title` | `listId`, `title` | title |
| `list.archived` | `listId`, `archived` (bool) | archived, reversible |
| `list.pinned` | `listId`, `pinned` (bool) | shown in every member's project sidebar, reversible |
| `item.add` | `listId`, `itemId`, `text`, `rank` | the item into existence |
| `item.text` | `listId`, `itemId`, `text` | text |
| `item.done` | `listId`, `itemId`, `done` (bool) | done, and completion |
| `item.assignee` | `listId`, `itemId`, `assignee` (64 hex or null) | assignee |
| `item.due` | `listId`, `itemId`, `due` (`YYYY-MM-DD` or null) | due date |
| `item.rank` | `listId`, `itemId`, `rank` | position among open items |
| `item.remove` | `listId`, `itemId` | the item out of existence, for good |

`schema` is always present. `listId` and `itemId` are 32 lowercase hex.
Titles and text are non-blank, at most 1024 bytes, free of control characters
other than newline and tab. Content is at most 4096 bytes. The event's tags
are `a`, `td-v` (`td1-1`), `td-op`, `td-list`, `td-vis` (`project` or
`personal`), and `td-item` on item ops; `td-op`/`td-list`/`td-item` equal the
content's `op`/`listId`/`itemId`, and on a `list.create` `td-vis` equals the
content's `visibility`. An `h` tag is a rejection.

**Visibility.** A list is `project` — every member of the project reads and
edits it — or `personal` — only its author does. The choice is fixed by
`list.create` and repeated on **every** op of the list as `td-vis`, so the
relay withholds a personal op from every reader but its author (WS `REQ`,
`/query`, `/count`, live fan-out) without parsing content. A pin on a
personal list is therefore only ever seen by its owner.

Wire validation is `crates/beekeeper-core/src/project_todo.rs`;
the fold's decode is the same rule, and an event that fails it is counted in
`ignored`, not folded.

## Rules, in order

1. **Decode.** An event whose kind is not 44248, whose tags do not carry
   exactly one `a` naming this project (compared after normalizing hex
   case), whose tags do not carry exactly one `td-vis` with a known value,
   or whose content fails the op grammar for that visibility is counted in
   `ignored` and dropped. Duplicate ids keep the first occurrence and count
   nothing.
2. **Order.** Ops sort by `(created_at, id)` ascending — the string `id`
   compared bytewise. That pair is the only clock; there is no per-author
   sequence and no vector clock.
3. **Create.** The earliest `list.create` per `listId` and the earliest
   `item.add` per `(listId, itemId)` bring the target into existence with the
   op's values, author and `created_at`. A later create for an existing id is
   counted in `ignored`. An `item.add` naming a list that was never created
   is counted in `ignored`. A list's visibility is its create's. Every other
   op on an existing list must carry the same `td-vis`, and on a `personal`
   list must be signed by the list's creator; an op failing either is
   counted in `ignored` (checked before the remove and field rules).
4. **Remove.** Any `item.remove` on an existing item is terminal, whenever it
   was stamped: the item is dropped from the digest and every other op on it
   is disregarded without being counted. An `item.remove` on an item that
   never existed is counted in `ignored`. There is no un-remove.
5. **Fields.** Each remaining op sets one field. Per field, the write with
   the greatest `(created_at, id)` wins. The create op's own values (`title`,
   `archived=false`, `pinned=false` for a list; `text` and `rank` for an
   item, plus `done=false`, `assignee=null`, `due=null`) take part with the
   create's key, so a field
   write stamped before the create loses to it. A field op naming a list or
   item that does not exist is counted in `ignored`.
6. **Done.** The winning `item.done` decides `done`. If it is `true`,
   `completedAt` is that op's `created_at` and `completedBy` its author;
   otherwise both are `null`.
7. **Timestamps.** `updatedAt` on an item is the greatest `created_at` among
   the ops applied to it (winners and losers alike; a loser is never later
   than its winner). `updatedAt` on a list is the greatest over its own ops,
   its items' ops, and removes.
8. **Order out.** Lists sort by `(createdAt, id)`. Within a list, `open`
   holds the not-done items sorted by `(rank, id)` bytewise and `completed`
   holds the done items sorted by the winning done op's `(created_at, id)`
   descending — most recently completed first.

## Digest shape (`buzz-project-todo-digest/v1`)

```json
{
  "schema": "buzz-project-todo-digest/v1",
  "project": "30621:<owner>:<dtag>",
  "ignored": 0,
  "lists": [
    {
      "id": "<32 hex>", "title": "…", "visibility": "project",
      "archived": false, "pinned": false,
      "createdAt": 0, "createdBy": "<64 hex>", "updatedAt": 0,
      "open": [ <item>… ], "completed": [ <item>… ]
    }
  ]
}
```

An item:

```json
{ "id": "<32 hex>", "listId": "<32 hex>", "text": "…", "done": false,
  "rank": "a0", "assignee": null, "due": null,
  "createdAt": 0, "createdBy": "<64 hex>", "updatedAt": 0,
  "completedAt": null, "completedBy": null }
```

Every nullable member is emitted as `null`, never omitted. `ignored` is the
fold's honesty counter: a client shows it when it is not zero rather than
presenting a list that silently dropped somebody's write. Whether the read
that produced the input was **truncated** (the relay pages at 1000 events) is
the caller's fact, carried beside the digest, not inside it.

## Ranks (`fixtures/rank-vectors.json`)

A rank is a fractional-indexing order key over the base-62 alphabet
`0-9A-Za-z`, compared bytewise: an integer part whose first letter encodes
its length (`a`+1 digit … `z`+26 digits for non-negative, `Z`+1 … `A`+26 for
negative), then an optional fraction that never ends in `0`. The first rank
of an empty list is `a0`. `rankBetween(after, before)` (either side may be
absent) mints a key strictly between its bounds; the midpoint digit between
fraction digits `a` and `b` is `⌈(a + b) / 2⌉` in integer arithmetic. A rank
is at most 64 bytes. `between` vectors are `[after, before, expected]`;
`invalid` vectors must be refused by every implementation's validator.

Ties in rank (two clients minting the same key concurrently) are legal and
resolve by item id (rule 8).

The vectors are produced by `generate-fold-vectors.py` beside this file
from hand-stated expectations; edit the script, rerun it, and make all
three folds pass.

## Changing this contract

Bump the digest schema and the vectors' schema together. Add a vector for
every rule you add or change, then make all three folds pass it. Never edit
an existing vector's expectation to match one implementation; if two
implementations disagree, the contract decides which one is wrong.
