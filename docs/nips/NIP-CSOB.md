# NIP-CSOB — Coding-session observations

`draft` `optional` `client` `relay`

`kind:44246` is one signed, append-only **observation** inside a coding
session: something its author saw while working. It carries four facts a person
watching an agent team needs and cannot get from the governance record — where a
seat is in its own loop, what its gates said, what it found and did about it,
and how long a phase took.

An observation **settles nothing**. It authorizes nothing, blocks nothing,
excludes nothing and corrects nothing. A mission's state is decided entirely by
kind 44244 (NIP-CSTX), and no reader of this kind may change that state.

The event signature is the author. Content MUST NOT carry `author`,
`authorPubkey`, `actor`, or another field that restates authorship.

This kind is additive. Clients that do not implement NIP-CSOB simply never query
kind 44246 and lose nothing else.

## Allocation

`44246` is the lowest unused and unreserved value available to this protocol.
The allocation scan checked both this fork and `vanilla/main`:

- `44231` is reserved for session checkpoints and `44232` for native snapshots.
- `44233` and `44234` are proposed for git transitions and checks.
- `44235` through `44239` are explicitly reserved as coding-session headroom.
- `44240` through `44243` are used or reserved by Project Pulse extensions.
- `44244` is the team transaction (NIP-CSTX) and `44245` the policy (NIP-CSP).
- `44246` matched nothing but lockfile hashes in either tree before this NIP.

This is a fork-local allocation, not a claim of global Nostr registry ownership.

### Why a new kind rather than four more 44244 operations

Three consequences of NIP-CSTX's own fold decided it, and each is checkable in
the code rather than a matter of taste.

1. **A 44244 envelope error is a whole-set hard error.** That kind's operation
   vocabulary is a closed serde enum with no `other` arm, and
   `fold_coding_session_team_transactions` returns `Err` when an envelope fails
   to validate. So any build predating a new token would read a session
   carrying one gate row as a **broken mission** — the same cliff finding 13
   cost, this time on the stream every seat writes many times an hour. A build
   that has never heard of 44246 simply never queries it.
2. **The governance fold is bounded for a handful of assignments.** Hundreds of
   observations inside it would evict the records mission state depends on.
3. **Observations carry no authority, supersession or causal reference.** The
   correction validator and the twelve exclusion codes buy nothing here, and
   they would let one observation's defect become a governance disclosure.

## Envelope

The event is a regular stored event. It has exactly five ordered, two-field
tags:

```json
[
  ["h", "<canonical channel UUID>"],
  ["d", "<canonical sessionRef UUID>"],
  ["csob-v", "buzz-coding-session-observation/v1"],
  ["csob-genesis", "<genesis event id, lowercase 64-hex>"],
  ["csob-type", "<closed observation token>"]
]
```

`d`, `csob-genesis` and `csob-type` MUST exactly equal the corresponding content
fields. Extra, missing, repeated, reordered, or non-two-field tags are invalid.
`h` and `d` use lowercase canonical hyphenated UUIDs. The event kind MUST be
44246.

### Regular, not replaceable

The `d` tag groups an umbrella so a consumer can query one session's
observations; it never opts this kind into NIP-16/NIP-33 replacement. A
replaceable observation would let a seat's newest gate row **erase** the failing
one a person was reading. Newest-wins exists here, but as a *fold* rule applied
per `(author, gate)` and per `(author, findingId)`, and the older event stays on
the wire where a reader can still find it.

## Content

Content is public JSON with exactly seven top-level keys, **every one always
present**:

```json
{
  "schema": "buzz-coding-session-observation/v1",
  "sessionRef": "<canonical UUID>",
  "genesisRef": "<genesis event id>",
  "type": "checkpoint" | "gate" | "finding" | "phase",
  "source": "observed" | "declared",
  "assignmentRef": "<assignment event id>" | null,
  "body": { }
}
```

`source` says **how the record came to exist**. It is a **required content
key**, so adding it was a breaking change to the schema: an event signed by a
build that predates it omits `source`, is refused by the decoder with
`coding-session observation payload is missing "source"`, and lands in the
fold's `ignored` list with that reason rather than being read. (Harmless in
practice — nothing had published a kind 44246 anywhere when the key was added
— but a wire spec should say so plainly.)

It is **not** carried in a tag. The five-tag envelope below is frozen and is
what relays index on; provenance is a property a reader filters the folded
collection by, not one it queries the relay for.

* `observed` — written by a mechanism watching the subject: the session
  provider deriving a gate row from a seat's own tool calls, or a hire host's
  git hook. The signer is that mechanism, never the subject.
