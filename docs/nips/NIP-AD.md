NIP-AD
======

Agents-Repository Drafts
------------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-MP (the project a draft belongs to, and its roster), NIP-PK (the kind:30624 source that names a project's agents repository). Interacts with NIP-09 (withdrawal of one's own draft).

## Abstract

This NIP defines `kind:44250`, an **agents-repository draft op**: one signed proposal to change one file of a project's agents repository (`<slug>-beekeeper-agents`, spec § 4.11 — the only place a project's roles, plans, team manifest, actions and skills live), or one committer's record that named proposals landed in a commit on that repository's `main`. A draft is not a commit. Every member of the project can read, preview and build on the open drafts; nothing a draft says reaches a seat until someone commits it, and `main` of the agents repository stays the only thing a seat stages from.

## Motivation

The agents repository was created so that agents stop reading stale or unrelated plans in the code repository. What it lacked was a way for the people and agents on a project to **edit** it together: the Roles tab showed composed summaries, `plans/` was read nowhere, and the only pushes were the seed and the setup-candidate branch. Andy's chosen shape (2026-09-21) is that edits travel through the relay as shared drafts — "users can edit, save and preview changes via the relay; they're not real until someone hits the commit button".

Why whole-file drafts rather than a CRDT: a role or a plan is prose that people review and agents read from git; the concurrency answer that fits is "the newest save is the head, older saves stay visible as superseded, and a save from a stale head is refused before it is signed". Nothing anyone wrote disappears; only the claim that it is the head does. Why not NIP-33: replacement keys are one head per *author*, and a draft chain is shared (NIP-TD.md § Motivation). Why not a relay-side conditional write per save: the 30624 path's lock cost, for something edited far more often than a pin.

## Event

`kind:44250` — regular, stored, append-only. Never replaceable.

Tags, position-independent, closed key set:

| tag | multiplicity | value |
|---|---|---|
| `a` | exactly one | canonical `30621:<lowercase-hex>:<dtag>` project coordinate |
| `ad-v` | exactly one | `ad1-1` |
| `ad-op` | exactly one | the op name, equal to the content `op` |
| `ad-repo` | exactly one | canonical `30617:<lowercase-hex>:<id>` — the repository the project's newest kind:30624 pins |
| `ad-path` | one on `file.put`, `file.delete` and `asset.put`, two on `file.move`, one per `paths` entry on `commit.record` | the paths the op names, as a set equal to the content's |

Any other key — **including `h`** — is a rejection. A draft op is never channel-scoped; the relay lists the kind as global-only.

