NIP-AR
======

Project Artifact Pins
---------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-MP (the project a pin belongs to, and its roster), NIP-PK (the kind:30624 source that names a project's agents repository), NIP-AD (the path grammar a pin's target is drawn from). Interacts with NIP-09 (withdrawal of one's own pin op).

## Abstract

This NIP defines `kind:44251`, a **project artifact pin op**: one signed statement that a document, plan or folder of a project's agents repository belongs in every member's sidebar, and where in the order it sits. A pin is not an event anyone reads directly. What a reader gets is the fold of every op for the project under one pure rule set shared by every client (`conformance/project-artifact-pin-fold/CONTRACT.md`).

## Motivation

A project's written material lives in its agents repository (NIP-AD): plan artifacts under `plans/`, document artifacts under `docs/`. All of it sits behind a tab, which is the right place for forty files and the wrong place for the two or three a team reads every day. A to-do list has had `list.pinned` since NIP-TD — "show in every member's project sidebar" — and a document is at least as worth reaching for.

Why not another op on NIP-AD's kind:44250, which already carries everything else about these files? Because 44250's fold is a per-path **draft chain** that a `commit.record` closes. A pin is not a draft of a file's contents and must never be closed by a commit; threading an exception through that fold would complicate a contract three languages are already bound to, for an op that shares none of its shape. What the two kinds do share is the **gate**, and that is shared through one predicate (`buzz_core::kind::is_project_a_scoped_kind`), so none of the admission, withholding, fan-out, pushdown or request-shape rules is written twice.

Why not NIP-33: replacement keys are one head per *author*, and a pin is shared — two members pinning different documents would produce two independent heads and the reader would have to guess which is the sidebar. This is the hazard NIP-TD § Motivation records for to-do lists and kind:30624 met for the pack pin, and the answer is the same: an append-only kind folded client-side, one field per op.

## Event

`kind:44251` — regular, stored, append-only. Never replaceable.

Tags, position-independent, closed key set:

| tag | multiplicity | value |
|---|---|---|
| `a` | exactly one | canonical `30621:<lowercase-hex>:<dtag>` project coordinate |
| `ar-v` | exactly one | `ar1-1` |
| `ar-op` | exactly one | the op name, equal to the content `op` |
| `ar-repo` | exactly one | canonical `30617:<lowercase-hex>:<id>` — the repository the project's newest kind:30624 pins |
| `ar-target` | exactly one | the target, equal to the content's |

Any other key — **including `h`** — is a rejection. A pin belongs to a project, never to a room; the relay lists the kind as global-only.

Content is JSON, at most 2048 bytes, with `schema` = `buzz-project-artifact-pin/v1` and an **exact** key set per op. Nothing is nullable:

| `op` | keys |
|---|---|
| `pin.set` | `target`, `targetKind`, `pinned`, `rank` |
| `pin.rank` | `target`, `rank` |

- `targetKind` is `file` or `folder`.
- A **file** target is any path NIP-AD's grammar admits — a plan, a document, an asset, a role, the manifest — except a `document-folder` keep. The keep is how an empty directory exists in git; pinning it instead of the folder it holds open would put a row called `.gitkeep` in the sidebar.
- A **folder** target is `docs/<segment>/…` with one to seven segments, each a document name. It is deliberately **not** a path the grammar admits: git has no directory object, so a folder is named here by the prefix its files share. Seven and not eight so a file under the deepest pinnable folder still fits the path cap.
- `rank` is a fractional-indexing order key over the base-62 alphabet, the same one NIP-TD uses (`crates/buzz-core/src/fractional_rank.rs`, `conformance/project-todo-fold/fixtures/rank-vectors.json`). Both ops carry one: a `pin.set` establishes the order a target enters at, and a `pin.rank` moves it.

The single validator is `crates/buzz-core/src/project_artifact_pin.rs`; the relay, the SDK builder and `bee pins` call it and keep no copy.

## Relay behaviour

Admission, withholding, live fan-out, the SQL pushdown and the HTTP request-shape rule are the Pulse, to-do and draft rules, inherited through `buzz_core::kind::is_project_a_scoped_kind` (44240, 44248, 44250, 44251): a private project's owner or collaborator may write, a viewer may not (`OK false "restricted: …"`, **403** over `POST /events`, CLI exit 3); a stored op is withheld from a reader whose hidden-private-project set contains its coordinate; `{"kinds":[44251],"#a":[c]}` is accepted, an unscoped or mixed filter is `400`.

One check is this kind's own, made at ingest and answered as a rejection (**400**, CLI exit 2) because it is a fact about the event against the world, not about the author's authority:

**The pin names the project's agents repository.** `ar-repo` must equal the `repo` of the project's newest kind:30624. A project with no source is refused ("has no agents repository"); a pin for another repository is refused naming what the source pins. On a re-point, stored pins for the old repository stay stored; the fold reports them as `otherRepo`, never drops them silently — a pin names a path *in a repository*, and silently re-aiming one at a different repository would put a row in the sidebar that nobody wrote.

What the relay does **not** check, stated so nobody reads more into a pin than it says: that the target exists on `main`. It cannot without hydrating the repository on every pin, and a pin is written exactly when a document is new. A reader whose target has no file and no draft shows the row as missing rather than dropping it, so the person who pinned it can see what happened and unpin it.

**Deletion.** A NIP-09 `kind:5` from the op's author deletes the op (a withdrawal). The fold then never sees it; a target whose every `pin.set` is withdrawn leaves its `pin.rank` ops counted in `ranksWithoutPin` rather than conjuring a row.

**Storage.** The generic `events` table; the `a` tag is served by the JSONB tag index. No migration.

## Fold

Normative text is `conformance/project-artifact-pin-fold/CONTRACT.md`, pinned by `fixtures/fold-vectors.json`, which the Rust (`buzz-core`), TypeScript (Desktop) and Dart (Mobile) folds all bind to. In one paragraph: decode every op for the coordinate (a malformed op is `ignored`; a well-formed op for another repository is `otherRepo`); sort by `(created_at, id)`; a target exists when some `pin.set` names it, decided over the whole bag so arrival order never matters, and a `pin.rank` with no set is counted in `ranksWithoutPin`; per target per field the greatest key wins, with a `pin.set`'s own rank taking part under the set's key; rows sort by `(rank, target)` bytewise; `by` is the author of the winning `pin.set`.

An unpinned target stays in the digest with `pinned: false`, because the control that draws the pin needs to know the target was pinned once and is not now.

## Authority

Any writer the project admits may pin, unpin or reorder anything, including what someone else pinned. This is the intended shape of a shared sidebar, stated so nobody expects per-author ownership — the same note NIP-TD makes about a project list.

## Timestamps

The relay refuses any event whose `created_at` is more than 900 s from its clock. Writers stamp `created_at = max(now, latest seen op on the same target + 1)` so a write made after reading the current state wins the field even on a slightly slow clock; if that exceeds the window they surface a retry rather than silently dropping the bump. Live subscriptions use `since = now − 900` and dedupe by id.

## Known limitations

- **Cold read is the whole log.** A reader replays every op for the coordinate (relay page cap 1000). Clients paginate with `until` and disclose truncation rather than presenting a sidebar that silently lost a pin. Compaction is a follow-up, not part of this NIP.
- **A pin is shared.** There is no private sidebar arrangement: hiding the pinned rows is a per-device viewing preference that never reaches the wire, so one member cannot keep a pin to themselves.
- **A pin does not know whether its target exists** (above).
- **A folder pin is a prefix, not an object.** Renaming a folder does not move its pin, because a rename is one `file.move` per file under it and nothing in git says the directory moved. A client that renames a folder republishes the pin, and one that does not leaves a row pointing at a prefix with no files — which the reader shows as missing rather than hiding.
- **Purging a project leaves its ops readable** to community members, inherited from Pulse, NIP-TD and NIP-AD.

## Reference

- Kind and predicates: `crates/buzz-core/src/kind.rs` (`KIND_PROJECT_ARTIFACT_PIN_OP`, `is_project_a_scoped_kind`).
- Validator: `crates/buzz-core/src/project_artifact_pin.rs`. Fold: `crates/buzz-core/src/project_artifact_pin_fold.rs`. Ranks: `crates/buzz-core/src/fractional_rank.rs`.
- Relay: `crates/buzz-relay/src/handlers/ingest.rs`, `handlers/agents_repo_draft.rs` (`admit_pin_repository`), `handlers/req.rs`, `api/bridge.rs`; `crates/buzz-db/src/event.rs` pushdown.
- SDK: `crates/buzz-sdk/src/builders.rs` (`build_project_artifact_pin_op`, `build_delete_event`). CLI: `crates/buzz-cli/src/commands/pins.rs`.
- Folds: Desktop `desktop/src/features/agents-repo/lib/artifactPinFold.ts`, Mobile `mobile/lib/features/agents_repo/domain/artifact_pin_fold.dart`, both beside the draft modules whose path grammar a target is drawn from.
