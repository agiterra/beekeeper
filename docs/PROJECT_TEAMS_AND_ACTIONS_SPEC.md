# Project teams, composable roles and project actions — design spec

2026-09-16. Direction: Andy, with Brian to review. Drafted by Fable from a
read-only survey of `main` at `92cc7b888`. Product authority:
`VISION_COLLABORATION.md` § "Roles evolve with the project" and
§ "Deterministic operations first"; `VISION_PROJECTS.md` § "CI and Workflows";
`docs/PROJECT_TEAM_SETUP_IMPL.md` decisions 1–8; `docs/CREW_SESSIONS_PLAN.md`
D11–D17. This file defines the work; it is not a claim that anything below is
built. Status belongs in `CURRENT_STATE.md`; findings go to `SESSION_STATE.md`
(item 141 records the survey this spec rests on).

## 0. Purpose, in three sentences

A project's team is defined in the project's own repository, as composable
role prompts that can include versioned templates Beekeeper ships, and a
manifest naming the roles and the advisory shape of the agents that fill them.
Beekeeper ships those templates in-tree, every supported version present at
once, so an app update never silently rewrites a project's team. A project
also declares actions — scripts the relay triggers on a schedule, on a git
push, on a CI result or on a click — that the operator's own machine executes
after approval and whose output is routed to an agent by a rule the project
wrote.

## 1. Decisions taken 2026-09-16 (Andy)

Each was a question with alternatives; the chosen answer is quoted.

1. **Where base templates live.** "We keep them all within the app, same repo,
   all supported templates remain in the current commit. We add new template
   versions ONLY IF the template changes between release versions of the app.
   Otherwise users can use `@latest`. Old ones may be flagged as deprecated,
   future users can always clone the files into their own projects where
   necessary." Rejected: a relay-hosted base packs repo; a GitHub repo.
2. **Where a project's roles live.** "In the project repo." Rejected: keep the
   sibling packs repo as the only form. Sibling packs repos keep working (§ 4.8).
3. **Where actions run.** "Relay triggers, host executes — for now user should
   approve all triggered actions unless they allow future executions with a
   checkbox." Rejected: a desktop-local scheduler; running scripts on the relay.
4. **Deliverable.** This spec; no product code in the drafting session.

## 2. What exists today, and what does not

Verified against `main` `92cc7b888` on 2026-09-16. The spec builds on the first
list and must not duplicate it; the second list is the gap it fills.

**Exists.**

- A role pack is `.plugin/plugin.json` + `personas/<name>.persona.md` +
  `skills/<skill>/SKILL.md` (`crates/buzz-persona/PERSONA_PACK_SPEC.md`;
  `PackManifest` `crates/buzz-persona/src/manifest.rs:79`; `PersonaConfig`
  `crates/buzz-persona/src/persona.rs:123`, frontmatter rejects unknown keys at
  `:204`; body cap `MAX_BODY_BYTES` `:24`). Unclaimed skills are shared to every
  persona (`crates/buzz-persona/src/pack.rs:254`). `pack_instructions` is
  resolved (`crates/buzz-persona/src/resolve.rs:222`) and consumed by no seat.
- Eight shipped packs under `personas/roles/`, bundled as a Tauri resource
  (`desktop/src-tauri/tauri.conf.json:65`), pinned on the wire as
  `packRef.repo = app:shipped`, `sha` = app version
  (`desktop/src-tauri/src/managed_agents/packs_cache.rs:334`).
- A project names its pack source with one signed kind 30624: `repo`, exactly
  one of `ref`/`sha`, optional `path` defaulting to `personas/roles`
  (`crates/buzz-core/src/project_pack_source.rs`; write rule
  `crates/buzz-core/src/kind.rs:958-964`; `docs/nips/NIP-PK.md`).
- The seat staging ladder `plan_seat_pack`
  (`desktop/src-tauri/src/managed_agents/actor_seats.rs:586`): 30624 source →
  session checkout `<path>/<role>` → installed local pack → shipped. The
  provider receives only `pack_dir` + `persona_id`
  (`crates/buzz-session-provider/src/session.rs:176-190`), resolves the persona
  (`:1369`), quotes its body verbatim into the briefing
  (`seat_role_briefing`, `:1738`) and materializes its skills into
  `<app data>/agents/seats/<session id>/skills/`
  (`crates/buzz-core/src/coding_session_seat_bundle.rs`; ledger 132, 135).
  Nothing is written into the seat's tree.
- `packRef {repo, sha, role, path}` on 44223 metadata, `path` must end in
  `/<role>` (`crates/buzz-core/src/coding_session_payload.rs:1348,1449`); the
  metadata key set is closed and enumerated (`:1580-1600`).
- The workflow engine (`crates/buzz-workflow`): triggers `message_posted`,
  `reaction_added`, `diff_posted`, `schedule {cron | interval}`, `webhook`
  (`src/schema.rs:38`); actions `send_message`, `send_dm`,
  `set_channel_topic`, `add_reaction`, `call_webhook`, `request_approval`,
  `delay`, `record_ci_result` (`:92`). A definition is kind 30620 with the YAML
  as content, channel-scoped by `h`, projected to a Postgres row
  (`crates/buzz-relay/src/handlers/command_executor.rs:714-830`;
  `schema/schema.sql:366`). Manual trigger is kind 46020 (`:922`, owner-only at
  `:953`). Runs are rows (`schema.sql:390`); telemetry is kinds 46001–46012;
  the 60-second cron loop claims each fire durably
  (`crates/buzz-workflow/src/lib.rs:489`; `claim_scheduled_workflow_fire`
  `crates/buzz-db/src/workflow.rs:499`; PK `schema.sql:456-467`).
- Approval **resume** exists on the relay (`resume_workflow_after_approval`,
  `command_executor.rs:1380-1403`, guarded on `WaitingApproval`) but the
  engine never produces that state: `finalize_run` marks any run that reached
  `request_approval` as `Failed / approval_not_supported`
  (`crates/buzz-workflow/src/lib.rs:229-253`, WF-08).
- Precedents for "an event wakes an agent without a model polling":
  `crates/buzz-session-provider/src/ci_result_listener.rs` (one authenticated
  subscription, signer must equal the witnessed relay `self`, `:100-114`) with
  `ci_continuation.rs` (result rebuilt into an addressed turn, idempotent via
  the operation ledger, `DeliveredContinuation` `:155-170`;
  `consume_operation` `state.rs:822`; store states
  `ci_continuation_store.rs:87`); `team_wake.rs` (`provider_may_wake` `:317`,
  `build_wake_event` `:455`).
