# Project Pulse — the digest fold (conformance corpus)

**Source of truth:** `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md`
§5.4 (client fold) and §6 (relay fold). Decision 19 of that plan makes this
directory the fold's single source of truth: **a fold rule that exists in only
one language is a defect.**

The fold is implemented at least three times — Rust in `buzz-cli`
(`buzz pulse digest`, Slice 1), TypeScript in Desktop (Slice 1), and Rust in
`buzz-relay` (kind 39011, Slice 2). All three bind to
`fixtures/fold-vectors.json` in this directory and must produce the digest in
`expected` for the events and clock in `input`.

Nothing here is a rule about *rendering*, except where a rendering rule is
called out explicitly. The fold produces facts; surfaces render them.

---

## What the fold consumes

Per project, the caller supplies:

- every readable kind:44240 Pulse entry for the project coordinate;
- every readable coding-session fact reachable through the project's channels —
  kind 44223 (metadata), 44227 (goal), 44229 (name), 44230 (closure);
- `now`, a Unix-seconds clock read once, after the last source query returned;
- `sourceErrors`, one `{scope, message}` per source query that failed or was
  truncated.

The caller's session reach is **`sessionsScope: "project channels"`** and
nothing wider: a community-wide 44223 scan is forbidden (it is unbounded and it
leaks), and 44223 carries no `a` tag, so `#a` does nothing for sessions. A
session running in a channel outside the project's channel set is not
discoverable in v1, and no surface may present the Active-work list as
exhaustive.

---

## The envelope

The fold emits **exactly** the kind-39011 content object of §6, plus `source`,
with these keys in this order:

```json
{
  "schema": "buzz-project-pulse-digest/v1",
  "source": "client-composed",
  "project": "30621:<owner>:<dtag>",
  "asOf": 1785513037,
  "complete": true,
  "sessionsScope": "project channels",
  "sessions": [],
  "entries": [],
  "errors": []
}
```

- `source` is `"client-composed"` when the caller folded it and
  `"relay-digest"` when kind 39011 produced it. Slice 2 changes who computes the
  digest, never its shape. Every vector in this corpus is `"client-composed"`;
  a relay-side fold binding to these vectors substitutes `"relay-digest"` and
  changes nothing else.
- `asOf` is mandatory: the wall-clock second the last source query returned.
- `complete` is false whenever any source query failed or was truncated by
  `limit`. **`complete:false` plus a non-empty `errors[]` is the only
  representation of a partial read.** A partial fold must never print as a
  complete digest with an empty session list — that is the "read error renders
  as an empty project" failure arriving through a side door. The caller exits 2
  on `complete:false`; a confirmed-empty project is `complete:true` and exit 0.

Member shapes, with every key always present (`null`, not omitted, when there
is no value) so the two languages serialize byte-identically:

```text
entries[]      = { eventId, pubkey, createdAt, type, text, claimedAreas[],
                   branch|null, sessionRef|null, supersedes|null,
                   supersededBy[], active }
sessions[]     = { targetKey, sessionRef|null, name|null, goal|null,
                   status, statusAt, closed, activity, branch|null,
                   observedCommit|null, dirty|null, relayReachable|null,
                   verifiedAt|null, commitConfirmation, observedAgeSeconds,
                   sourceEventIds[] }
errors[]       = { scope, message }
supersededBy[] = { eventId, pubkey|null, honored, reason|null }
```

**Ordering** (byte-identity requires it, and the plan leaves it open):

- `entries[]`: `createdAt` descending, ties broken by the **greater** event id
  first — the same total order the supersession law uses, so "newer" means one
  thing in this file.
- `sessions[]`: `statusAt` descending, ties broken by `targetKey` ascending.
- `supersededBy[]`: `eventId` ascending. This is a set of claims, not a
  recency ordering.
- `errors[]`: `scope` ascending, then `message` ascending.
- `claimedAreas[]` and `sourceEventIds[]`: `sourceEventIds[]` ascending;
  `claimedAreas[]` keeps the author's first-seen order (the entry decoder
  already rejected duplicates).

