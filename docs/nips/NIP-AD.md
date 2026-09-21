NIP-AD
======

Agents-Repository Drafts
------------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-MP (the project a draft belongs to, and its roster), NIP-PK (the kind:30624 source that names a project's agents repository). Interacts with NIP-09 (withdrawal of one's own draft).

## Abstract

This NIP defines `kind:44249`, an **agents-repository draft op**: one signed proposal to change one file of a project's agents repository (`<slug>-beekeeper-agents`, spec § 4.11 — the only place a project's roles, plans, team manifest, actions and skills live), or one committer's record that named proposals landed in a commit on that repository's `main`. A draft is not a commit. Every member of the project can read, preview and build on the open drafts; nothing a draft says reaches a seat until someone commits it, and `main` of the agents repository stays the only thing a seat stages from.

## Motivation

The agents repository was created so that agents stop reading stale or unrelated plans in the code repository. What it lacked was a way for the people and agents on a project to **edit** it together: the Roles tab showed composed summaries, `plans/` was read nowhere, and the only pushes were the seed and the setup-candidate branch. Andy's chosen shape (2026-09-21) is that edits travel through the relay as shared drafts — "users can edit, save and preview changes via the relay; they're not real until someone hits the commit button".

Why whole-file drafts rather than a CRDT: a role or a plan is prose that people review and agents read from git; the concurrency answer that fits is "the newest save is the head, older saves stay visible as superseded, and a save from a stale head is refused before it is signed". Nothing anyone wrote disappears; only the claim that it is the head does. Why not NIP-33: replacement keys are one head per *author*, and a draft chain is shared (NIP-TD.md § Motivation). Why not a relay-side conditional write per save: the 30624 path's lock cost, for something edited far more often than a pin.

## Event

`kind:44249` — regular, stored, append-only. Never replaceable.

Tags, position-independent, closed key set:

| tag | multiplicity | value |
|---|---|---|
| `a` | exactly one | canonical `30621:<lowercase-hex>:<dtag>` project coordinate |
| `ad-v` | exactly one | `ad1-1` |
| `ad-op` | exactly one | the op name, equal to the content `op` |
| `ad-repo` | exactly one | canonical `30617:<lowercase-hex>:<id>` — the repository the project's newest kind:30624 pins |
| `ad-path` | one on `file.put` and `file.delete`, two on `file.move`, one per `paths` entry on `commit.record` | the paths the op names, as a set equal to the content's |

Any other key — **including `h`** — is a rejection. A draft op is never channel-scoped; the relay lists the kind as global-only.