- Run-and-capture: `crates/buzz-dev-mcp/src/shell.rs` (defaults `:16-22`;
  result shape `:298-318`); gate rows kind 44246 minted by
  `crates/buzz-session-provider/src/gate_observer.rs`.
- Persistent agent identity and per-project association:
  `ManagedAgentRecord.home_role` / `project_ref`
  (`desktop/src-tauri/src/managed_agents/types.rs:354,358`); hire is 44221
  `session.hire` answered by a seated `session.create`
  (`crates/buzz-core/src/coding_session_lifecycle_command.rs:108,211`); ACL
  `crates/buzz-db/src/coding_session_acl.rs:57-63`.

**Does not exist.**

- Any include, reference, overlay or version-range syntax in role text. D12's
  "project overlay" and D15's "playbook" are unbuilt.
- Any per-repository team convention other than `personas/roles/<role>/`
  packs. No `beekeeper/`, `.beekeeper/`, `.buzz/` directory is read anywhere.
- A workflow action that runs a command, wakes an agent or hires one; a
  project-scoped workflow; a trigger on a git push (the relay-signed kind
  30618 repo state is inserted with `channel_id = None`,
  `crates/buzz-relay/src/api/git/transport.rs:2160-2176`, so `on_event` never
  sees it); a trigger on a CI result (46008 is channel-scoped but the loop
  guard `is_workflow_execution_kind` drops 46001..=46012,
  `crates/buzz-core/src/kind.rs:1655`); a schedule in any timezone but UTC
  (`cron_fire_instant`, `lib.rs:765`).
- A GUI for project-level actions. Workflows have a channel-scoped screen only.

## 3. Part A — shipped templates

### 3.1 Layout

Templates sit beside the shipped packs. The shipped packs stay complete packs:
rung 4 of the ladder and `bee packs init` seeding
(`desktop/src-tauri/src/managed_agents/packs_repo.rs`) are untouched, and
decision 2 of `PROJECT_TEAM_SETUP_IMPL.md` wants a whole pack as the neutral
foundation a project copies.

```
personas/
  roles/<role>/…                        # unchanged packs
  templates/
    working-contract/1.0.0/TEMPLATE.md
    memory/1.0.0/TEMPLATE.md
    memory/1.0.0/skills/recall/SKILL.md
    memory/1.2.0/TEMPLATE.md
    memory/1.2.0/skills/recall/SKILL.md
    project-pulse/1.0.0/TEMPLATE.md
    project-pulse/1.0.0/skills/check-pulse/SKILL.md
```

A template is one `TEMPLATE.md` and an optional `skills/` directory in the
existing `SKILL.md` format (`crates/buzz-persona/src/skill_meta.rs`).
Templates are leaves: a template cannot include another template, so there
are no cycles and no transitive surprises.

```markdown
---
name: memory
version: 1.2.0
description: "Recall and record durable project memory through the memory MCP."
deprecated: null            # or: "superseded by 1.2.0: the recall tool was renamed"
skills:
  - ./skills/recall/
---
Before acting on a task, recall what this project already knows about it…
```

### 3.2 Versioning rules

- The directory name must equal `version`; the validator refuses otherwise.
- A new version directory is added **only** when the text or skills change
  between app releases. Identical bytes under two versions is a validator
  warning, so the catalog does not grow by habit.
- Ranges: `@1.2.0` exact, `@^1.0.0`, `@~1.1`, `@latest`. `@latest` is the
  highest version whose `deprecated` is null. A range matching only deprecated
  versions resolves to the highest of them **with a warning**. A range
  matching nothing **refuses**, naming what this build ships: `this build
  ships memory 1.0.0, 1.2.0; ^2.0.0 matches none — update Beekeeper or widen
  the range`.