* `declared` — written by the subject about its own work. A claim.

A reader **prefers observed rows, shows the word, and never merges the two**.
The fold's dedupe keys carry `source` for the same reason, so a claim can never
take the place of a measurement.

**`observed` is honoured only when the signer is verified.** The word is
self-asserted on the wire — anyone may sign it — so a consumer that ranks or
labels observed rows must check the signer against the session's own provider
instances. `fold_coding_session_observations` takes that set: given one, a row
claiming `observed` from a signer outside it is folded as `declared` and listed
under `misclaimedObserved`; given none, nothing is checked and the fold reports
`provenanceChecked: false`, because "not verified" and "verified good" are
different facts.

**Newest-wins is never silent.** A later statement replacing an earlier one for
the same `(author, source, gate)` is counted in `truncated.displacedGates` (and
`displacedFindings`), so a `failed` row replaced by a `passed` one cannot read
like a gate that had only ever passed. (Added 2026-09-02, after a seat's prose
"`cargo test -p buzz-cli` green" was reproduced red on the same patch by a
verifier — an agent's account of its own work is a claim, and only a record
produced without its cooperation is evidence.)

`assignmentRef` is a **pointer**, never a causal reference. An observation that
names an assignment nobody supplied is still a real statement its author made,
so a dangling id is disclosed as `unresolved` by the fold and excludes nothing.

There is **no `supersedes` key**. A later observation with the same
`(author, source, findingId)` or `(author, source, gate)` is simply the newer
statement.

**Absent is not null.** Every key above and every key in every body is always
present. An unset optional is written as JSON `null`; an omitted key is invalid,
and so is `null` in a position that requires a value. Unknown keys, at any
level, are **rejected rather than ignored** — a reader that ignored a key would
disagree with its peer about the same signed bytes.

### `checkpoint`

```json
{
  "phase": "planning" | "red" | "green" | "gates" | "reporting",
  "testsWritten": 4,
  "testsRed": 4,
  "testsGreen": 0,
  "lastCommand": "<= 512 B" | null,
  "lastSummary": "<= 2048 B" | null,
  "note": "<= 8192 B" | null
}
```

`testsRed` and `testsGreen` MUST NOT exceed `testsWritten`.

### `gate`

```json
{
  "rows": [
    {
      "gate": "<= 64 B",
      "outcome": "passed" | "failed" | "not-run",
      "command": "<= 512 B",
      "summary": "<= 2048 B" | null,
      "durationMs": 41000 | null,
      "headSha": "<40- or 64-hex git object id>" | null,
      "dirty": true | false | null
    }
  ]
}
```

One to 32 rows, **unique by `gate`**. `rows: []` is invalid: an observation with
nothing to say claims to state something and states nothing. The three outcome
words are deliberately the same three a 44244 `report.tests[].outcome` uses —
one word per outcome across the whole wire.

#### `headSha` and `dirty` — which commit the gate ran against (2026-09-03)

`headSha` is the commit the gate was measured over, and `dirty` says whether the
worktree matched it. Both are **required on write and optional on read**, the
same rule `source` is under (see *Read-optional keys* below).

* They travel **together or not at all**. A row carrying one without the other
  is refused by name: a commit named without saying whether the tree matched it
  is not evidence about that commit, and cleanliness with no commit says nothing
  a reader can use.
* `headSha` accepts a 40-hex (SHA-1) or 64-hex (SHA-256) object id — the same
  shape kind 44244's `report.headSha` carries, because the two are compared
  against each other.
* Both are `null` when nobody resolved a commit: a workdir that is not a
  repository, an unborn branch, a `git` that could not answer, or a declared row
  whose author named none. **Absent and `null` mean "this row names no
  commit"** and are never read as "the commit currently checked out".

For an `observed` row the producer is the session provider, which runs
`git rev-parse HEAD` and `git status --porcelain` **in the seat's own workdir at
the moment the gate closed**, in the provider process. The seat is never asked
and cannot sign the row. For a `declared` row the author states both itself
(`bee sessions observe gate --head-sha <sha>:clean|dirty`), and it is still a
claim.

**Why the key exists.** Without it the strongest thing a reader could say was
"this mission has green rows somewhere", which would let an earlier commit's
green stand for a later one. NIP-GS arm (B) — observed gates admitting a push —
is computed from this key and refuses every row that does not carry it.

### `finding`

