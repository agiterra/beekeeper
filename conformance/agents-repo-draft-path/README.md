# Agents-repository draft path grammar

Which paths an agents repository's layout admits, and what each one is. Three
strict readers answer this question, in three languages:

| reader | file |
| --- | --- |
| `buzz-core` (used by `bee agents-repo`) | `crates/beekeeper-core/src/agents_repo_draft.rs` — `validate_draft_path` |
| Desktop | `desktop/src/features/agents-repo/lib/agentsRepoDraftOp.ts` — `draftPathClass` |
| Mobile | `mobile/lib/features/agents_repo/domain/agents_repo_draft_op.dart` — `draftPathClass` |

Three readers, so the record needs vectors (`../README.md`). A rule implemented
in only one of them is a defect, and nothing in CI would have caught it before
this corpus existed: each reader's tests used its own table.

Two questions, one corpus: `cases` says what class a path is (or that it is
refused), and `moves` says whether a `file.move` from one path to another is a
legal destination. Both are answered by the same three readers.

The grammar is not only about what a draft may *write*. The Desktop Files tab
runs every path it lists through the same function to decide what it is, and
`read_tip` refuses to open anything the grammar rejects
(`desktop/src-tauri/src/managed_agents/agents_repo_read.rs`). A path this corpus
refuses is a file a person cannot open in the app, however ordinary it looks in
git.

## A role stem is a slug; a plan stem is a document name

They are deliberately different, and the difference is the point of this corpus.

A **role** file's stem is the key the team manifest uses for that role, so it
must be a slug — lowercase, digits and `-`. `roles/Lead.md` is refused because
`Lead` cannot be a `team.yml` key. The same goes for a skill directory, which
is named by role frontmatter.

A **plan** file's stem is the name of a document. It carries uppercase, `_` and
interior dots, because the documents that move into an agents repository are
called `CURRENT_STATE.md`, `SESSION_STATE.md` and `README.md`. Requiring a slug
there was a carry-over from the role rule with no reason behind it, and it cost
something real: when Beekeeper's own map, ledger and in-force plans moved into
`bee-keeper-beekeeper-agents` on 2026-09-22, every one of them landed at a path
the Files tab lists as `other` and refuses to open. Size was never the problem;
casing was. TankLoop's move the same day used lowercase-kebab throughout and
opened fine, which is how the divergence was found.

A plan stem is still bounded: at most 96 bytes, never starting with `.` or `-`,
and never `archive` in any case, which names the sibling directory.

## The documents tree

`docs/` is the one tree with folders. `plans/`, `roles/` and `skills/` keep
their flat, fixed-depth shapes, because a plan's path is cited by every adopted
`planRef` and a role's stem is a `team.yml` key. A document is cited by nothing,
so it may be organised.

| shape | class |
| --- | --- |
| `docs/<folder>/…/<stem>.md`, `…/<stem>.html` | `document` |
| `docs/<folder>/…/<file>.(png\|jpg\|jpeg\|gif\|webp\|svg)` | `document-asset` |
| `docs/<folder>/…/.gitkeep` | `document-folder` |

- Folders nest up to **eight components** under `docs/` (the last of which is
  the file), inside the existing 512-byte path cap. A folder segment follows the
  same rule as a document stem — at most 96 bytes, no leading `.` or `-` — so
  the tree cannot hide files or mint something an argument parser reads as a
  flag.
- Extensions are **lowercase**, so one path names one file.
- `archive` is an ordinary folder name here. The documents tree has no archive
  rule: a document is moved or deleted, and `moves` says so.
- `.gitkeep` is admitted **only** under `docs/`, and only by that exact name.
  Git has no empty directories, so a folder someone created and has not filled
  yet exists only as its keep; without that, "new folder" and "pin a folder"
  would be things the product claims and git drops at the next commit.
- `svg` is admitted as an asset although `buzz-media` refuses `image/svg+xml`
  as an upload MIME (`crates/beekeeper-media/src/validation.rs`, the active-web-content
  block). In the tree an asset is only ever rendered through `<img>`, which runs
  no script, or inside the preview window, which has its own CSP and no network.
  The two decisions are about different surfaces and neither is loosened here.

## Destinations: what a `file.move` may name

`moves` pins the destination rule, which is **not** the same for every class:

- a **role** or **plan** has exactly one destination, its archive counterpart.
  A plan is never renamed — every adopted `planRef` names it by path, and a
  rename would orphan them.
- a **document**, **asset** or **folder keep** may move to any path of its own
  class, which is what rename and move-between-folders are. Markdown and HTML
  are one class, so changing a document's format is a move.
- a **root file** is put-only, and a skill file has no move at all.

Renaming a folder is one move per file under it, issued by the client. The
grammar admits each one; it does not make a directory move atomic.

## Adding a rule

Follow `../README.md`: add the vector first, run all three readers' tests and
watch each one fail, then change all three in the same landing. Never loosen a
reader to make a vector pass.

## A refusal that is recorded, not endorsed

`model-registry.yaml` is pinned as **refused**, and that is a known gap rather
than a decision. The product's own seed writes that file at the repository root
and the seeded `README.md` tells a project to edit its rows, but no reader
admits it as a root file, so the Files tab cannot open it. The vector states
what the readers do today. Whoever decides the file should be editable adds it
to `ROOT_FILES` in all three readers and flips this vector in the same landing.