- Resolution is a pure function of (range, this build's catalog). Add
  `semver = "1"` as a direct dependency of `buzz-persona` (already in
  `Cargo.lock` transitively at 1.0.28).
- Bundle: add `personas/templates` beside the `personas/roles` resource in
  `desktop/src-tauri/tauri.conf.json:65`; `shipped_templates_dir` mirrors
  `shipped_packs_dir` (`packs_cache.rs:263`). The catalog's identity on the
  wire is the app version, the value `shipped_packs_version` already uses.
- Clone: `bee packs clone-template memory@1.2.0 --into beekeeper/` copies
  `TEMPLATE.md` to `beekeeper/templates/memory.md` and its skills to
  `beekeeper/skills/`, rewriting nothing; the project then includes it as a
  project file and owns it.

### 3.3 First templates

`working-contract` — the "Working contract" paragraph all eight shipped
personas repeat verbatim today (`grep -l "Working contract"
personas/roles/*/personas/*.md`); `memory`; `project-pulse` (the `bee pulse`
re-check Andy's project-manager example wants).

## 4. Part B — the project's team in its own repository

### 4.1 Layout

```
beekeeper/
  team.yml
  roles/project-manager.md
  roles/builder.md
  roles/builder/skills/run-ci/SKILL.md     # role-private skills, auto-claimed
  skills/verify-before-claiming/SKILL.md   # shared: every role receives it
  rules.md                                 # any file a role may include
  actions.yml                              # Part C
```

Andy's draft called the manifest `beekeeper-agents.yml`; `team.yml` was
proposed because D11 says git never holds identities and the file mostly
describes roles. Andy accepted `team.yml` on 2026-09-16.

### 4.2 `team.yml`

```yaml
schema: beekeeper-team/v1
version: 0.3.0                  # becomes the synthesized pack version
lead: project-manager           # the role hired first (D14)
roles:
  project-manager:
    file: roles/project-manager.md      # default: roles/<role>.md
    runtime: claude                     # advisory → PersonaConfig.runtime
    model: anthropic:claude-sonnet-5    # advisory → PersonaConfig.model (D17: the router decides)
    workspace: { roles_visible: true }  # this role may see beekeeper/ in its worktree (§ 4.10)
  builder: {}
agents:                         # advisory: the host mints identities (D11)
  - { name: Keystone, role: project-manager, lifetime: persistent }
  - { name: Levain,   role: builder,         lifetime: ephemeral }
```

`lifetime` is parsed and surfaced here and enforced only by Part C (§ 5.7).
Status 2026-09-17: the manifest is read by the composer (ledger 147); `name`
is optional and defaults to the root directory's name; `runtime`/`model`
fill in only where the role file's own frontmatter is silent.
`agents[].name` is what `actions.yml` uses to name a routing target. A role in
`agents` that is not in `roles` is a validation error; a role in `roles` with
no agent is fine (hired by name at run time).

### 4.3 `roles/<role>.md`

Bare markdown with optional frontmatter. When frontmatter is present it must
be valid persona frontmatter — the same `Frontmatter` struct, still rejecting
unknown keys. Defaults when absent: `name`, `role` and `display_name` from the
file stem, which must pass `is_valid_role_slug` (`persona.rs:64`);
`description` from the role's own first line of prose (a template's opening
line describes the template, not the role), falling back to the expanded
body, and refusing when neither has one. `skills:` paths
are relative to `beekeeper/`; skills under `roles/<role>/skills/` are claimed
by that role automatically.

```markdown
---
description: "Keeps the plan, hires builders, never commits code."
runtime: claude
---
![[beekeeper/working-contract@^1.0.0]]
![[beekeeper/memory@^1.0.0]]
![[beekeeper/project-pulse@latest]]
![[./rules.md]]

Always re-check for new pulse items at the end of each turn.
Always spawn new worker agents for tasks; you do not have access to commit code.
```

### 4.4 Include syntax

One sigil pair, whole-line directives only. A line that is not exactly a
directive is prose: `![[` in the middle of a sentence is never parsed, and
the validator warns "looks like an include but is not on its own line".

| Form | Meaning |
| --- | --- |
| `![[beekeeper/<template>@<range>]]` | a shipped template; the `@` part is required, there is no implicit latest |
| `![[./<path>]]` | a project file relative to `beekeeper/`; must stay inside it (the `PathEscape` rule, `pack.rs:44`) |
| `![[roles/<role>]]` | another project role's **resolved body**; its frontmatter and skills are not inherited |

Why not Andy's draft `[@…]` / `[!…]`: `[@key]` is a pandoc citation and
`[!NOTE]` is a GitHub alert, so both render as something else in the tools
people read these files in. `![[…]]` is the established transclusion spelling
in wiki-style markdown, renders as literal text on GitHub, and one sigil means
one parser and one error message. Andy accepted this spelling on
2026-09-16; nothing below depends on it.

### 4.5 Resolution: a pure composer, a staged artifact, an untouched provider

`buzz_persona::compose::compose_role(source: &RoleSource, catalog:
&TemplateCatalog) -> Result<ComposedRole, ComposeError>` where
`RoleSource::{Pack(dir), Flat{root, role}}` and `TemplateCatalog{dir,
app_version}`. It returns the expanded body, a synthesized `LoadedPack`
(`pack.rs:64`) whose manifest is `{id: "project:<slug>", version:
team.yml.version, personas: [<role>]}` with one persona whose `role == name`,
the union of skills from every source, and a provenance record.

The **host then writes a staged pack directory**, the vision's "locally
resolved artifact" made literal:

```
<app data>/packs/staged/<source-key>/<digest12>/
  .plugin/plugin.json                  # synthesized manifest
  personas/<role>.persona.md           # fully expanded body, full frontmatter
  skills/<every skill from every source>/SKILL.md
  compose.json                         # provenance, § 4.6
```

`source-key` is `<owner8>-<id>-<sha>` for a repository and `app-<version>`
for shipped; `digest12` is the first twelve hex of SHA-256 over the expanded
persona and skill bytes. Every rung of `plan_seat_pack` goes through staging,
including legacy packs, so the provider sees one shape.

The provider is **unchanged**: it still receives `pack_dir` and `persona_id`
(`session.rs:176-190`), still calls `resolve_persona_by_name` (`:1369`),
`materialize_skill_bundle` still copies from one `skills_dir`
(`crates/buzz-persona/src/skills.rs:301`), and `seat_role_briefing` still
quotes the body verbatim (`:1738`).

Staging also removes a latent hazard: today a seat's `pack_dir` points inside
the shared packs checkout (`packs_cache.rs:591-600`), and a later hire under a
`ref` pin runs `git checkout --detach --force` plus `git clean` on that same
directory (`packs_cache.rs:447-459`). That is "an active execution silently
changes instructions", which the vision forbids. A staged directory keyed by
source sha and digest is immutable for the seat's life.

Rules:

- Cycle detection over `roles/<role>` includes with an explicit stack; depth
  cap 8; a cycle refuses naming the chain (`project-manager → roles/lead →
  project-manager`).
- The expanded body is held to `MAX_BODY_BYTES` (`persona.rs:24`): a pile of
  templates cannot exceed what a hand-written persona may.
- A skill name present in two sources (two templates both shipping `recall`)
  is a compose refusal, never a silent overwrite.
- A missing template, version or file refuses the hire through the existing
  `HIRE_PACK_UNAVAILABLE` path (`packs_cache.rs:55`), surfaces in
  `RolePackSummary.refusal` (`desktop/src-tauri/src/managed_agents/
  role_packs_view.rs:64`) and in a new `compose: {ok, reason}` object in `bee
  packs status`. A deprecated template resolves with a warning:
  `RolePackSummary` gains `warnings: Vec<String>`, `bee packs status` prints
  them, and readiness stays green — a warning is a warning.
- Staged directories are pruned when no `actor-seats.json` entry references
  them; a running seat's directory is never pruned.

### 4.6 Provenance

`packRef` is unchanged and names the **source**: repo, sha, role, path. For
the flat layout `path` is `beekeeper/roles/<role>` — it ends in `/<role>`, so
the closed validator at `coding_session_payload.rs:1449` passes; the spec
records that the `.md` is implied. Template resolution is a function of that
source plus the app version, and the app version is not on a repository
`packRef`, so slice A4 adds one additive closed-set key to 44223 metadata,
`composeRef {appVersion, digest}`, following the `packRef` amendment pattern
(`:1580-1600`), and writes the same object into the seat bundle
`manifest.json` (`skills.rs:335`). Until A4 lands, `bee packs status` says
"template versions are recorded locally in compose.json and not yet
published" rather than implying otherwise.

```json
{
  "schema": "buzz-composed-role/v1",
  "role": "project-manager",
  "source": {"kind": "repository", "repo": "30617:…", "sha": "…", "path": "beekeeper/roles/project-manager"},
  "appVersion": "0.4.2",
  "digest": "sha256:…",
  "includes": [
    {"ref": "beekeeper/memory@^1.0.0", "resolved": "1.2.0", "deprecated": null},
    {"ref": "beekeeper/project-pulse@latest", "resolved": "1.0.0", "deprecated": null},
    {"ref": "./rules.md", "bytes": 1832}
  ],
  "warnings": []
}
```

Vision mapping: project source = the git bytes at `packRef.sha`; locally
resolved artifact = the staged directory; staged execution revision =
`packRef.sha` + `composeRef.digest`; adoption evidence = unchanged
(`docs/ROLE_ADOPTION_EVIDENCE.md`).

### 4.7 The 30624 pin when roles ride with the code

`repo` = the project's **code** repository coordinate, `path` = `beekeeper`,
pin `ref: refs/heads/<default>` by default; `sha` stays available for a
release train. Decision 8 of `PROJECT_TEAM_SETUP_IMPL.md` preferred an
immutable sha because 30624's CAS cannot protect bytes pushed to an adopted
branch. When the roles live in the code repository, the bytes are protected
by that repository's own controls — review and the `buzz-protect` rules on its
30617 (`crates/buzz-core/src/git_perms.rs`) — the same controls that protect
the code the role will run against. The spec states plainly what each control
enforces: the `ref` pin guarantees **which branch**; `packRef.sha` guarantees
**which bytes ran**; nothing guarantees the branch was reviewed unless the
repository's rules say so. This amends decision 8 for the in-repo case only;
sibling packs repos keep the immutable-sha rule.

Execution boundary: `plan_seat_pack` re-resolves on every hire and on restage
(`actor_seats_restage.rs`), so a new seat gets the new tip while a running
seat keeps its staged directory. `pack_revisions.rs` is unchanged: it compares
`packRef.sha` by git ancestry, and "earlier revision, still running" is the row
it was built to show. Template drift is not git ancestry; it is app-version
inequality, disclosed as its own row: "composed with Beekeeper 0.4.1; this
computer is 0.4.2".

### 4.8 Discovery and compatibility

`locate_role_source(checkout, path, role)` replaces `role_pack_in_checkout`
(`packs_cache.rs:547`) and tries, in order: (1) `<path>/<role>/.plugin/
plugin.json` → `RoleSource::Pack`; (2) `<path>/roles/<role>.md` →
`RoleSource::Flat`; (3) none. Tank Loop and Beekeeper's `agiterra-packs` keep
their `personas/roles/<role>/` packs and their `packRef.path`; a legacy pack
with no include lines composes to bytes identical to today (pinned by a test).
The ladder order is unchanged; the session-checkout rung also learns
`beekeeper/roles/<role>.md`. `bee packs init` keeps seeding the pack layout;
`--layout flat` arrives in slice A3.

### 4.9 Worktrees: main's roles by default, a branch may override

A seat runs in a linked worktree the host cuts from the project checkout on
its own branch (`desktop/src-tauri/src/coding_sessions/worktree.rs:596`,
`git worktree add -b <branch> <path> <start_point>`). Andy's rule
(2026-09-16): a worktree runs the roles that are on `main`, unless that
worktree's branch specifically overrides a role's definition.

- **Default.** A seat's roles come from the project's 30624 pin (§ 4.7,
  `ref: refs/heads/main`), read from the host's packs cache, never from the
  seat's working copy. A feature branch therefore runs `main`'s roles even
  though its tree may carry an older `beekeeper/`.
- **Override, per role.** Before staging, the host compares the commit the
  seat's tree is cut from (`start_point`, or the branch tip on restart)
  against the pinned commit over the files that role's composition reads:
  `beekeeper/roles/<role>.md`, everything it includes transitively, its
  `team.yml` entry and its skills. If any differ, the role is composed from
  the branch commit's bytes (`git show <sha>:beekeeper/…`, never the working
  copy). `compose.json` says `source.kind: "branch-override"` and
  `packRef.sha` is that commit — a real commit, possibly unpushed, and the
  Roles and Agents tabs say "branch-local, not on main". Other roles in the
  same team stay on `main`'s definitions.
