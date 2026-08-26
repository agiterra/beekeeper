# Analyzing recorded coding sessions

Coding sessions are stored, not streamed-and-forgotten. Every turn, every tool
call, every result is a signed event the relay already persists, which makes the
relay's own database the analysis database — no export pipeline, no warehouse,
no second copy to keep coherent.

This page is how to ask it questions: the CLI for the common ones, direct SQL
for everything else.

## Where the transcripts live

The eleven coding-session kinds, plus kind 44240, land in the relay's Postgres
`events` table
(`migrations/0001_initial_schema.sql`), partitioned by `created_at`:

| Kind | What it is | Contract |
| --- | --- | --- |
| 44220 | Turn command (operator → provider) | [NIP-CSC](nips/NIP-CSC.md) |
| 44221 | Lifecycle create command | [NIP-CSL](nips/NIP-CSL.md) |
| 44222 | Provider catalog | [NIP-CSPC](nips/NIP-CSPC.md) |
| 44223 | Per-generation metadata | [NIP-CSL](nips/NIP-CSL.md) |
| 44224 | Lifecycle receipt | [NIP-CSL](nips/NIP-CSL.md) |
| 44225 | Transcript item | [NIP-CST](nips/NIP-CST.md) |
| 44226 | Umbrella session genesis (founder) | [NIP-CSG](nips/NIP-CSG.md) |
| 44227 | Goal revision | [NIP-CSG](nips/NIP-CSG.md) |
| 44228 | Authority transition (draft) | [NIP-CSAT](nips/NIP-CSAT.md) |
| 44229 | Name revision | [NIP-CSG](nips/NIP-CSG.md) |
| 44230 | Closure revision | [NIP-CSG](nips/NIP-CSG.md) |
| 44240 | Project Pulse entry — **pending the Slice 1 branch split** | `crates/buzz-core/src/pulse.rs` (no published NIP yet) |

