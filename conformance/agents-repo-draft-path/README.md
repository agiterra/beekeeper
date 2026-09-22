# Agents-repository draft path grammar

Which paths an agents repository's layout admits, and what each one is. Three
strict readers answer this question, in three languages:

| reader | file |
| --- | --- |
| `buzz-core` (used by `bee agents-repo`) | `crates/buzz-core/src/agents_repo_draft.rs` — `validate_draft_path` |
| Desktop | `desktop/src/features/agents-repo/lib/agentsRepoDraftOp.ts` — `draftPathClass` |
| Mobile | `mobile/lib/features/agents_repo/domain/agents_repo_draft_op.dart` — `draftPathClass` |

Three readers, so the record needs vectors (`../README.md`). A rule implemented
in only one of them is a defect, and nothing in CI would have caught it before
this corpus existed: each reader's tests used its own table.

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