- **Uncommitted edits are never in effect.** A dirty `beekeeper/` in a
  worktree is disclosed as "uncommitted role edits in this worktree are not
  in effect; commit them, then restart", the same stance verification turns
  take on a dirty tree (ledger 133). There is no way to run bytes the wire
  cannot name.
- **A running seat sees its definition change only at a boundary it is
  given.** For every open seat the host re-resolves the role in its context
  (main pin, or branch override) and compares the result's digest and commit
  with the seat's `composeRef.digest` / `packRef.sha`. Where they differ, the
  seat's card on the Agents tab and in the session shows **Definition
  changed** with the cause named — "main moved `9a1…` → `c04…` in
  `roles/builder.md`" or "this branch changed `roles/builder.md`" — and one
  button, **Restart with current definition**. The click closes the current
  execution, re-stages the seat through the existing restage path
  (`desktop/src-tauri/src/managed_agents/actor_seats_restage.rs`) and starts
  a new execution of the **same identity** through the provider's restore
  path (`crates/buzz-session-provider/src/native_restore.rs`,
  `rehydrated_bootstrap` in `crates/buzz-session-provider/src/session.rs:1696`), so it continues from its
  checkpoint with the new instructions and the transcript shows the
  boundary. Nothing restarts without the click; a project policy that
  restarts automatically is not v1 (§ 6). If the host cannot resolve one
  side (packs cache unreachable), the card says "unknown", not "current".

### 4.10 Seats do not see the roles directory

Observed by Andy (2026-09-16): an instantiated agent that finds the other
roles' instructions while searching the repository gets confused about its
own role. With roles in the project repository, every seat's worktree would
contain all of them. The fix keeps roles in the repository — that is what
makes § 4.9's branch override and reviewing role changes beside the code
possible — and removes them from every seat's working copy.

- **Mechanism.** When the host cuts a seat worktree it runs
  `git sparse-checkout set --no-cone '/*' '!/beekeeper/'` in that worktree.
  Proven on git 2.55.0: the seat's tree has no `beekeeper/` files, the hub
  checkout is untouched, and the setting lives in the worktree's own config
  (`core.sparseCheckout`, written with `--worktree`).
  `extensions.worktreeConfig` is already enabled for seats by the seat-hooks
  command (`desktop/src-tauri/src/commands/coding_session_seat_hooks.rs:
  16-30`), for the same reason: a setting that says "this seat" must not
  reach the person's own checkout.
- **The host never composes from a seat's working copy.** Pinned source →
  packs cache (§ 4.5); branch override → `git show <sha>:beekeeper/…`
  (§ 4.9). Skills materialize into the seat bundle outside the tree as today
  (ledger 132, 135). So the exclusion costs the seat nothing it needs.
- **Opt-in visibility.** `team.yml` `roles.<role>.workspace.roles_visible:
  true` (default `false`) skips the exclusion for a role whose job is to
  author or evolve roles — `project-setup`, or a lead maintaining the pack —
  and the Roles tab discloses which roles can see the directory.