44220–44225 are the per-generation, provider-authored kinds the CLI's
`sessions` subcommand and the SQL below were originally written against.
44226–44230 are the human-authored facts about the umbrella session those
generations belong to — who founded it, what it's for, who else may steer it,
what it's called, and whether it's closed — and are channel-scoped like the
rest. 44240 is different in kind, not degree: it is not a coding-session event
at all, but a Project Pulse coordination claim scoped to a project by an `a` tag
rather than to a channel by `h` — though an optional `h` may still be present
(`crates/buzz-core/src/kind.rs:712-713`) — optionally cross-referencing an
umbrella session via `pu-session`. It is included here because it lives in the same `events` table
and answers the same kind of question ("what is this project's work actually
doing right now?"). As of this writing kind 44240 exists in `buzz-core` and
`buzz-cli` on a `wip/*` branch and has not yet been split into its owning
`feature/*` branch — treat the examples below as correct against the schema,
not as proof the kind is live on any deployed relay.

There is no separate kinds table: `kind` is an `INT` column, and the registry of
what each integer means is `crates/buzz-core/src/kind.rs`. The columns that
matter for analysis:

- `kind` — the integer above.
- `channel_id` — materialized UUID of the `h` tag, so channel scoping is a plain
  equality filter rather than a JSONB probe.
- `community_id` — always filter on it. Every index is community-leading, and a
  query without it scans partitions.
- `tags` — JSONB array of arrays, GIN-indexed with `jsonb_path_ops`
  (`migrations/0004_events_tags_gin.sql`). Probe it with containment:
  `tags @> '[["cs-target","…"]]'::jsonb`.
- `content` — `TEXT` holding the JSON payload. Cast it: `content::jsonb`.
- `deleted_at` — non-null means tombstoned. Filter it out.

Analysis is read-only against stored rows. The private-project ACL that hides a
channel from a user in the app is applied at query time by the relay, not baked
into these rows, so **direct SQL sees everything in the community** — treat a
psql session against the relay as a privileged operation. This applies doubly to
kind 44240: its project-ACL gate (`pulse_entry_hidden_from` in
`crates/buzz-core/src/kind.rs`) is enforced by the relay per reader at query
time, not by anything in the row itself, so a raw SQL scan returns entries for
projects the querying human may not be a member of.

## The CLI surface

`bee sessions` covers the questions that come up most, and resolves generations
the same way the desktop app does (a **lifecycle** receipt confirms a
generation exists; the newest metadata wins, with a same-second burst broken
on event id). A **turn** receipt (`turn_queued`/`turn_started`/`turn_dropped`/
`turn_refused`, [NIP-CSL](nips/NIP-CSL.md) fork amendment 7) is the deliberate
exception — it never creates, confirms, or ends a generation, so it plays no
part in this resolution. Use the CLI when you want the CLI and the app to
agree.

```
bee sessions list       --channel <uuid>
bee sessions transcript --channel <uuid> --target <cs-target> | --session <sessionId>
                         [--format md|jsonl]
bee sessions tools      --channel <uuid> [--target <cs-target>]
bee sessions export     --channel <uuid> --out <dir>
bee sessions grant      --channel <uuid> --genesis <event-id> --pubkey <hex> --role collaborator|viewer
bee sessions revoke     --channel <uuid> --genesis <event-id> --pubkey <hex>
bee sessions roster     --channel <uuid> --genesis <event-id>
```

- **`list`** — one row per generation: target key, title, status, model, and
  created-at. `--format` is the global flag (`bee --format compact sessions
  list …`); `compact` drops everything but those five fields.
- **`transcript`** — items in `cst-seq` order. `--format md` renders turns as
  headings with tool calls folded into one line each carrying their outcome;
  `--format jsonl` emits the raw signed events, one per line, signature
  included, so the output stays independently verifiable.
- **`tools`** — per-tool call counts, error counts, and error rate, sorted by
  frequency. The JSON form also reports `itemKinds` (every item kind seen, with
  anything the CLI does not recognize folded into `other`) and
  `malformedEvents`, so a number is never quietly averaged over an unreadable
  channel.
- **`export`** — one `<sessionId>-g<generation>.jsonl` per generation plus a
  `manifest.json` describing targets, counts, and time range. The output
  directory must be absent or empty; an export never overwrites, because a
  manifest that does not describe the files beside it is worse than no export.
- **`grant` / `revoke`** — extend a session's authority chain (kind 44228):
  `grant --role collaborator` mints a `grant-operator` transition, `grant
  --role viewer` mints `grant-viewer`, `revoke` mints `revoke`. Only the
  session owner (the genesis signer) may call these; the relay rejects
  anything else atomically with storage.
- **`roster`** — a session's folded grant map plus any pending transitions.
  Grants are folded from relay-signed acceptance receipts (kind 40099) in
  `seq` order, not from raw 44228 rows directly, so a transition with no
  matching receipt shows up separately as pending rather than silently
  granting standing it never received.

There is no CLI surface yet for genesis (44226), goal (44227), name (44229),
closure (44230), or Pulse (44240, which is besides still unsplit from `wip/*`) —
query those directly, below.

Exit codes are the usual ones: `0` ok, `1` bad input or not found, `2`
relay/network, `3` auth, `4` other.

## Direct SQL

Nine starting points. Add `AND community_id = '<uuid>'` to every one of them in
a multi-tenant deployment, and `AND channel_id = '<uuid>'` to scope to a single
channel — **except the kind-44240 query**, whose `h` tag is optional, so a
`channel_id` predicate silently drops every Pulse entry posted without one.
Scope that one by its `a` coordinate instead.

### Tool frequency across all sessions

Which tools does the fleet actually reach for?

```sql
SELECT
    item -> 'tool' ->> 'toolName' AS tool_name,
    COUNT(*)                      AS calls
FROM (
    SELECT (content::jsonb) -> 'item' AS item
    FROM events
    WHERE kind = 44225
      AND deleted_at IS NULL
) AS transcript_items
WHERE item ->> 'kind' = 'tool_call'
GROUP BY tool_name
ORDER BY calls DESC;
```

### Error rate by tool

A `tool_result` names its tool only sometimes, so join it back to its call on
`toolId`. A `LEFT JOIN` keeps calls that never produced a result — those are
turns that were interrupted or crashed, and dropping them would flatter the
numbers.

```sql
WITH items AS (
    SELECT (content::jsonb) -> 'item' AS item
    FROM events
    WHERE kind = 44225
      AND deleted_at IS NULL
),
calls AS (
    SELECT
        item -> 'tool' ->> 'toolId'   AS tool_id,
        item -> 'tool' ->> 'toolName' AS tool_name
    FROM items
    WHERE item ->> 'kind' = 'tool_call'
),
results AS (
    SELECT
        item ->> 'toolId'              AS tool_id,
        (item ->> 'isError')::boolean  AS is_error
    FROM items
    WHERE item ->> 'kind' = 'tool_result'
)
SELECT
    calls.tool_name,
    COUNT(*)                                          AS calls,
    COUNT(*) FILTER (WHERE results.is_error)          AS errors,
    ROUND(
        COUNT(*) FILTER (WHERE results.is_error)::numeric / COUNT(*),
        3
    )                                                 AS error_rate
FROM calls
LEFT JOIN results USING (tool_id)
GROUP BY calls.tool_name
ORDER BY errors DESC, calls DESC;
```

### Sessions per project

`projectRef` is `null` for standalone sessions, which `COALESCE` reports as its
own bucket rather than dropping.

```sql
SELECT
    COALESCE((content::jsonb) ->> 'projectRef', '(standalone)') AS project_ref,
    COUNT(DISTINCT (content::jsonb) #>> '{session,sessionId}')  AS sessions,
    COUNT(*)                                                    AS metadata_events
FROM events
WHERE kind = 44223
  AND deleted_at IS NULL
GROUP BY project_ref
ORDER BY sessions DESC;
```

### Full transcript dump for one generation

Take the `cs-target` key from `bee sessions list`. **Order by the numeric
sequence, not the tag text** — `cst-seq` is a decimal string on the wire, so a
lexicographic sort puts item 10 before item 9 and silently reorders every
transcript longer than nine items.

```sql
SELECT
    ((content::jsonb) ->> 'eventSeq')::bigint  AS seq,
    (content::jsonb) ->> 'turnId'              AS turn_id,
    (content::jsonb) -> 'item' ->> 'kind'      AS item_kind,
    content::jsonb                             AS envelope
FROM events
WHERE kind = 44225
  AND deleted_at IS NULL
  AND tags @> '[["cs-target", "coding-session/v1|…"]]'::jsonb
ORDER BY seq;
```

Gaps in `seq` are expected and are not corruption: the producer reserves a
sequence number before it publishes, so a crash loses one. Duplicates are the
thing that would be wrong.

### An umbrella session's founder (kind 44226)

Semantics: the operator-signed origin of one umbrella session. The signer is
the session's founder — the authority every later session operation (goal
edits, authority transitions, closure) resolves back to. Content is one of two
shapes: a fresh founding (`{"sessionRef", "v"}`) or an explicit legacy
adoption (`{"sessionRef", "v", "adopts": {"createEventId", "receiptEventId"}}`)
naming the pre-genesis `session.create` (44221) and its joining receipt
(44224) as evidence. Tags: exactly `h` and `csg-v`, plus `csg-session`
mirroring `sessionRef` for the relay's uniqueness probe and for diagnostics
only.

