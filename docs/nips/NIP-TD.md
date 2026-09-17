NIP-TD
======

Project To-Do Lists
-------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-MP (the project a list belongs to, and its roster). Interacts with NIP-09 (deletion of an individual op).

## Abstract

This NIP defines `kind:44248`, a **project to-do op**: one signed, field-level edit to a shared to-do list that belongs to a NIP-MP project. A list is not an event. It is what every reader gets by folding the ops that name it, under one pure rule set shared by every client (`conformance/project-todo-fold/CONTRACT.md`). Any writer the project admits may edit any item; there is no per-author ownership of a list or an item.

## Motivation

A project needs a place for its people and agents to keep a short, shared list of what is left to do — with a box to tick, an optional owner, an optional date, and an order — that every member sees change as it changes. The forum is for discussion, Pulse (NIP-MP kind:44240) is for one author's *claims* about their own work, and the transcript plan rail is a seat's private scratchpad. None is a shared, ticked list.

Why not an addressable event per list or per item (NIP-33)? Replacement keys on `(kind, pubkey, d)`: one head per **author**. Two collaborators editing one list would produce two independent heads, and the reader would have to guess which one is the list. Buzz met this exact hazard with the project pack source (kind:30624, ledger item 113) and resolved it with a relay-side cross-author conditional write; that path costs a transaction lock and a conflict response on every edit, and a to-do list is edited far more often than a pack pin.

The design that fits is the one Pulse already established: a regular, append-only kind, scoped to the project by its canonical `a` coordinate, gated by project membership at every relay surface, and folded client-side. What is new here is that each op sets exactly **one field**, so the fold can take the latest write per field and two people editing different fields of the same item never overwrite each other.

## Event

`kind:44248` — regular, stored, append-only. Never replaceable.

Tags, position-independent, closed key set:

| tag | multiplicity | value |
|---|---|---|
| `a` | exactly one | canonical `30621:<lowercase-hex>:<dtag>` project coordinate |
| `td-v` | exactly one | `td1-1` |
| `td-op` | exactly one | the op name, equal to the content `op` |
| `td-list` | exactly one | the list id, equal to the content `listId` |
| `td-item` | exactly one on an item op, none on a list op | the item id, equal to the content `itemId` |

Any other key — **including `h`** — is a rejection. A to-do op is never channel-scoped; unlike a Pulse entry there is no optional channel binding to reconcile, so the relay lists the kind as global-only outright.

Content is JSON, at most 4096 bytes, with `schema` = `buzz-project-todo/v1` and an **exact** key set per op (every listed key present, no other key; `assignee` and `due` are `null` when cleared, never absent):

| `op` | keys |
|---|---|
| `list.create` | `listId`, `title` |
| `list.title` | `listId`, `title` |
| `list.archived` | `listId`, `archived` (boolean) |
| `item.add` | `listId`, `itemId`, `text`, `rank` |
| `item.text` | `listId`, `itemId`, `text` |
| `item.done` | `listId`, `itemId`, `done` (boolean) |
| `item.assignee` | `listId`, `itemId`, `assignee` (64 lowercase hex, or null) |
| `item.due` | `listId`, `itemId`, `due` (`YYYY-MM-DD`, a real calendar date, or null) |
| `item.rank` | `listId`, `itemId`, `rank` |
| `item.remove` | `listId`, `itemId` |

`listId` and `itemId` are 32 lowercase hex characters minted by the client (a UUID without hyphens). `title` and `text` are non-blank, at most 1024 bytes, with no control characters other than newline and tab. `rank` is a fractional-indexing order key (see § Ranks).

The single validator is `crates/buzz-core/src/project_todo.rs`; the relay, the SDK builder and `bee todos` call it and keep no copy.

## Relay behaviour

Everything below is the Pulse rule applied through one predicate, `buzz_core::kind::is_project_a_scoped_kind` (44240, 44248). A relay chokepoint that gates on a project's hidden set keys on the predicate, so both kinds are admitted, withheld, fanned out, pushed down and shape-checked by the same code.

**Ingest.** Scope `messages:write`. The envelope is validated, the coordinate must name a project that exists in this community, and the author must be admitted:

- private project — the roster's `owner` or `collaborator` may write; a `viewer` may not;
- public project — any community member may write;
- unknown coordinate — refused.

