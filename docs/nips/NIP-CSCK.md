# NIP-CSCK — Coding-session turn checkpoints

`draft` `optional` `client` `relay`

`kind:44231` is one provider-signed, append-only **turn checkpoint**: what the
working tree of one exact coding-session generation was when a turn ended,
which range of that generation's transcript the turn covers, and which files
the turn changed. When the tree could not be read, the checkpoint is still
published and says why.

It is the commit↔session link on the relay. A commit finds the session and
turn that produced it through `git.head`; a session finds its commits through
the same field. And it is the real list of changed files: a turn that only ran
`sed -i` or a code generator through a shell shows its files here, where a fold
over the transcript's edit-tool calls cannot see them.

This kind is additive. A client that does not implement NIP-CSCK never queries
kind 44231 and loses nothing else.

## Allocation

`44231` was reserved for a session checkpoint by the session-continuity
research, which shaped it with `coverage {fromSeq, throughSeq}` and a context
summary. This NIP claims the reservation for its intended number: `coverage` is
kept identical, and `summary` is held at `null` in v1 so the context use is one
field away rather than a second kind. `44232` (native snapshot), `44233`/`44234`
(git transition and check) and `44235`–`44239` stay reserved. `git grep 44231`
was re-run on 2026-10-04 before the constant existed; it matched only the
reservation notes and an unrelated signature hex in `NIP-IA.md`.

This is a fork-local allocation, not a claim of global Nostr registry ownership.

### Why this is not a 44246 observation

NIP-CSOB's `checkpoint` observation is a different thing with the same word. It
is a TDD-phase report — declared by a seat or observed about one — folded
newest-wins per author, and it carries no commit SHA. A turn checkpoint is a
fact the host measured about one exact generation: it is keyed by that
generation's own transcript seq exactly as a 44225 item is, it is signed by the
generation's provider rather than by a seat, and a later `session.rewind`
names it by event id. Folding it into 44246 would put a measured tree under an
author-newest fold, and the reasons NIP-CSOB gives for not being a 44244
subtype (a closed vocabulary that fails a whole set on one unknown token, a
fold bounded for a few records) apply to it here as well.

## Signer

The event MUST be signed by the key that signs the same generation's
`kind:44225` transcript items (NIP-CST). A reader MUST discard a checkpoint
whose signer is not that key. The relay validates structure only; it does not
know which key a provider uses.

## Envelope

A regular stored event, channel-scoped. Exactly five ordered two-field tags,
mirroring NIP-CST:

1. `h`: canonical lowercase channel UUID
2. `csck-v`: `csck1-1`
3. `cs-target`: the NIP-CSC structured exact-target key
   (`coding-session/v1|<driver><instanceId><sessionId><generation>`,
   length-prefixed), byte-identical to the 44225 items of the same generation
4. `csck-seq`: decimal `coverage.throughSeq`
5. `csck-key`:
   `coding-session-checkpoint/v1|<driver><instanceId><sessionId><generation><throughSeq><reason>`,
   each field prefixed with its UTF-8 byte length and a colon, exactly as
   `cst-key` is; `<reason>` is the wire token `turn` or `pre_rewind`

Every tag value is re-derived from the content, and a mismatch is refused. A
second checkpoint with the same `csck-key` is a duplicate, not a revision.

The reason is part of the identity because a `pre_rewind` capture normally
ends at the same seq as the last turn's checkpoint (see `pre_rewind`
groundwork). Without it the two would share a key, and a reader would drop the
`pre_rewind` as a duplicate, and with it the only way to undo the rewind.
Example: turn 3 ends at seq 58, so its key ends `2:584:turn`; the rewind's
capture also covers through seq 58, and its key ends `2:5810:pre_rewind`.

## Content

At most 32 KiB of strict JSON:

```jsonc
{
  "schema": "buzz-coding-session-checkpoint/v1",
  "session": { "driver": "claude-agent-acp", "instanceId": "…", "sessionId": "…", "generation": 2 },
  "turnId": "…",                       // null only when reason is pre_rewind
  "reason": "turn",                    // "turn" | "pre_rewind"
  "coverage": { "fromSeq": 41, "throughSeq": 58 },
  "git": {                             // null exactly when unavailable is set
    "head": "<oid>",                   // or null on an unborn branch
    "branch": "main",                  // short name, or null when detached
    "baseTree": "<tree oid>",          // or null when the baseline capture timed out
    "tree": "<tree oid>",
    "commit": "<commit oid>",
    "outsideTurn": false,              // true | false | null
    "complete": false,
    "omitted": [ { "path": "big.bin", "reason": "too_large" } ],
    "omittedNotListed": 0              // omissions not named in omitted
  },
  "files": [
    { "path": "src/main.rs", "status": "modified", "from": null, "additions": 3, "deletions": 1 }
  ],
  "filesNotListed": 0,
  "restorable": false,
  "unavailable": null,                 // or { "code": "NOT_A_REPOSITORY", "sentence": "…" }
  "summary": null
}
```

### Rules

- **Unknown keys are rejected, not ignored**, at every level: the payload,
  `session`, `coverage`, `git`, each `git.omitted` entry, each `files` entry,
  and `unavailable`. A reader that ignored a key would disagree with its peer
  about the same signed bytes.
- **Absent is not null** (the NIP-CSOB rule). Every key is always present. An
  unset optional is written as JSON `null`, and `null` where a value is
  required is refused by name. The nullable keys are `turnId`, `git`,
  `unavailable`, `summary`; `git.head`, `git.branch`, `git.baseTree`,
  `git.outsideTurn`; and `files[].from`, `files[].additions`,
  `files[].deletions`.
- **Exactly one of `git` and `unavailable` is non-null.** A checkpoint with
  `unavailable` set lists no files and has `filesNotListed` 0: nothing was
  measured.
- `session` identifiers and `turnId` are non-blank single-line strings of at
  most 512 UTF-8 bytes, as in NIP-CST. `generation` is a positive safe integer.
- `turnId` is `null` only when `reason` is `pre_rewind`. A `turn` checkpoint
  always names its turn.
- `coverage.fromSeq` and `coverage.throughSeq` are positive safe integers
  (at most 2^53 − 1) in the generation's 44225 `eventSeq` clock, with
  `fromSeq ≤ throughSeq`. For a `turn` checkpoint the range runs from the
  turn's `user_prompt` item through its terminal result item.
- `git.tree` and `git.commit` are required; `git.head` and `git.baseTree` are
  nullable. Every object id is a lowercase 40-hex (SHA-1) or 64-hex (SHA-256)
  git object id — the shape kinds 44244 and 44246 already write — and all of a
  checkpoint's object ids have the same width.
- `git.branch` is a short branch name, never a ref name (`refs/…`).
- `git.outsideTurn` is `true` when the previous checkpoint's `tree` differs
  from this `baseTree` — files changed between turns, by a person or another
  process — `false` when they are equal, and `null` when there is no previous
  checkpoint or no `baseTree` to compare.
- `git.omitted` names at most 32 paths left out of the captured tree, each
  `too_large` or `unreadable`, each at most once. Omissions beyond 32, and
  omitted paths this NIP refuses to publish, are counted in
  `git.omittedNotListed` (a non-negative safe integer, never `null`) rather
  than dropped silently, so `omitted.length + omittedNotListed` is the true
  number of paths left out. When `omitted` names any path or
  `omittedNotListed` is above 0, `git.complete` MUST be `false`. A producer
  also writes `false` with nothing omitted when it cannot vouch that `tree` is
  this turn's end alone — for example the session's next turn was prompted
  before this capture finished, so `tree` and `files` may include that turn's
  edits. A reader MUST NOT present a `complete: false` checkpoint as a complete
  measurement, whether or not it names an omission.
- `files` lists at most 256 changes from `baseTree` to `tree`, each path at
  most once, with `status` `added`, `modified`, `deleted` or `renamed`. `from`
  is non-null exactly when `status` is `renamed`, and differs from `path`.
  `additions` and `deletions` are non-negative integers, or `null` when git
  cannot count lines (a binary file) — never a guessed zero. Changes beyond
  256, and changes whose path this NIP refuses, are counted in
  `filesNotListed` rather than dropped silently.