**Fold rule: there isn't one.** Canonical identity is this event's own **id**,
never the `csg-session` tag or the `sessionRef` label — a consumer must
resolve a founder by following an explicit genesis event id (reached from the
receipt-joined create that names it), not by selecting among rows that share a
`csg-session` value. The relay enforces at most one genesis per
`(channel, sessionRef)` at ingest (barring an explicit `adopts`), so more than
one row matching the query below is corruption to report, never an ambiguity
to resolve by taking the earliest or the newest:

```sql
SELECT id, pubkey, created_at, content::jsonb AS genesis
FROM events
WHERE kind = 44226
  AND deleted_at IS NULL
  AND tags @> '[["csg-session", "<sessionRef-uuid>"]]'::jsonb
ORDER BY created_at;
```

### An umbrella session's current goal and name (kinds 44227, 44229)

Semantics: human-authored revisions of the umbrella session's durable goal
(raw prose, ≤4096 bytes) and short navigation label (single line, ≤256
bytes). Both are regular append-only events that use a `d` tag for lookup
without opting into parameterized replacement, so every prior revision stays
queryable — there is no kind:5-style deletion of an old goal, only a newer
revision superseding it in the fold. Tags: goal is `h`, `d`, `csgl-v`; name is
`h`, `d`, `csnm-v`. `d` carries the session's `sessionRef` in both.