- **Honest limit.** The exclusion removes the files from the working tree,
  from file search and from the harness's directory walk. It does not stop
  a seat that deliberately runs `git show main:beekeeper/roles/lead.md`; the
  objects are in the repository. It targets accidental confusion, not
  exfiltration, and changes no authority.
- **Harness directives are a second layer, not a replacement (Andy,
  2026-09-17).** On Claude Code the permission grammar the write fence
  already uses has `Read(…)` rules, and the provider already writes
  `.claude/settings.local.json` into every Claude seat before the child
  starts (`crates/buzz-session-provider/src/agent_fence.rs:297,635`). A
  `Read` denial for the worktree's `beekeeper/` directory joins that file.
  Its limits are the write fence's, measured and recorded there: the file
  tools are governed, Bash is not (`cat`, `grep`, `rg` still read), and
  whether Grep and Glob honour a `Read` denial is **measured before it is
  claimed** (slice A2). Codex has no permission surface — codex-acp's fence
  is briefing text only (`agent_fence.rs:98`) — so on Codex the sparse
  checkout is the whole mechanism. No `.claudeignore` or Codex equivalent
  exists to rely on.
- **Not affected.** The operator's own checkout, and Solo sessions, which
  run in the operator's folder and hold no role. Offering Solo sessions the
  same exclusion is a possible setting, not part of this spec.
- **Rejected.** An orphan `beekeeper-roles` branch or a sibling packs repo
  would hide the files but lose the branch override and the review-with-
  the-code property; telling agents not to read the directory has already
  failed in practice, which is the observation this section answers.

## 5. Part C — agent types and project actions

### 5.1 An action is a workflow definition

`beekeeper/actions.yml` is a list of `WorkflowDef` entries plus two top-level
keys, `schema` and `timezone`. Three new `TriggerDef` variants: `ref_updated
{ref glob, repository?}`, `ci_result {check, conclusion[]}`, `manual`. Three
new `ActionDef` variants: `run_on_host`, `wake_agent`, `hire_agent`. A
separate compiled format would be a parallel registry with its own run row,
claim and cron, which `COLLABORATIVE_WORKSPACE_PLAN.md` § "Decisions already
made" forbids; extending the schema reuses the run row, the durable fire
claim, the approvals table, evalexpr conditions and the telemetry kinds as
they are. It is a sibling of `team.yml`, not a section of it: agents are
identities, actions are programs, and a different parser validates each.

```yaml
schema: buzz-project-actions/v1
timezone: America/New_York          # default for every schedule below
actions:

  # (1) conditional: wake an agent only on failure
  - name: nightly-build
    trigger: { on: schedule, cron: "0 0 17 * * FRI" }   # Friday 17:00 local
    steps:
      - id: build
        action: run_on_host
        command: ["just", "ci"]
        working_directory: "."          # relative to the project checkout; ".." refused
        timeout: 1800s                  # host cap 3600s
        env: { CARGO_TERM_COLOR: never }          # literals only; no secrets in git
        env_from_host: [NPM_TOKEN]                # names only; the host supplies values
        capture: { tail_bytes: 8192, artifact_max_bytes: 10485760 }
      - id: fix
        action: wake_agent
        if: "steps_build_output_exit_code != 0"
        to: { agent: Levain }
        brief: "The nightly build failed. Fix the build errors, run the gate, report."

  # (2) always: a different brief on success and on failure
  - name: on-push-main
    trigger: { on: ref_updated, ref: "refs/heads/main" }
    steps:
      - id: build
        action: run_on_host
        command: ["cargo", "build", "--workspace"]
      - id: review
        action: wake_agent
        to: { agent: Keystone }
        brief:
          on_success: "Review the build log for unexpected warnings."
          on_failure: "Try to correct the build errors, then report."

  # (3) agent-managed: no host step; the agent runs and watches the process
  - name: flaky-e2e
    trigger: { on: manual }
    steps:
      - id: run
        action: hire_agent
        role: runner
        session: { agent: Keystone }
        brief: "Run `just test-e2e` yourself. Act on unexpected output; report."

  - name: after-ci
    trigger: { on: ci_result, check: gate, conclusion: [failure] }
    steps:
      - id: triage
        action: wake_agent
        to: { agent: Keystone }
        brief: "CI failed on {{trigger.commit}}: {{trigger.evidence_url}}"
```

Host-side validation refuses an absolute or parent-escaping
`working_directory`, a `timeout` above 3600 s, and a `wake_agent` naming an
agent absent from `team.yml`; it warns on an `env` value that looks like a
secret. `brief.on_success/on_failure` is sugar the host compiles into two
`wake_agent` steps with opposite `if` on the preceding step's exit code.

### 5.2 Routing modes, and what "monitor" can honestly mean

Routing steps are ordinary steps after `run_on_host`, so `if:` uses the
engine's flat names (`crates/buzz-workflow/src/executor.rs:253-263`):
`steps_<id>_output_exit_code`, `_timed_out`, `_stdout_tail`, with
`str_contains` available. Mode (1) is one conditional step; mode (2) is the
sugar above; mode (3) has no `run_on_host` — the command lives in the brief
and the agent runs it through its own tools. Limits to state in the product,
not paper over: a dev-mcp shell call is foreground for at most 600 s
(`shell.rs:17`), and the provider enforces the 870 s silence and 7,200 s turn
caps (`COLLABORATIVE_WORKSPACE_PLAN.md` § 2a). So an agent either runs the
command within the cap or backgrounds it and re-reads the artifact with
bounded polls; there is no streaming feed into a model turn, and
`gate_observer.rs` still mints observed gate rows for recognised commands.

### 5.3 Project scoping and triggers