Content is JSON, at most 65,536 bytes (the relay's advertised `max_content_len`), with `schema` = `buzz-agents-repo-draft/v1` and an **exact** key set per op (every listed key present, nullables as `null`, never absent):

| `op` | keys |
|---|---|
| `file.put` | `path`, `text`, `base`, `baseCommit`, `prev`, `message` |
| `file.move` | `path`, `to`, `base`, `baseCommit`, `prev`, `message` |
| `file.delete` | `path`, `base`, `baseCommit`, `prev`, `message` |
| `commit.record` | `commit`, `paths`, `drafts`, `message` |

- `text` is the whole new text of the file, at most 60,000 bytes, no control characters other than newline, carriage return and tab. Empty is legal.
- `base` is the blob sha (40 lowercase hex) of the file on `main` the author started from, or `null` for a file not there. A committer refuses a draft whose `base` is no longer the blob at its path: the author must reload and re-apply.
- `baseCommit` is advisory: the `main` commit the author read, or `null`.
- `prev` is the event id of the draft head the author edited from, or `null`. A client refuses to sign a save whose `prev` is not the current head (CLI exit 5).
- `message` is one line of at most 512 bytes, or `null`.
- `to` on a `file.move` is the only legal destination: `roles/<r>.md` ↔ `roles/archive/<r>.md`, `plans/<p>.md` ↔ `plans/archive/<p>.md`.
- `commit` is a 40-hex sha; `paths` and `drafts` are 1–256 unique entries.

**Paths** follow the agents repository layout: `README.md`, `team.yml`, `actions.yml` (put-only — never moved or deleted), `roles/<slug>.md`, `roles/archive/<slug>.md`, `roles/<slug>/skills/<skill>/<file…>`, `skills/<skill>/<file…>`, `plans/<slug>.md`, `plans/archive/<slug>.md`. Slugs are `[a-z0-9-]{1,64}` and never `archive`. No `..`, no empty or dot segments.

The single validator is `crates/buzz-core/src/agents_repo_draft.rs`; the relay, the SDK builder and `bee agents-repo` call it and keep no copy.

## Relay behaviour

Admission, withholding, live fan-out, the SQL pushdown and the HTTP request-shape rule are the Pulse and to-do rules, inherited through `buzz_core::kind::is_project_a_scoped_kind` (44240, 44248, 44249): a private project's owner or collaborator may write, a viewer may not (`OK false "restricted: …"`, **403** over `POST /events`, CLI exit 3); a stored op is withheld from a reader whose hidden-private-project set contains its coordinate; `{"kinds":[44249],"#a":[c]}` is accepted, an unscoped or mixed filter is `400`.

Two checks are this kind's own, made at ingest and answered as a rejection (**400**, CLI exit 2) because they are facts about the event against the world, not about the author's authority:

1. **The draft names the project's agents repository.** `ad-repo` must equal the `repo` of the project's newest kind:30624. A project with no source is refused ("has no agents repository"); a draft for another repository is refused naming what the source pins. On a re-point, stored drafts for the old repository stay stored; the fold reports them as `otherRepo`, never drops them silently.
2. **A `commit.record` names a commit on `main`.** The relay loads the repository's manifest chain (no pack is hydrated) and requires `commit` to be `refs/heads/main` now, or to have been its tip within the last 32 published states (`crates/buzz-relay/src/api/git/hydrate.rs` `commit_was_main_tip`). A record is therefore a relay-checked fact every reader may close drafts on, not a client's claim; a store failure is an internal error, never a silent accept.

What the relay does **not** check, stated so nobody reads more into a record than it says: that the named drafts' text is what landed in that commit. A committer who names drafts it did not apply closes them wrongly; nothing is deleted, the fold lists them under `commits[].drafts`, and the author re-opens by putting again with `prev` = the closed id.

**Deletion.** A NIP-09 `kind:5` from the op's author deletes the op (a withdrawal). The fold then never sees it.

**Storage.** The generic `events` table; the `a` tag is served by the JSONB tag index. No migration.

## Reading the tip without git

A client with no git (Mobile; `bee` before drafting) reads the repository's `main` through two routes beside the smart-HTTP ones, under the same NIP-98 repo-root token and the same read gate, answered with the same generic 404 on denial:

```text
GET /git/{owner}/{repo}/tree/{ref}[/{path}]  → {"commit", "path", "entries": [{"path","kind","oid","size"}]}
GET /git/{owner}/{repo}/raw/{ref}/{path}     → the blob's bytes; X-Git-Commit, X-Git-Blob headers
```

`ref` is `refs/heads/<branch>` or a 40-hex commit; `raw` serves at most 1 MiB (`413` beyond); both are `Cache-Control: no-store`. Each request hydrates the repository from the object store, as `info/refs` does.

## Fold

Normative text is `conformance/agents-repo-draft-fold/CONTRACT.md`, pinned by `fixtures/fold-vectors.json`, which the Rust (`buzz-core`), TypeScript (Desktop) and Dart (Mobile) folds all bind to. In one paragraph: decode every op for the coordinate (a malformed op is `ignored`; a well-formed op for another repository is `otherRepo`); sort by `(created_at, id)`; the union of every `commit.record`'s `drafts` is `closed`; per path, the open ops naming it (a move names both ends) — the greatest key is the `head`, the rest are `superseded`, oldest first; `diverged` is true when the head did not build on the newest superseded op; paths sort bytewise; records list newest first.

## Committing

A committer (`bee agents-repo commit`, the desktop's Commit button) fetches `main`'s tip `T`; refuses any chosen head whose `base` is not `T`'s blob at its path (`stale-base`, naming path and author) and any move whose destination exists; builds the tree from `T`'s index plus the heads; materializes it and validates it with `buzz_persona::agents_repo::validate_root` (manifest, live roles composed against the shipped templates, skills, `actions.yml`) — any refusal names its path and nothing is pushed; commits as the committer with `Co-authored-by:` per draft author, `Signed-off-by:` and a `Beekeeper-Drafts:` trailer; pushes `--force-with-lease=refs/heads/main:T`; verifies with `ls-remote` — a push whose verify failed is reported **unknown**, never "failed"; then publishes a `commit.record` naming the commit, the paths, and every open op on each landed path (head and superseded). A record that could not be published after a push that landed is disclosed with the repair verb `bee agents-repo commit-record <sha> --draft <id>…`.

## Timestamps

The relay refuses any event whose `created_at` is more than 900 s from its clock. Writers stamp `created_at = max(now, latest seen op on the same path + 1)` so a save made after reading the head is its successor even on a slightly slow clock. Live subscriptions use `since = now − 900` and dedupe by id.

## Known limitations

- **Cold read is the whole log**, and every save is a whole-file copy (relay page cap 1000). Clients disclose truncation. Compaction is a follow-up, not part of this NIP.
- **A file over the cap cannot be drafted.** `team.yml` and a persona body may legally be 256 KiB; the tip routes still serve them, and they are edited with git.
- **The `prev` head check is client-side.** Two saves from the same `prev` both store; the fold keeps both and reports `diverged`. Relay-side enforcement is a follow-up if a real conflict shows up.
- **A record does not prove its drafts' text landed** (above).
- **Purging a project leaves its ops readable** to community members, inherited from Pulse and NIP-TD.

## Reference

- Kind and predicates: `crates/buzz-core/src/kind.rs` (`KIND_AGENTS_REPO_DRAFT_OP`, `is_project_a_scoped_kind`).
- Validator: `crates/buzz-core/src/agents_repo_draft.rs`. Fold: `crates/buzz-core/src/agents_repo_draft_fold.rs`. Tree validation: `crates/buzz-persona/src/agents_repo.rs`.
- Relay: `crates/buzz-relay/src/handlers/agents_repo_draft.rs`, `handlers/ingest.rs`, `api/git/hydrate.rs` (`commit_was_main_tip`), `api/git/read_routes.rs`.
- SDK: `crates/buzz-sdk/src/builders.rs` (`build_agents_repo_draft_op`, `build_delete_event`). CLI: `crates/buzz-cli/src/commands/agents_repo.rs`, `agents_repo_git.rs`.
- End-to-end: `crates/buzz-test-client/tests/e2e_agents_repo_drafts.rs`.
