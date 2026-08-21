# Analyzing recorded coding sessions

Coding sessions are stored, not streamed-and-forgotten. Every turn, every tool
call, every result is a signed event the relay already persists, which makes the
relay's own database the analysis database — no export pipeline, no warehouse,
no second copy to keep coherent.

This page is how to ask it questions: the CLI for the common ones, direct SQL
for everything else.

## Where the transcripts live

All six coding-session kinds land in the relay's Postgres `events` table
(`migrations/0001_initial_schema.sql`), partitioned by `created_at`:

| Kind | What it is | Contract |
| --- | --- | --- |
| 44220 | Turn command (operator → provider) | [NIP-CSC](nips/NIP-CSC.md) |
| 44221 | Lifecycle create command | [NIP-CSL](nips/NIP-CSL.md) |
| 44222 | Provider catalog | [NIP-CSPC](nips/NIP-CSPC.md) |
| 44223 | Per-generation metadata | [NIP-CSL](nips/NIP-CSL.md) |
| 44224 | Lifecycle receipt | [NIP-CSL](nips/NIP-CSL.md) |
| 44225 | Transcript item | [NIP-CST](nips/NIP-CST.md) |

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
psql session against the relay as a privileged operation.

## The CLI surface

`buzz sessions` covers the questions that come up most, and resolves generations
the same way the desktop app does (a receipt confirms a generation exists; the
newest metadata wins, with a same-second burst broken on event id). Use it when
you want the CLI and the app to agree.

```
buzz sessions list       --channel <uuid>
buzz sessions transcript --channel <uuid> --target <cs-target> | --session <sessionId>
                         [--format md|jsonl]
buzz sessions tools      --channel <uuid> [--target <cs-target>]
buzz sessions export     --channel <uuid> --out <dir>
```

- **`list`** — one row per generation: target key, title, status, model, and
  created-at. `--format` is the global flag (`buzz --format compact sessions
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

Exit codes are the usual ones: `0` ok, `1` bad input or not found, `2`
relay/network, `3` auth, `4` other.

## Direct SQL

Four starting points. Add `AND community_id = '<uuid>'` to every one of them in
a multi-tenant deployment, and `AND channel_id = '<uuid>'` to scope to a single
channel.

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

Take the `cs-target` key from `buzz sessions list`. **Order by the numeric
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
cursor for the rest, which is what `buzz sessions` does internally.

## See also

- [NIP-CST](nips/NIP-CST.md) — the transcript-item contract these queries read.
- `crates/buzz-cli/src/commands/sessions.rs` — the CLI implementation, including
  the generation-resolution rules it shares with the desktop consumer.