---

## Entries

An event is an entry when it is kind 44240, carries this project's coordinate,
and passes `buzz_core::pulse::validate_pulse_entry_envelope` (Rust) or its
TypeScript twin.

- `claimedAreas` are **claims**, never observed facts, and every surface must
  say so. They are the decoded `codeAreas` — a single leading `./` already
  stripped by the decoder.
- `sessionRef` is the `pu-session` tag, echoed verbatim. It is author-controlled
  and unverified at ingest; see *Entry-to-session attribution*.
- An event that fails validation is **excluded** from `entries[]` and recorded
  in `errors[]` as
  `{"scope": "invalid-entry", "message": "entry <id> failed validation and was excluded"}`.
  It is never dropped silently and never counted as a valid claim. Ingest
  rejects these shapes, so only a smuggled or legacy event reaches a reader.
  An invalid entry is not a failed read: `complete` stays true.

---

## The supersession fold law (plan §5.4, verbatim in effect)

Supersession is a **single-pass marking, never a traversal**, so cycles are
structurally impossible and no naive traversal can hang or blank the active set.

Entry `E` is marked superseded iff some entry `S` in the same
(community, project) result set satisfies **all** of:

- `S.supersedes == E.id`;
- `S.pubkey == E.pubkey`;
- `S.created_at >= E.created_at`, ties on `created_at` broken by the greater
  event id — matching the established `(created_at, event id)` fold at
  `crates/buzz-core/src/coding_session_closure.rs:148-156`;
- `S.id != E.id`.

`E.active` is false exactly when at least one honored claim names it.

Consequences, all of which are honesty requirements and not preferences:

- **A cross-author `supersedes` never removes its target.** The target stays
  active and both entries render; the superseding entry carries
  `supersededBy: [{eventId: <target>, pubkey: <target author>, honored: false,
  reason: "cross-author"}]`, and Desktop renders
  `supersession claimed by <author>`. Without this rule, any project writer
  could publish a one-line 44240 superseding a peer's `blocker` and silently
  push it out of the active set that drives wait|consult|proceed — one agent
  erasing another agent's claim.
- **A reference to an id not in the result set** leaves the target unknown and
  `E` active; the digest echoes `supersedes` verbatim with
  `supersededBy: [{eventId: <target>, pubkey: null, honored: false,
  reason: "unresolved"}]`, Desktop renders `supersedes <id> (not visible)`, and
  `errors[]` records
  `{"scope": "unresolved-supersedes", "message": "entry <S.id> supersedes <target>, which is not in the visible result set"}`.
  A reference to a non-44240 or different-project event is treated identically.
  An unresolved reference is not a failed read: `complete` stays true.
- **A same-author claim that loses the ordering** (`S.created_at <
  E.created_at`, or a `created_at` tie lost on event id) is not honored and
  carries `reason: "out-of-order"`. The order is total, so two entries can
  never supersede each other.
- **Superseded entries are never dropped.** They are returned with
  `active: false` and their `supersededBy[]` populated, and remain available in
  Desktop through progressive disclosure.

`reason` is `null` on an honored claim and one of `"cross-author"`,
`"unresolved"`, `"out-of-order"` otherwise. `pubkey` is the other entry's
author, or `null` when that entry is not in the result set.

---

## Sessions

Sessions are keyed by `targetKey` — the `cs-target` tag value,
`coding-session/v1|<len>:<driver><len>:<instance><len>:<session><len>:<generation>`
(`buzz_core::coding_session_command::coding_session_target_key`). Only 44223
events whose content `projectRef`, normalized through
`normalize_project_coordinate`, equals the requested coordinate participate.

- The winning 44223 for a target is the newest by `(created_at, event id)`.
  `status` is its content `status`; `statusAt` is its `created_at`.
- `sessionRef` is the winning 44223's `sessionRef` echo — the umbrella UUID,
  **not** a genesis event id. `name`, `goal`, and `closed` are joined on it:
  the newest 44229, the newest 44227, and the newest 44230 by
  `(created_at, event id)`, each `null`/`false` when absent.