```json
{
  "findingId": "<= 64 B",
  "title": "<= 512 B",
  "disposition": "found" | "fixed" | "cross-lane" | "needs-ruling" | "wont-fix",
  "detail": "<= 8192 B" | null,
  "refs": ["<event id>"],
  "decisionRef": "<decision.request event id>" | null
}
```

`findingId` is the author's own id and is unique **within that author** only.
`refs` holds at most 16 unique event ids and is a pointer list, never causal.

### `phase`

```json
{
  "phase": "<= 64 B",
  "startedAtMs": 1756800000000,
  "endedAtMs": 1756800413000 | null,
  "durationMs": 413000 | null
}
```

`endedAtMs` MUST NOT precede `startedAtMs`.

## Bounds and reference grammar

| Field | Bound |
|---|---|
| whole payload | 96 KiB |
| `command`, `lastCommand` | 512 B |
| `summary`, `lastSummary` | 2 KiB |
| `note`, `detail` | 8 KiB (newlines allowed; these are prose) |
| `gate`, `findingId`, `phase` name | 64 B |
| `title` | 512 B |
| `rows` | 1..=32, unique by `gate` |
| `headSha` | 40 or 64 lowercase hex; present exactly when `dirty` is |
| `refs` | 0..=16, unique |

Every event id is lowercase 64-hex. Every UUID is lowercase canonical
hyphenated. Single-line fields reject control characters; `note` and `detail`
accept `\n`, `\r` and `\t` and nothing else.

### Every time in this record is the author's own measurement

`startedAtMs`, `endedAtMs` and `durationMs` are **claims**, not facts a relay or
a reader can verify. They MUST be rendered as the author's own measurement and
MUST NOT be used for ordering, discovery or dedupe. "Newest" in this kind's fold
means *last in the order the caller supplied*, and nothing an author writes can
change where its record sits.

## Authority matrix

| Who | May publish |
|---|---|
| The umbrella's founder | any observation |
| Any active seat | any observation |
| Anyone else | any observation — it folds like everyone else's, listed under its own pubkey |

**Any active seat or the founder may observe; the relay checks structure only.**
This is the same division NIP-CSP and NIP-CSTX draw: the signature is the
author, and whether that author held a seat is the consumer's question against
the accepted NIP-CSAT chain.

**The fold has no seat model, and says so rather than implying one.**
`fold_coding_session_observations` carries no authority context — a stranger's
observation folds exactly like a seat's — so a consumer that renders one MUST
name its author and MUST NOT present it as a seat's word. What the fold does
guarantee is that the author is real: every event's signature is verified before
any pubkey is attributed an observation, and a failure is listed under
`ignored`.

Unlike NIP-CSP, a writer is **not** refused before signing for lacking standing,
because there is nothing an unseated observation could bind that a reader would
then have to un-believe: an observation binds nothing in the first place.

## Validation boundary and deterministic fold

The relay validates **structure**: schema, tags, closed vocabularies, bounds,
tag-to-content parity. It adjudicates nothing else.

Concretely, at ingest (`crates/buzz-relay/src/handlers/ingest.rs`): 44246 is a
coding-session kind, so it takes `Scope::MessagesWrite` and passes the strict
channel-membership gate **before** its content is parsed — a non-member is
refused for want of standing in the channel, not for anything its JSON says.
An admitted event is then handed to `buzz-core`'s own
`validate_coding_session_observation_envelope`, so the relay and every reader
refuse exactly the same bytes for exactly the same reason; there is no second
implementation to drift. It carries no separate storage cap, because the
decoder already bounds content at `MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES`
and a second bound could only disagree with the first.

What the relay does **not** decide is whether the signer held a seat. That is
the authority question, and it belongs to the consuming fold against the
accepted NIP-CSAT chain — exactly the division NIP-CSP draws for kinds 44244
and 44245. A relay that adjudicated standing at ingest would be asserting
something it cannot verify, and an observation refused that way would be a
statement its author really made, silently deleted.

`fold_coding_session_observations` **never fails**. There is no whole-set hard
error and no exclusion code in this kind. An event that is malformed,
cross-context, or not an observation at all is listed under `ignored` with a
reason and costs nothing but itself — that is the whole reason these four facts
are not 44244 subtypes.

It returns five collections plus two disclosures:

| Key | Contents |
|---|---|
| `checkpoints` | every checkpoint, in supplied order |
| `gates` | one row per `(author, source, gate)`, newest statement shown, the newest 16 event ids listed with `droppedEventIds` counting the rest |
| `findings` | one row per `(author, source, findingId)`, newest disposition shown, the newest 16 event ids listed with `droppedEventIds` counting the rest |
| `phases` | every phase timing, in supplied order |
| `unresolved` | observations whose `assignmentRef` resolved to nothing supplied |
| `ignored` | events the fold could not read, each with its reason |
| `truncated` | how many entries each collection dropped |