**Fold rule:** group by `(h, d)` and take the greatest `(created_at, event
id)` tuple. Nostr timestamps have one-second precision, so the
lexicographically greatest lowercase event id is the deterministic tie-break
within the same second — the same rule the transcript's `cst-seq` caution
above is protecting against, just on event id instead of a tag value.

```sql
-- current goal
SELECT content AS goal_text, created_at, id
FROM events
WHERE kind = 44227
  AND deleted_at IS NULL
  AND tags @> '[["d", "<sessionRef-uuid>"]]'::jsonb
ORDER BY created_at DESC, id DESC
LIMIT 1;

-- current name
SELECT content AS session_name, created_at, id
FROM events
WHERE kind = 44229
  AND deleted_at IS NULL
  AND tags @> '[["d", "<sessionRef-uuid>"]]'::jsonb
ORDER BY created_at DESC, id DESC
LIMIT 1;
```

A missing name falls back to the founding execution's title for display
purposes; that fallback is a consumer-side convention, not something this
query needs to know about.

### A session's authority chain (kind 44228, draft)

Semantics: one append-only link in a session's authority chain — who besides
the founder may steer it (`grant-operator`), who may merely read it
(`grant-viewer`), or the removal of either (`revoke`). Content is exactly five
fields: `genesisRef`, `prevAccepted` (nullable, never absent), `seq`
(1-based, incrementing by exactly one), `type`, `granteePubkey`. Tags:
exactly `h`, `csat-v`, `csat-genesis`. Chain linkage (`prevAccepted` must equal
the current accepted head; `seq` must be head + 1) and signer standing (must
be the session owner) are validated atomically with storage, so a row stored
under this kind already passed those checks — a competing transition that lost
the race was refused and never persisted.

**Fold rule:** reconstruct the live grant set by folding the relay's
acceptance receipts (kind 40099, `content.type =
"coding_session_authority_transition_accepted"`) in `seq` order — not the raw
44228s directly. `bee sessions roster` already does this; prefer it over
hand-rolled SQL unless you specifically need the unconfirmed chain:

```
bee sessions roster --channel <uuid> --genesis <genesis-event-id>
```

```sql
-- raw chain, ordered by seq (equivalent to what bee sessions roster folds)
SELECT
    (content::jsonb) ->> 'seq'           AS seq,
    (content::jsonb) ->> 'type'          AS transition_type,
    (content::jsonb) ->> 'granteePubkey' AS grantee,
    id                                   AS event_id
FROM events
WHERE kind = 44228
  AND deleted_at IS NULL
  AND tags @> '[["csat-genesis", "<genesis-event-id>"]]'::jsonb
ORDER BY ((content::jsonb) ->> 'seq')::int;
```

### An umbrella session's closure state (kind 44230)

Semantics: a shared, append-only `closed`/`open` revision of the umbrella,
explicitly rooted at its genesis event — not a command to a provider, and by
itself neither starts, stops, nor resurrects an execution. Content: strict
JSON with `action` (`closed` or `open`), `genesisRef`, `sessionRef`, `v`.
Tags: exactly `h`, `d` (= `sessionRef`), `cscl-v`, `cscl-genesis` (=
`genesisRef`). `closed` is accepted only from the genesis signer; `open` from
either the genesis signer or, inside a project session-transport channel, any
member of the project's current ACL.

**Fold rule:** group by `(h, d, cscl-genesis)` and take the greatest
`(created_at, event id)` tuple, same tie-break as goal/name above. With no
closure revision at all, the legacy default is `open`. Closure rows cannot be
deleted through NIP-09/NIP-29 moderation — only a newer signed revision moves
the fold, so an older state can never silently reappear because a newer fact
was erased:

```sql
SELECT (content::jsonb) ->> 'action' AS action, created_at, id
FROM events
WHERE kind = 44230
  AND deleted_at IS NULL
  AND tags @> '[["d", "<sessionRef-uuid>"], ["cscl-genesis", "<genesis-event-id>"]]'::jsonb
ORDER BY created_at DESC, id DESC
LIMIT 1;
```

### Project Pulse entries and their supersession chain (kind 44240 — pending the Slice 1 branch split)

Semantics: one explicit coordination claim (`plan`, `milestone`, `note`,
`handoff`, or `blocker`) an author makes about their own work on a project —
never an observed fact about a worktree, which is what the coding-session
kinds above are for. Content is strict JSON:
[`PulseEntry`](../crates/buzz-core/src/pulse.rs) — `schema`, `type`, `text`,
`codeAreas[]`, `branch`, `supersedes`. Tags are position-independent with a
closed key set (`crates/buzz-core/src/kind.rs:716-718`): exactly one `a` (the
canonical `30621:<owner>:<dtag>` project coordinate), `pu-v`, and `pu-type` —
which must equal the content's `type`
(`crates/buzz-core/src/pulse.rs:130`) — plus at most one each of `h` (transport
channel), `branch`, and `pu-session` (a cross-referenced umbrella session's
`sessionRef`).

**Fold rule:** deliberately not parameterized-replaceable — a revision is a
*new* entry naming its predecessor in `supersedes`, and the fold decides
whether that claim is honored. An entry is only ever superseded by a **later
entry from the same author**; the ordering is `(created_at, event id)`
descending, greater first (the same total order [NIP-CST](nips/NIP-CST.md)'s
`cst-seq` caution above exists to protect on the transcript side). Author
identity is settled by the event signature, never by a claimed field, so a
different signer naming your entry in `supersedes` never revises it — it is
recorded but not honored. The reference implementation of this fold is pinned
by `conformance/project-pulse-fold/` — three implementations across two
languages (Rust in `buzz-cli`, TypeScript in Desktop, Rust in `buzz-relay` for
Slice 2; `conformance/project-pulse-fold/CONTRACT.md:8-10`);
prefer it (or, once shipped, `bee pulse digest`) over reimplementing the fold
in SQL:

```sql
SELECT
    id                          AS event_id,
    pubkey,
    created_at,
    (content::jsonb) ->> 'type' AS entry_type,
    (content::jsonb) ->> 'text' AS text,
    (content::jsonb) ->> 'supersedes' AS supersedes
FROM events
WHERE kind = 44240
  AND deleted_at IS NULL
  AND tags @> '[["a", "30621:<owner-pubkey>:<dtag>"]]'::jsonb
ORDER BY created_at DESC, id DESC;
```

This raw dump is ordered to match the fold's precedence but does not itself
apply the same-author supersession rule — treat it as input to the fold, not
as the fold's answer, and remember that the project ACL gate described above
means a raw SQL scan can surface entries a given human reader is not entitled
to see in the app.

## Querying over HTTP instead

`POST /query` takes Nostr REQ filters, so the same reads work without database
access. One rule: **every filter must carry explicit `kinds`.** An open-ended
filter trips the relay's p-gate and comes back `403` — there is no "just fetch
the channel". Scope with `#h` for the channel and `#cs-target` for one
generation:

```json
{ "kinds": [44225], "#h": ["<channel-uuid>"], "#cs-target": ["coding-session/v1|…"] }
```

Results are capped per page; follow the relay's composite `(until, before_id)`
cursor for the rest, which is what `bee sessions` does internally.

## See also

- [NIP-CST](nips/NIP-CST.md) — the transcript-item contract these queries read.
- [NIP-CSG](nips/NIP-CSG.md) — genesis, goal, name, and closure revisions.
- [NIP-CSAT](nips/NIP-CSAT.md) — the authority transition chain (draft).
- `crates/buzz-core/src/pulse.rs` — Project Pulse entry validation (kind
  44240, pending the Slice 1 branch split).
- `conformance/project-pulse-fold/CONTRACT.md` — the Pulse supersession
  fold's single source of truth.
- `crates/buzz-cli/src/commands/sessions.rs` — the CLI implementation, including
  the generation-resolution rules it shares with the desktop consumer.