A refusal is an authorization failure (`OK false "restricted: …"` on the wire, **403** over `POST /events`, CLI exit 3), so an agent script can tell it from a malformed event (400, exit 2).

**Reads.** A stored op is withheld from a reader whose hidden-private-project set contains its coordinate unless the reader is its author, on every surface: WS `REQ`, `POST /query`, `POST /count`, FTS, and live fan-out. An inadmissible read is an empty `200`, never a `403`. The SQL pushdown excludes hidden coordinates before `ORDER`/`LIMIT` so a private project cannot starve a page; the per-event gate stays as defence in depth.

**Request shape (HTTP bridge).** A filter naming a project-scoped kind must name exactly one canonical coordinate in `#a` and no kind outside the set; `{"kinds":[44240,44248],"#a":[c]}` is accepted (one gate, one coordinate), `{"kinds":[44248]}` and `{"kinds":[44248,9],…}` are `400`. WS `REQ` accepts any shape and gates per event.

**Deletion.** A NIP-09 `kind:5` from the op's author deletes the op. The fold then simply never sees it: an item whose `item.add` is deleted disappears with every op on it; a list whose `list.create` is deleted disappears with its items.

**Storage.** The generic `events` table; the `a` tag is served by the JSONB tag index. No migration.

## Fold

Normative text is `conformance/project-todo-fold/CONTRACT.md`, pinned by `fixtures/fold-vectors.json`, which the Rust (`buzz-core`), TypeScript (Desktop) and Dart (Mobile) folds all bind to. In one paragraph: decode every op for the coordinate (a malformed op is counted in `ignored`); sort by `(created_at, id)`; the earliest `list.create` / `item.add` per id creates; any `item.remove` on an existing item is terminal; per field the greatest `(created_at, id)` wins, with the create's values taking part under the create's key; `item.done{true}` records `completedAt`/`completedBy` from the winning op; lists order by `(createdAt, id)`, open items by `(rank, id)`, completed items by the winning done op's key descending.

## Ranks

Open items are ordered by `rank`, a fractional-indexing order key over the base-62 alphabet `0-9A-Za-z` compared bytewise: an integer part whose first letter encodes its length, then an optional fraction never ending in `0` (`conformance/project-todo-fold/fixtures/rank-vectors.json`; `crates/buzz-core/src/fractional_rank.rs`). The first rank is `a0`; appending increments the integer (`a1`, `a2`, …), so a list of thousands keeps three-character ranks; a drag or `bee todos move --index N` mints one key between the new neighbours and publishes one `item.rank` op. Equal ranks are legal and order by item id.

## Timestamps

The relay refuses any event whose `created_at` is more than 900 s from its clock. Writers stamp `created_at = max(now, latest seen op on the same target + 1)` so a write made after reading the current state wins the field even on a slightly slow clock; if that exceeds the window they surface a retry rather than silently dropping the bump. Live subscriptions use `since = now − 900` and dedupe by id, because a peer may legally stamp up to 900 s in the past.

## Known limitations

- **Cold read is the whole log.** A reader replays every op for the coordinate (relay page cap 1000). Clients paginate with `until` and disclose truncation rather than presenting a list that silently lost history. A compaction record is a possible follow-up; it is not part of this NIP.
- **Purging a project leaves its ops readable** to community members, inherited from Pulse: deleting the `kind:30621` head drops the project's ACL row, after which the coordinate is in nobody's hidden set, and `project_purge` does not touch channel-less `a`-tagged events. Recorded in the ledger; not fixed here.
- **Authority is project-level.** Any writer may edit or remove anyone's item. This is the intended shape of a shared list, stated so nobody expects per-author ownership.

## Reference

- Kind and predicates: `crates/buzz-core/src/kind.rs` (`KIND_PROJECT_TODO_OP`, `is_project_a_scoped_kind`, `project_a_scoped_coordinate`, `project_a_scoped_event_hidden_from`).
- Validator: `crates/buzz-core/src/project_todo.rs`. Fold: `crates/buzz-core/src/project_todo_fold.rs`. Ranks: `crates/buzz-core/src/fractional_rank.rs`.
- Relay: `crates/buzz-relay/src/handlers/ingest.rs` (`admit_project_scoped_write`), `req.rs`, `event.rs`, `api/bridge.rs`; `crates/buzz-db/src/event.rs` pushdown.
- End-to-end: `crates/buzz-test-client/tests/e2e_project_todos.rs`.