- `sourceEventIds[]` lists every event folded into the row — the winning 44223
  plus each winning goal, name, and closure — ascending.
- `observedAgeSeconds` = `now − statusAt`, rendered `observed <age> ago`. It is
  **never** derived from `verifiedAt`, which is null whenever the reachability
  check did not complete and would otherwise show "age: unknown" for a session
  with perfectly fresh 44223 observations.

### Active work — the definition

The absence of a closure is not evidence of life. A machine that dies mid-turn
leaves the last 44223 saying `running` forever, and a session whose provider
identity is gone can never publish a closure. Answering "who is actively
working on this project?" with that ghost would tell a new worker to **wait**
on it indefinitely.

A session's `activity` is `"active"` only on a positive freshness signal. All
three must hold:

- (a) the latest 44230 fold is not `closed`;
- (b) the newest 44223 status is one of
  `starting | running | idle | waiting_for_input`;
- (c) `statusAt` is within `PULSE_ACTIVE_WINDOW = 30 minutes` of `now`, i.e.
  `now − statusAt <= 1800`. Exactly 1800 is still active; 1801 is not.

Anything failing (b) or (c) is `activity: "stale"`, renders in a separate
**Last seen** group labelled `<status> · last observed <age> ago`, and **must
never contribute a `wait` to the wait|consult|proceed advisory**.
`disconnected` and `failed` render as `Disconnected` / `Needs attention` per
`codingSessionWireWorkspaceStatus`, never as Active work. A session failing (a)
is also `"stale"`; it carries `closed: true` and renders under a closed
grouping — closed history is disclosed, never deleted, and never presented as
somebody currently working.

Implementations must assert that their own `PULSE_ACTIVE_WINDOW` equals this
file's `activeWindowSeconds` (1800). The constant lives in code; the corpus
pins it.

### Session status precedence

The session status is one derivation, not two: a signed lifecycle status
outranks the transcript and `statusAt` decides freshness, exactly as
`deriveCodingSessionWorkspaceStatus`
(`desktop/src/features/coding-sessions/lib/codingSessionWorkspaceModel.ts:193-230`)
does. A fixture test asserts the CLI and Desktop produce identical status for
the same event set.

### Observation fields — unknown is not false

`observedCommit`, `dirty`, `relayReachable`, and `verifiedAt` are emitted
exactly as nullable observations. **Never convert `null` into `false`.**
`relayReachable` is null exactly when `verifiedAt` is null.

`commitConfirmation` is a fixed tri-state string that every surface emits, so
they cannot drift:

| `relayReachable` | `commitConfirmation`         |
|---|---|
| `true`  | `Commit confirmed on relay` |
| `false` | `Commit not found on relay` |
| `null`  | `Commit not checked`        |

**Rendering rule, not a fold rule:** surfaces append `· <verifiedAt age> ago`
to the first two, and nothing to the third. The age is *rendered*, never folded
into the digest — a humanized age baked into a JSON field is stale the moment
it is written, and Slice 2's relay-signed 39011 would then carry a sentence
that ages while the digest sits in a client cache. `verifiedAt` is in the
envelope; the age is computed from it at paint time.

**Never render the words "relay reachable" or "relay unreachable."** The
underlying fact is "the relay's advertised refs contained this exact commit at
`verifiedAt`" — it says nothing about whether the session is connected.

### Branch

44223's `branch` is `Option<String>`. A null branch is its own group: it is
never merged into a named branch and never rendered as one. `--branch <name>`
selects rows whose branch equals `<name>` exactly, case-sensitive; the reserved
`--branch -` selects only null-branch rows; omitting `--branch` returns
everything. Desktop's "no branch" chip maps to `--branch -`.

---

## Entry-to-session attribution