Content is JSON, at most 65,536 bytes (the relay's advertised `max_content_len`), with `schema` = `buzz-agents-repo-draft/v1` and an **exact** key set per op (every listed key present, nullables as `null`, never absent):

| `op` | keys |
|---|---|
| `file.put` | `path`, `text`, `base`, `baseCommit`, `prev`, `message` |
| `file.move` | `path`, `to`, `base`, `baseCommit`, `prev`, `message` |
| `file.delete` | `path`, `base`, `baseCommit`, `prev`, `message` |
| `asset.put` | `path`, `sha256`, `mime`, `size`, `base`, `baseCommit`, `prev`, `message` |
| `commit.record` | `commit`, `paths`, `drafts`, `message` |

- `text` is the whole new text of the file, at most 60,000 bytes, no control characters other than newline, carriage return and tab. Empty is legal.
- `base` is the blob sha (40 lowercase hex) of the file on `main` the author started from, or `null` for a file not there. A committer refuses a draft whose `base` is no longer the blob at its path: the author must reload and re-apply.
- `baseCommit` is advisory: the `main` commit the author read, or `null`.
- `prev` is the event id of the draft head the author edited from, or `null`. A client refuses to sign a save whose `prev` is not the current head (CLI exit 5).
- `message` is one line of at most 512 bytes, or `null`.
- `to` on a `file.move` depends on the class, and the rule is not uniform. A **role** or **plan** has exactly one destination, its archive counterpart: `roles/<r>.md` ↔ `roles/archive/<r>.md`, `plans/<p>.md` ↔ `plans/archive/<p>.md`. A plan is never renamed — every adopted `planRef` (NIP-PW) names it by path, and a rename would orphan them. A **document**, **asset** or **folder keep** may move to any path of its own class, which is what rename and move-between-folders are; Markdown and HTML are one class, so changing a document's format is a move. A root file is put-only and a skill file has no move. Renaming a folder is one op per file under it: the grammar admits each one, it does not make a directory move atomic. The single rule is `validate_move_destination`, pinned by `conformance/agents-repo-draft-path/` (`moves`).
- `sha256`, `mime` and `size` on an `asset.put` describe the media blob holding the image's bytes. A draft op is UTF-8 text only — base64 inside the 60,000-byte text cap would ceiling an image at about 45 KB and bloat the log — so the bytes go to the relay's media store, which already validates magic bytes, the MIME allowlist and pixel dimensions and dedupes by sha256, and the committer fetches the blob and writes it into the tree. The image is therefore versioned in git like the document beside it. `mime` must be exactly the MIME the path's extension names (`DOCUMENT_ASSET_MIMES`), so a blob is never served as something its path does not say; `size` is 1..=100 MiB, with the media store's own configured cap the real gate. `.svg` has no entry: the media store refuses `image/svg+xml` as active web content, so an SVG in the tree is committed with git and the refusal says so by name.
- `commit` is a 40-hex sha; `paths` and `drafts` are 1–256 unique entries.

**Paths** follow the agents repository layout: `README.md`, `team.yml`, `actions.yml` (put-only — never moved or deleted), `roles/<slug>.md`, `roles/archive/<slug>.md`, `roles/<slug>/skills/<skill>/<file…>`, `skills/<skill>/<file…>`, `plans/<slug>.md`, `plans/archive/<slug>.md`, and the **documents tree** below. Slugs are `[a-z0-9-]{1,64}` and never `archive`; a plan's stem is a document name (`is_doc_stem`). No `..`, no empty or dot segments.

**The documents tree** is the one part of the layout with folders, because a project needs somewhere to keep writing that is not a `beekeeper-plan/v1` work plan — an ordinary document, a mockup, a diagram:

| shape | class |
|---|---|
| `docs/<folder>/…/<stem>.md`, `…/<stem>.html` | `document` |
| `docs/<folder>/…/<file>.(png\|jpg\|jpeg\|gif\|webp\|svg)` | `document-asset` |
| `docs/<folder>/…/.gitkeep` | `document-folder` |

`plans/`, `roles/` and `skills/` stay flat: a plan's path is cited by every adopted `planRef` and a role's stem is a `team.yml` key, while a document is cited by nothing and so may be organised. Folders nest up to **eight components** under `docs/` (the last of them the file), inside the 512-byte path cap, and a folder name follows the document-stem rule — at most 96 bytes of `[A-Za-z0-9._-]`, never leading with `.` or `-`. Extensions are lowercase, so one path names one file. `archive` is an ordinary folder name here: the documents tree has no archive rule, and a document is moved or deleted. `.gitkeep` is admitted **only** under `docs/`, and it is *draftable* there — git has no empty directories, so a folder someone created and has not filled yet exists only as its keep; without that, creating or pinning an empty folder would be something a client claims and the next commit drops.

The single validator is `crates/buzz-core/src/agents_repo_draft.rs`; the relay, the SDK builder and `bee agents-repo` call it and keep no copy. The path grammar has its own cross-language corpus, `conformance/agents-repo-draft-path/`.

## Relay behaviour

Admission, withholding, live fan-out, the SQL pushdown and the HTTP request-shape rule are the Pulse and to-do rules, inherited through `buzz_core::kind::is_project_a_scoped_kind` (44240, 44248, 44250, 44251): a private project's owner or collaborator may write, a viewer may not (`OK false "restricted: …"`, **403** over `POST /events`, CLI exit 3); a stored op is withheld from a reader whose hidden-private-project set contains its coordinate; `{"kinds":[44250],"#a":[c]}` is accepted, an unscoped or mixed filter is `400`.

Three checks are this kind's own, made at ingest and answered as a rejection (**400**, CLI exit 2) because they are facts about the event against the world, not about the author's authority:

1. **The draft names the project's agents repository.** `ad-repo` must equal the `repo` of the project's newest kind:30624. A project with no source is refused ("has no agents repository"); a draft for another repository is refused naming what the source pins. On a re-point, stored drafts for the old repository stay stored; the fold reports them as `otherRepo`, never drops them silently.
2. **An `asset.put` names a blob this community holds.** The relay reads the community-scoped sidecar — the tenant read gate for otherwise shared content-addressed bytes — and requires the blob to be there with exactly the `mime` and `size` the op claims. Reading the raw object instead would let a draft in one community name a blob only another ever uploaded. Three client claims therefore become relay-checked facts, and a reader may show the image on the strength of the op alone.
3. **A `commit.record` names a commit on `main`.** The relay loads the repository's manifest chain (no pack is hydrated) and requires `commit` to be `refs/heads/main` now, or to have been its tip within the last 32 published states (`crates/buzz-relay/src/api/git/hydrate.rs` `commit_was_main_tip`). A record is therefore a relay-checked fact every reader may close drafts on, not a client's claim; a store failure is an internal error, never a silent accept.

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

The digest is `buzz-agents-repo-draft-digest/v2` — **v2** added `sha256`, `mime` and `size` to a row for `asset.put`; a v1 digest is the same document without them. Normative text is `conformance/agents-repo-draft-fold/CONTRACT.md`, pinned by `fixtures/fold-vectors.json`, which the Rust (`buzz-core`), TypeScript (Desktop) and Dart (Mobile) folds all bind to. In one paragraph: decode every op for the coordinate (a malformed op is `ignored`; a well-formed op for another repository is `otherRepo`); sort by `(created_at, id)`; the union of every `commit.record`'s `drafts` is `closed`; per path, the open ops naming it (a move names both ends) — the greatest key is the `head`, the rest are `superseded`, oldest first; `diverged` is true when the head did not build on the newest superseded op; paths sort bytewise; records list newest first.

## Committing

A committer (`bee agents-repo commit`, the desktop's Commit button) fetches `main`'s tip `T`; refuses any chosen head whose `base` is not `T`'s blob at its path (`stale-base`, naming path and author) and any move whose destination exists; builds the tree from `T`'s index plus the heads; materializes it and validates it with `buzz_persona::agents_repo::validate_root` (manifest, live roles composed against the shipped templates, skills, `actions.yml`) — any refusal names its path and nothing is pushed; commits as the committer with `Co-authored-by:` per draft author, `Signed-off-by:` and a `Beekeeper-Drafts:` trailer; pushes `--force-with-lease=refs/heads/main:T`; verifies with `ls-remote` — a push whose verify failed is reported **unknown**, never "failed"; then publishes a `commit.record` naming the commit, the paths, and every open op on each landed path (head and superseded). A record that could not be published after a push that landed is disclosed with the repair verb `bee agents-repo commit-record <sha> --draft <id>…`.

## Timestamps

The relay refuses any event whose `created_at` is more than 900 s from its clock. Writers stamp `created_at = max(now, latest seen op on the same path + 1)` so a save made after reading the head is its successor even on a slightly slow clock. Live subscriptions use `since = now − 900` and dedupe by id.

## Known limitations

- **Cold read is the whole log**, and every save is a whole-file copy (relay page cap 1000). Clients disclose truncation. Compaction is a follow-up, not part of this NIP.
- **An image's bytes do not travel in the op.** They go to the media store and the op names the blob, so a draft an agent can read is not a draft it can render without fetching that blob, and a purge of the blob before the commit leaves the asset op naming nothing. The committer's fetch is what turns it into a file.
- **A folder move is not atomic.** Renaming a folder is one `file.move` per file under it; a client that stops halfway leaves the rest where they were, visibly, in the digest.
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