Checkpoints and phase timings are never deduped: each is a distinct moment its
author recorded, and collapsing them would delete the history the observer came
for.

**Every collection is bounded and reports what did not fit** — unknown is not
empty, and empty is not "nothing dropped". The five collections and `ignored`
each hold at most 512 entries, and a gate or finding entry lists at most **16**
event ids, keeping the newest (the newest statement is the one shown, so its
immediate neighbours are what a reader chasing it wants). `truncated` carries
one count per collection plus `entryEventIds`, the ids dropped from the front of
gate and finding entries.

## Implementation

| Layer | Path |
|---|---|
| Types, decoder, validator, envelope | `crates/buzz-core/src/coding_session_observation.rs` |
| Bounded fold | `crates/buzz-core/src/coding_session_observation_fold.rs` |
| Kind allocation and assertions | `crates/buzz-core/src/kind.rs` |
| Signed builder | `crates/buzz-sdk/src/coding_session_observation.rs` |
| Writer and reader (`bee sessions observe`, `bee sessions observations`) | `crates/buzz-cli/src/commands/sessions/observations.rs` |
| Relay structural validation | `crates/buzz-relay/src/handlers/ingest.rs` (scope, membership, envelope arm) |

**Residual, stated plainly:** ingest registration for kind 44246 landed with
this NIP, so the relay will store an observation — but **nothing here has been
written live yet.** Every reader and writer in this NIP is exercised by unit
tests and by the relay's own integration suite, not by a live publish against
a running community. Until a live write happens, treat the surfaces as proven
against the code and unproven against the wire.

## Reading observations without asking for them (batch 3, lane L9, 2026-09-02)

Kind 44246 is defined above as something an author *publishes*. This appendix
records the two producers that publish it **without anybody being asked to**,
because a vocabulary whose only producer is a cooperating agent inherits that
agent's memory.

1. **The hire host's git hooks.** At hire time the site creating a seat's
   worktree writes a `post-commit` and a `prepare-commit-msg` hook into **that
   worktree's own** `.git/hooks` and that worktree's **own** config (git's
   `--worktree` scope, which needs one `extensions.worktreeConfig` line in the
   enclosing repository's shared config and touches nothing else) — never the
   user's global config. The hook pushes `HEAD` to
   `refs/heads/wip/<role>/<assignment-hex8>` under the seat's own key. The agent
   only runs `git commit`. Hook text and config lines are pure strings in
   `buzz-core::seat_git_hooks`, so every installer writes the same bytes.

   **What it does not yet do.** The 44246 `checkpoint` body has no field for a
   commit SHA, a branch or a subject, and its `testsWritten`/`testsRed`/
   `testsGreen` are counts somebody measured. So the hook pushes the ref and
   publishes **no** checkpoint, logging that it did not: a checkpoint carrying
   invented counts, or a SHA smuggled into a prose field, would be exactly the
   claim this kind exists to replace. The provider's own seated-workdir path is
   a second site that creates seat worktrees and does not install these hooks
   yet.

2. **The provider's observed gate rows.** A `gate` row derived from a tool call
   the provider watched carries `source: "observed"` and is signed by the
   provider instance; a row its author states about its own work stays
   `declared`. The two classes are **never merged**: a consumer shows which it
   is holding, and an `observed` row that displaces a `declared` one says so.

### Two fields this appendix depends on

`gate.source` (`observed` | `declared`, closed, always present) and
`checkpoint.files` (≤ 64 paths, ≤ 256 B each) are **not defined by this lane**.
Until they land:

- every row reads `declared`, because the strict decoder refuses unknown keys
  and a row nothing observed is a claim; and
- no checkpoint names a path, so Project Pulse's overlap row is never computed —
  the honest failure, since paths nobody published are paths nobody may be told
  about.

Consumers must read `source` through one adapter that tolerates its absence
(`buzz_core::pulse_mission::PulseGateSource::from_wire_token`), and must not
promote an unknown token to `observed`.

### What a checkpoint may not carry

A checkpoint's `testsWritten` / `testsRed` / `testsGreen` are counts somebody
measured. A producer that has no measurement publishes **no checkpoint** rather
than a zero: a zero is a measurement, and an invented one is exactly the class
of claim this kind exists to replace.