- When `git.baseTree` is `null` there is nothing to compare `tree` against, so
  `files` MUST be `[]` and `filesNotListed` 0. That empty list means "not
  known", never "nothing changed": a reader MUST say the baseline was not
  captured (for example "Baseline not captured") and MUST NOT render it as
  "0 files changed".
- `restorable` is `true` exactly when the provider build implements
  `session.rewind` (SV-29) and the checkpoint is a `turn` checkpoint; a
  `pre_rewind` checkpoint is never restorable. It says the **conversation**
  can be rewound to this turn, whatever the `git` facts are: a turn with no
  `git`, or with a `null` `baseTree`, can still be rewound chat only
  (`files: keep`). Whether the **files** can be restored is read from
  `git.baseTree` alone — a reader MUST NOT infer it from `restorable`, and a
  provider refuses `files: restore` without one (`NOT_RESTORABLE`).
  Checkpoints published before rewind was built stay `false`, and so do
  those an earlier rewind-capable build published without a `baseTree` (it
  then tied `restorable` to the baseline); a provider refuses every rewind
  to a `false` one. It is per-checkpoint truth: a reader offers a rewind
  only from a checkpoint whose provider said it can perform one.
- `unavailable.code` is one of `NOT_A_REPOSITORY`, `BOUNDARY_UNPREPARED`,
  `TIMED_OUT`, `GIT_FAILED`. `unavailable.sentence` is one line of at most 512
  UTF-8 bytes naming no host path.
- `summary` MUST be `null` in v1. It is reserved for the continuity research's
  context use.

### Paths

Every path in `files[].path`, `files[].from` and `git.omitted[].path` is
**repo-relative, in canonical form**. A producer refuses to encode, and a
reader refuses to decode, a path that:

- is absolute (`/…`, `\…`, or a drive letter such as `C:`);
- has a `..`, `.` or empty segment (`/` and `\` both separate segments);
- contains a NUL or any other control character;
- is a ref name (starts with `refs/`);
- carries a redaction marker (`[elided private context:`, `••••••••`,
  `[redacted`) — a redacted path is not published, because doing so would
  either leak around the redaction or publish the marker as if it were a file;
- is empty or longer than 1024 UTF-8 bytes.

No absolute path, ref name or worktree name travels in this kind.

## Host-local refs: only SHAs travel

The checkpoint commit (`git.commit`) is written with git plumbing — a temporary
index, `write-tree`, `commit-tree` with `HEAD` as parent, `update-ref` — that
never touches the real index or `HEAD` and runs no hooks. It is kept alive by
host-local refs, one leaf per identity:

- `refs/beekeeper/checkpoints/<sessionId>/<generation>/<throughSeq>` — the
  turn's end;
- `refs/beekeeper/checkpoints/<sessionId>/<generation>/base-<firstSeq>` — the
  baseline captured before the turn's prompt was delivered, where `firstSeq`
  is the turn's `user_prompt` seq;
- `refs/beekeeper/checkpoints/<sessionId>/<generation>/pre-rewind-<throughSeq>-<command>`
  — a `pre_rewind` capture, where `<command>` is the lowercase 64-hex event id
  of the `session.rewind` lifecycle command that took it. It has its own leaf
  because its `throughSeq` is normally the last turn's: writing it to
  `<throughSeq>` would move that turn's ref and leave the turn's commit
  unreferenced, free for `git gc` to collect. The command id makes the leaf
  unique per rewind attempt, so no later capture can move a pre-rewind ref
  even if it reused a `throughSeq` (which the `pre_rewind` rule below
  forbids). A command id that is not 64 hex digits is refused before git
  runs.

The session id is used as one ref segment only when it is safe to be one:
ASCII letters, digits, `-`, `_` and `.`, with no `..`, no leading or trailing
`.` and no `.lock` suffix. An id that is not safe is refused rather than
encoded, so it can never name another ref. That capture is published with
`unavailable.code` `GIT_FAILED` and a sentence saying the id cannot name a
ref. T3 Code base64url-encodes the thread id instead; Beekeeper's session ids
are already UUID-shaped, so refusing the rare unsafe one is simpler and stays
visible.

**These refs are never published and never pushed.** A capture includes
untracked files, which may be uncommitted secrets; only the object ids travel.
A reader on another machine therefore has paths and counts, and a real diff
only where the commits it names already reached the relay's git. The refs share
the repository's ref namespace rather than a seat worktree's, so they outlive a
worktree. Retention in v1 is partial, and this is the whole of it:

- pruning a seat worktree deletes the refs of the session recorded on that
  worktree;
- refs of a seat worktree whose record names no session stay after the
  worktree is pruned, because the pruner will not guess whose they were;
- deleting a project retires the refs of every session the deleting host
  recorded under that project, open or closed, from each repository those
  sessions ran in (SV-55, `crates/beekeeper-session-provider/src/project_deletion.rs`);
  it cannot reach a session whose working directory is already gone, a
  session another machine ran, or a deletion observed while the project had
  no open execution;
- refs of a session that ran in the project's own checkout, rather than in a
  seat worktree, are retired only by that project's deletion; nothing else
  retires them in v1.

A separate namespace (`refs/beekeeper/…`) is what lets
them coexist with other tools' checkpoint refs (`refs/t3/…`, `refs/entire/…`)
in one clone.

## The commit↔session link

`git.head` is the commit `HEAD` named when the turn ended. A reader answering
"which session produced commit X" finds the checkpoints whose `git.head` is X;
`bee sessions checkpoints --commit <sha>` answers it on the command line. No
commit trailer is written and no commit is amended: amending after the fact
churns the SHAs a turn just reported.

## `pre_rewind` groundwork

A rewind (SV-29, a `kind:44221` `session.rewind` lifecycle action) restores
files to a checkpoint's `baseTree` and restarts the agent from the record. So
that every rewind is itself undoable, the provider captures and publishes a
checkpoint with `reason: "pre_rewind"` **before** it touches any file. A
`pre_rewind` checkpoint may have `turnId: null`, because no turn is open when
it is taken, and its `coverage` ends at the last transcript seq the rewound
generation wrote. That is normally the last turn's `throughSeq`; the two stay
distinct because the reason is part of `csck-key` (see Envelope) and because
the capture's ref leaf is `pre-rewind-<throughSeq>-<command>`.

**A `pre_rewind` capture's `throughSeq` must exceed that of every earlier
`pre_rewind` of the same generation**, because `csck-key` names the reason and
`throughSeq` but not the rewind command. A provider meets this by writing a
failed rewind into the reopened generation's transcript before it accepts
another rewind, so a second attempt's capture covers a later seq. A provider
that cannot advance the seq publishes no second `pre_rewind` checkpoint: a
reader would drop it as a duplicate of the first. The first capture is the
one that matters to keep, since it holds the files before any attempt touched
them; the command-keyed ref leaf keeps the second attempt's commit alive on
the host regardless. "Undo rewind" is then a rewind to that checkpoint, and
needs no new wire.

A `pre_rewind` checkpoint's `coverage.fromSeq` is the rewound checkpoint's
`fromSeq` and its `git.baseTree` is that checkpoint's `baseTree`, so its
`files` list what a `restore` undoes. It is queued at high priority before
any file is written; with `files: restore` a failed capture refuses the
rewind (`CHECKPOINT_UNAVAILABLE`) and nothing is touched, while a chat-only
rewind still publishes it with `unavailable` set. A provider that would
repeat a published `pre_rewind` key publishes none (see above).

A checkpoint from a provider that does not implement `session.rewind`
carries `restorable: false`, and a reader shows the rewind control disabled
with the reason rather than hiding it.

## Reader guidance

- Keep one `turn` checkpoint per `(cs-target, turnId)`; the highest
  `throughSeq` wins. Keep `pre_rewind` checkpoints separately.
- With `git` set, a turn's changed files come from `files`, labelled as coming
  from git. With `unavailable` set, say why: "No checkpoint · not a git
  repository". With `outsideTurn: true`, say that files also changed outside a
  turn. With `complete: false`, say how many files were not captured:
  `omitted.length + omittedNotListed`, never `omitted.length` alone — and
  when that is 0, say the capture is incomplete anyway.
- Never render an empty diff for a checkpoint whose objects this machine does
  not hold; say which machine holds them.