The `pu-session` tag is author-controlled and unverified at ingest. An entry is
displayed *inside* a session card only when its author is that session's founder
(the 44226 genesis pubkey) or holds a 44228 authority grant for it. Any other
entry naming a `pu-session` renders at project level as
`references session <name>`, attributed to its own author, and never inside the
session's card or its status line. Otherwise any project writer could publish a
`blocker` carrying another team's `sessionRef` and have it render on that team's
card.

This is a **consumer law with no representation in the v1 envelope**: the fixed
member shapes carry neither a founder pubkey on `sessions[]` nor an attribution
flag on `entries[]`, so a surface must resolve 44226/44228 itself before
placing an entry inside a card. The vectors therefore do not pin it, and it is
the one law in this document that the corpus cannot enforce. Treat any change
to the envelope that would let it be pinned as an improvement, not a break.

---

## The vector file

`fixtures/fold-vectors.json`:

```text
{
  schema:              "buzz-project-pulse-fold-vectors/v1",
  contract:            path to this file,
  activeWindowSeconds: 1800,          // PULSE_ACTIVE_WINDOW, pinned
  digestSchema:        "buzz-project-pulse-digest/v1",
  entrySchema:         "buzz-pulse-entry/v1",
  vectors: [{
    name:        unique slug,
    description: the law this vector pins, in prose,
    input: {
      project:      the project coordinate the fold was asked for,
      now:          injected clock, Unix seconds,
      sourceErrors: [{scope, message}] the caller's failed/truncated queries,
      events:       [ signature-stripped Nostr events ]
    },
    expected:      the complete digest object
  }]
}
```

Events are signature-stripped JSON objects — `id`, `pubkey`, `created_at`,
`kind`, `tags`, `content` — the same shape the CLI reads back from `POST /query`
and the same shape Desktop's bridge yields. **Ids are synthetic** and are not
hashes of the event: these are fold inputs, not signature or id-verification
fixtures. Every id is 64 lowercase hex characters so the `(created_at, id)`
ordering is exercised honestly, and every 44240 and 44223 content in the corpus
decodes with the real `buzz_core::pulse::decode_pulse_entry` and
`buzz_core::coding_session_payload::decode_coding_session_metadata` — except the
one deliberately invalid entry in `self-referencing-entry-excluded`.

Binders:

- `implementation.test.mjs` in this directory binds the Desktop fold module,
  following the node-only binder pattern of `conformance/transcript-export/`
  and run by `just conformance-check`.
- A `#[test]` in `crates/buzz-cli` loads the same JSON with `include_str!` and
  asserts byte-identical serialized output.
- Slice 2's kind-39011 fold binds to the same vectors, substituting
  `source: "relay-digest"`.

### What each vector pins

| Vector | Law |
|---|---|
| `empty-project` | A quiet project is a complete digest, not a partial read. |
| `same-author-supersession-chain` | Single-pass marking down a chain; only the newest stays active. |
| `cross-author-supersession-not-honored` | A peer's claim never removes your entry from the active set. |
| `unresolved-supersedes` | A dangling reference is echoed and recorded, never resolved away, and never a failed read. |
| `created-at-tie-greater-id-honored` | The one-second-precision tiebreak, honored direction. |
| `created-at-tie-lesser-id-not-honored` | The same tiebreak, refused direction — the order is total. |
| `two-entries-supersede-one-target` | Marking is per-claim; the target is returned once with both claims. |
| `self-referencing-entry-excluded` | An invalid entry is excluded *and* reported, never silently dropped. |
| `session-null-observations-preserved` | Unknown is not false; `observedAgeSeconds` comes from 44223, not `verifiedAt`. |
| `closed-session-is-not-active-work` | Clause (a): a closed umbrella is not active even with a fresh running status. |
| `freshness-window-and-commit-confirmation` | Clauses (b) and (c) at the exact 1800-second boundary, the orphaned execution, and all three `commitConfirmation` strings. |
| `no-branch-rows-preserved` | A null branch is its own group in both entries and sessions. |
| `partial-read-is-never-an-empty-digest` | `complete:false` + non-empty `errors[]` is the only partial read. |