- **Registration.** The host publishes each entry as one kind 30620 whose `d`
  is `sha256(<project coordinate> + <action name>)`, so a republish is an
  upsert (`command_executor.rs:714-830`), with the existing `h` tag (the
  project's coding-session channel) plus a new `a` tag
  `30621:<owner>:<d>`. The relay gains a nullable `workflows.project_ref`
  column (`schema.sql:366-381`); a write with `a` is admitted from a project
  Owner or a repository founder, the 30624 rule. `definition_hash`
  (`schema.sql:373`) is what an autorun grant binds to (§ 5.4).
- **`ref_updated`.** A new `WorkflowEngine::on_repo_state(community, repo,
  before_refs, after_refs, pusher, event_id)` is called right after the
  post-CAS fan-out at `transport.rs:2165`, where both ref maps are in scope
  (`:2138-2145`). It loads workflows whose project's 30621 lists that
  repository and matches the glob against changed refs. `TriggerContext`
  (`executor.rs:27-42`) gains `repository`, `ref`, `before`, `after`,
  `pusher`. Never derive this from the pre-receive policy path
  (`crates/buzz-relay/src/api/git/policy.rs:194`): a denied push must not
  fire, and 30618's existing no-change suppression makes no-op pushes silent.
- **`ci_result`.** In `on_event`, let kind 46008 through only when the
  channel holds a workflow with `TriggerDef::CiResult` and the decoded
  result's `identity.project` (`crates/buzz-core/src/ci_result.rs:56-73`)
  equals that workflow's `project_ref`; every other 460xx kind stays refused.
- **Schedule timezone.** `Schedule` gains `timezone: Option<String>` (IANA
  name, `chrono-tz`), defaulting to the file's top-level value, then UTC.
  `cron_fire_instant` (`lib.rs:765`) evaluates in that zone and converts the
  fire instant to UTC before `claim_scheduled_workflow_fire`, so the
  `(community, workflow, scheduled_for)` primary key is untouched. A local
  time skipped by a DST change fires at the next valid instant, disclosed in
  the run's trigger context.
- **`manual`.** A definition only kind 46020 can start. `handle_workflow_
  trigger` is owner-only (`command_executor.rs:953`); for a definition with
  `project_ref` it also admits project Owner and Admin per roster 39010. The
  desktop already signs 46020 (`desktop/src-tauri/src/events/workflows.rs:35`)
  and `bee workflows trigger` covers agents.

### 5.4 Run record, approval and the autorun grant

The run is the existing `workflow_runs` row. Two new tables:

- `workflow_host_steps (community_id, run_id, step_id) PK; status ∈
  {requested, claimed, exited, lost, expired}; requested_event_id;
  claimed_by; claimed_at; claim_event_id; result_event_id; exit_code NULL;
  timed_out; duration_ms; head_sha; dirty; artifact_ref; exited_at`. The
  claim is `UPDATE … SET claimed_by = $host WHERE claimed_by IS NULL
  RETURNING`, the shape `claim_scheduled_workflow_fire` uses, so exactly one
  host executes; the loser's ingest response says `claimed by <host>` and the
  GUI shows "executed on <host>".
- `workflow_autorun_grants (community_id, workflow_id, definition_hash,
  granted_by, grant_event_id, granted_at, revoked_at, revoke_event_id)`.

Approval is **not authored YAML**. For any definition with `project_ref`, the
engine inserts a synthetic approval gate before the first `run_on_host` unless
an unrevoked grant matches the current `definition_hash`. (Built in C1 as: the
gate is bound to the host step's own index, so the approval row that releases
it names that step, and `resume_index_after_approval` resumes *at* the step
rather than after it; an authored `request_approval` still resumes after.) Editing the action
changes the hash and re-arms approval. First task: close WF-08 — `finalize_run`
writes `WaitingApproval` and `create_approval` (`workflow.rs:990`) instead of
failing, and the existing 46010 request plus `handle_approval_grant`
(`command_executor.rs:1127`) resume it. `check_approver_spec` (`:1102-1125`,
pubkey or `any` today) gains `project-owner:<coord>` resolved against roster
39010. The 46030 grant content becomes JSON `{"note": "…", "scope": "run" |
"action"}` (builder `events/workflows.rs:41`); `scope: action` is the
checkbox and inserts the grant row. The operator's desktop key signs it
through `grant_approval` (`desktop/src-tauri/src/commands/workflows.rs:351`).
Revocation is kind 46032 or the GUI; both emit 46015. Relaxing the default
later is one relay-side rule (`approval: per_run | granted | none`), never a
host change.

### 5.5 Records and kinds

All new constants go in `crates/buzz-core/src/kind.rs`; none of these numbers
is in use on `main` `92cc7b888`.

| Kind | Signer | Name | Carries |
| --- | --- | --- | --- |
| 46013 | relay | `WORKFLOW_HOST_STEP_REQUESTED` | `d` = `<run>:<step>`, `a` project, `h` channel; `{runId, workflowId, stepId, definitionHash, stepKind, triggerContext, approval: {eventId, scope}, expiresAt}` — **no command text** |
| 46014 | relay | `WORKFLOW_HOST_STEP_EXITED` | echo of the accepted result plus `claimedBy` |
| 46015 | relay | `WORKFLOW_AUTORUN_CHANGED` | grant or revoke, bound hash |
| 46022 | host (provider) | `HOST_STEP_CLAIM` | the 46013 it claims |
| 46023 | host (provider) | `HOST_STEP_RESULT` | exit code or `null` with `disposition`, `headSha`, `dirty`, tails, artifact ref |
| 46032 | operator | `WORKFLOW_AUTORUN_REVOKE` | workflow id, hash |

`is_workflow_execution_kind` extends to 46015. Why not kind 44246 for the
exit: an observation needs `d = sessionRef` and a `csob-genesis`
(`kind.rs:773-780`), and a scheduled run has no session. When a result is
routed to a seat, the host **also** writes a 44246 gate row (`source:
observed`) into that session so the seat's evidence trail carries it.

### 5.6 Host verification and execution

The **session provider** executes, not the desktop: it already holds a relay
identity, a state directory, the operation ledger and the restart patterns.
New `crates/buzz-session-provider/src/action_steps.rs` and
`action_step_listener.rs` — one authenticated subscription
`{"kinds":[46013],"#a":[served project coordinates]}` mirrored on
`ci_result_listener.rs`.

Before executing, the host checks: the 46013's signer equals the witnessed
relay `self` (`ci_result_listener.rs:100-114`); its `a` names a project this
host serves; and its `definitionHash` equals the hash of the entry the host
compiles from `beekeeper/actions.yml` in its **own** checkout, else refuse
`ACTION_DEFINITION_DRIFT`. The 46013 carries no command text: the relay
cannot inject a command the repository does not contain.

- **Checkout.** The project's recorded repository folder
  (`ProjectsFile.projects[coord]`, `crates/buzz-session-provider/src/
  commands.rs:1775-1783`), the same folder hires cut from. Absent → refuse
  `ACTION_CHECKOUT_NOT_RECORDED`, naming Project settings → This computer →
  Repository folder (the `HIRE_CHECKOUT_NOT_RECORDED` precedent, ledger 136,
  139). v1 runs on whatever that folder has checked out and records `headSha`
  and `dirty`, so a reader sees the truth; a fresh worktree at `trigger.after`
  is slice C6.
- **Run.** Lift `buzz-dev-mcp/src/shell.rs::run` into a shared function: same
  process-group kill, 8 KB tails, 10 MB artifact, exit 124 on timeout.
  Artifacts at `<state_dir>/actions/<run>/<step>/{stdout,stderr}.log`; the
  result carries the path, disclosed as host-local. Values of `env_from_host`
  names are scrubbed from the tails.
- **Idempotency.** `consume_operation("action-step:<run>:<step>")`
  (`state.rs:822`) is written at spawn, before the claim event is published; a
  durable `action-steps.json` store in the `ci_continuation_store.rs` style
  tracks requested → claimed → running(pid) → exited → reported. On restart
  with a `running` record and a dead pid the host publishes a 46023 with
  `exitCode: null, disposition: "lost_on_restart"`. Scripts are not assumed
  idempotent: nothing re-runs without a new run.

### 5.7 Routing targets; persistent and ephemeral agents

Routing steps are also host steps (46013 `stepKind: wake_agent |
hire_agent`) because only the host holds a key with session authority.

- **Persistent target** (`to: {agent}`): resolve the agent's open umbrella for
  this project (44223 `agentRef` + `projectRef` fold, as
  `docs/PROJECT_AGENTS_TAB_SPEC.md` reads it); build an addressed 44220
  `thread.turn.start` with `build_wake_event` (`team_wake.rs:455`) under
  operation key `action-route:<run>:<step>`; the provider must satisfy
  `provider_may_wake` (`:317`), else refuse `ROUTE_NO_AUTHORITY`.
- **Ephemeral target** (`hire_agent`): the provider publishes 44221
  `session.hire` into the named umbrella under `may_hire`; the desktop answers
  as it does today. A brand-new seated Solo session is **not** v1: seat custody
  is staged only by the desktop (`actor_seats.rs:1-30` in the provider).
- **Brief** follows the `DeliveredContinuation` shape (`ci_continuation.rs:
  155-170`): `{type: "action_result", operationId, run: {workflowId, runId,
  stepId, requestedEventId, resultEventId}, trigger, exit: {code, timedOut,
  durationMs, headSha, dirty}, output: {stdoutTail, stderrTail, truncated,
  artifactPath}, brief}`.

Minimum definition of the two agent kinds. **Persistent**: one open umbrella
per (agent, project) that the host creates on the first route, resumes after
a restart and never closes on a report. **Ephemeral**: a hire whose 44244
report lets the lead or the host close its seat. Truth stays the 44223/44230
fold; `team.yml`'s `lifetime` only tells the host which behaviour to apply.

### 5.8 GUI

Add `actions` to `PATH_TABS`
(`desktop/src/features/projects-container/ui/ProjectPageTabs.tsx:22-31`) with
a route beside `projects.$projectId.agents.tsx`. Data: 30620 by `#a`; runs
from `/workflows/{id}/runs`; host steps from 46013/46014 in the `h` channel;
approvals from 46010 plus `grant_approval` with the checkbox mapped to
`scope`. Rows show only what records prove: `waiting approval (since …)`,
`approved (this run | this action) by <name>`, `claimed by <host>, no result
yet` (never "running"), `exited 1 on <host> at <sha>, dirty`, `routed to
<agent> · turn <commandId>`, `lost on host restart`.

## 6. Honest scope and open questions

Not in this spec: a relay-side shell; a desktop scheduler; a second role
registry; secrets in git; a streaming process feed into a model turn; a
hire that opens a new umbrella from a host step; enforcement of `lifetime`
beyond § 5.7; any change to how Solo sessions start
(`PROJECT_TEAM_SETUP_IMPL.md` § "Ordinary sessions remain first-class").

Open, to be settled before the slice that needs them:

1. ~~Include sigil: `![[…]]` (recommended) or Andy's `[@…]` / `[!…]` (A1).~~
   Settled 2026-09-16: Andy accepted `![[…]]`.
2. ~~File names: `team.yml` or `beekeeper-agents.yml`; `actions.yml` as a
   sibling (A3, C2).~~ Settled 2026-09-16: Andy accepted `team.yml` and
   `actions.yml`.
3. ~~Whether the session-checkout rung should prefer the session's own branch
   when that branch edits a role (A2; ties to C6).~~ Settled 2026-09-16 by
   § 4.9: `main`'s roles by default, a committed branch change to a role's
   inputs overrides that role, running seats are offered a restart.
4. Whether `hire_agent` may open a brand-new umbrella, which needs seat
   custody outside the desktop (after C5).
5. Whether a project policy may restart a seat automatically when its
   definition changes, instead of waiting for the click (§ 4.9; after A4).
6. Whether Solo sessions, which run in the operator's own checkout where
   the sparse exclusion does not apply, should get the Claude `Read` denial
   of § 4.10 as a per-project setting. It is the only lever there; Codex Solo
   sessions would have none.

## 7. Phased delivery

Each slice names what it proves, representative files and its tests. A/B and
C are independent lanes until C3, which needs `team.yml` (A3) for agent names.

### Part A/B

- **A1 — pure composer and template catalog.** `crates/buzz-persona/src/
  compose.rs`, `template.rs`, `Cargo.toml` (+`semver`), `personas/templates/
  {working-contract,memory,project-pulse}/…`, `PERSONA_PACK_SPEC.md` § templates,
  `bee pack compose <dir> --role <r>` printing the expanded persona and
  `compose.json`. Tests: exact/caret/tilde/latest; latest skips deprecated;
  deprecated-only warns; no match refuses naming the shipped set; cycle
  refuses naming the chain; depth cap; body cap; a legacy pack composes
  byte-identical; mid-line `![[` is prose.
- **A2 — flat layout and staged artifact on the host.** `packs_cache.rs`
  (`locate_role_source`, `stage_composed_pack`, `shipped_templates_dir`),
  `actor_seats.rs:586` (every rung stages), `role_packs_view.rs` (`warnings`),
  `crates/buzz-cli/src/commands/packs.rs` (`compose` block in `status`),
  `tauri.conf.json:65`, `coding_sessions/worktree.rs:596` (sparse exclusion
  of `beekeeper/` on every seat cut, § 4.10; branch-override detection and
  `git show`-based composition, § 4.9), `crates/buzz-session-provider/src/
  agent_fence.rs` (a `Read(//<worktree>/beekeeper/**)` denial in the Claude
  fence, with a measured note on whether Grep and Glob honour it). Tests:
  scratch-repo flat layout; pack-over-flat precedence; refusal text on a
  missing template; a running seat's `pack_dir` survives a checkout re-sync;
  a seat worktree has no `beekeeper/` files while the hub keeps them; a
  branch that changes `roles/builder.md` overrides builder only, and a dirty
  edit does not. Proof: a repo-resident `beekeeper/roles/builder.md` seats a
  builder whose 44223 `packRef.path` is `beekeeper/roles/builder`, and
  `find` in its worktree lists no role file.
- **A3 — `team.yml` and publication ergonomics.** `crates/buzz-persona/src/
  team.rs`, Roles page reads `team.yml` (advisory hints shown as advisory),
  `packs_repo.rs` / `packs_cli.rs` (`init --layout flat`, `clone-template`),
  `docs/nips/NIP-PK.md` (`path: beekeeper`, the ref-pin rationale of § 4.7);
  `workspace.roles_visible` honoured by the seat cut and disclosed on the
  Roles tab. Tests: schema refusals; an agent naming an unknown role refuses;
  clone-template round-trips bytes; a `roles_visible` role's worktree keeps
  `beekeeper/`.
- **A4 — wire provenance.** `coding_session_payload.rs` (`composeRef`,
  closed-set amendment and its decode test), the provider's 44223 publisher,
  `skills.rs:335` (bundle manifest), `pack_revisions.rs` (template-drift row
  and the per-seat definition-drift comparison of § 4.9), the Agents tab and
  session seat card (**Definition changed** + **Restart with current
  definition**, wired to `actor_seats_restage.rs` and the provider restore
  path). Tests: key-set shape test extended; a 44223 without `composeRef`
  decodes; drift reads "unknown" when the cache cannot be synced; the restart
  publishes a new execution for the same identity and the old one closes.
  Proof: edit `roles/builder.md` on `main`, watch the running builder's card
  offer the restart, click it, and read the new instructions in the new
  execution's briefing.
  *Status 2026-09-17: the wire half is built (ledger 152) — `composeRef` on
  44223, the provider reading it from the staged pack's `compose.json`, the
  bundle manifest mirror, and the Roles page's per-seat "composed with
  Beekeeper X · this computer is Y" line. The drift and restart half landed
  earlier under A3 (ledger 146, 148). The live proof is still owed.*
- **A5 — shipped roles compose from templates.** The eight "Working contract"
  paragraphs become `![[beekeeper/working-contract@^1.0.0]]`; rung 4 now
  exercises the composer. Test: the composed shipped lead equals today's lead
  byte-for-byte.

### Part C

- **C1 — manual run, approval, host execute, exit record.** Close WF-08
  (`lib.rs:229-253`); `RunOnHost`; kinds 46013/46014/46022/46023;
  `workflow_host_steps`; provider `action_steps.rs` + listener; a minimal
  Actions tab. Proof: `bee workflows trigger` → 46010 → grant → 46013 →
  provider runs `true` / `false` → 46014 exit 0 / 1; a second host sees
  `claimed by`. Tests: engine yields `WaitingApproval`; relay ingest claim
  race; provider store restart → `lost_on_restart`.
  *Status 2026-09-17: built (ledger 150). Three amendments to the text above,
  made while building: the run parks as a distinct `waiting_host` status,
  not `waiting_approval`, so the two resume guards can tell the parties
  apart; the step loop persists a suspension (row → status → publish) and
  `finalize_run` only logs it, so a grant or claim never finds a run that
  still says `running`; and the project binding rides inside the definition
  as `WorkflowDef.project` until C2 adds the column and `a`-tag index. Two
  more from review: `run_on_host` refuses a webhook trigger (the relay hashes
  a webhook definition after injecting its secret, so relay and host could
  never agree), and `manual` landed in C1 rather than C2 so a hand-run action
  has an honest trigger. The live proof is still owed: nothing has run
  against a deployed relay.*
- **C2 — project scoping, `ref_updated`, timezone, `actions.yml` publish.**
  `workflows.project_ref`, the `a` tag, `on_repo_state` at `transport.rs:2165`,
  `chrono-tz`. Proof: a push to `main` fires; a Friday 17:00 local schedule
  claims the right UTC instant across a DST boundary.
  *Status 2026-09-17: built (ledger 151). Amendments: the 30620 `d` is a UUID
  v5 of `<project>\n<name>` rather than a sha256, because the relay's `d` tag
  is a UUID; the roster has no "Admin" role, so a manual trigger admits Owner
  and Collaborator; a `ref_updated` trigger without `repository` matches the
  repository attached to the definition's project (`git_repo_names
  .project_ref`), not the project's own repository list; the `ci_result`
  carve-out stays C5. The push-to-`main` proof is engine-level against
  Postgres, not a live push.*
- **C3 — conditional and always routing to a persistent agent.** `wake_agent`,
  fenced 44220, 44246 row in the target session. Proof: a failing build wakes
  the builder exactly once across a duplicate 46013 delivery and a provider
  restart.
  *Status 2026-09-17: built (ledger 153). Amendments: `wake_agent` is a host
  step whose 46013 carries the earlier steps' outputs as `inputs` and no
  approval gate; the `on_success`/`on_failure` sugar is expanded by the
  actions-file parser before hashing (`<id>_success`, `<id>_failure`); the
  agent's name resolves to a role through `team.yml` and to that role's open
  execution on the routing host — no execution is created and no other host's
  execution is woken, both refused by name; the turn is a provider-signed
  44220 with a command id that is a pure function of run and step. The live
  proof is still owed.*
- **C4 — autorun grant and revoke.** `scope: action`, hash binding, 46032,
  46015, the inbox checkbox. Proof: editing the action re-arms approval.
- **C5 — ephemeral hire, agent-managed mode, `ci_result` trigger.** Proof:
  `hire_agent` seats a runner in the project manager's umbrella; a 46008
  failure wakes the project manager.
- **C6 — worktree at the triggering commit; artifact upload; `env_from_host`
  scrubbing audit.**

### Lanes and file ownership (for the build)

When this is built the pattern is the usual one: written slice → parallel
lanes with strict file ownership → full gate → adversarial review against the
named constraints (§ 4.5 rules, § 5.6 checks) → one finalizer commits. Natural
lane boundaries: `buzz-persona` (A1/A3), desktop host staging (A2), `buzz-core`
payload (A4), `buzz-workflow` + `buzz-db` schema (C1/C2 engine side),
`buzz-relay` handlers and git transport (C2), `buzz-session-provider` (C1/C3
host side), desktop Actions tab (C1/C4 UI).
