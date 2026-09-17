# Beekeeper — current state

Read this whole file first. It is gated to 300 lines and 24,000 bytes
(`just current-state-check`), and it says where the project is and where to
look. Detail lives in the linked plan, the numbered ledger item, or the dated
report; read what your task links to, not everything it could. After your
context is compacted, re-read this file and the plan it names.

Every observation carries the date it was checked and, where it can be re-run,
the command. A line without a date is a rule, not an observation. Where two
sources disagreed and the evidence could not settle it, the state is marked
unknown and both links are kept. "Ledger §" means a heading in
[`SESSION_STATE.md`](SESSION_STATE.md); find it with `grep -n`.

## What is live (check dates in observations)

| Surface | Observed | How |
| --- | --- | --- |
| `main` on the relay and on GitHub | `19b23b9d9`, checked 2026-09-16 13:30 EDT on both remotes by `ls-remote`; the seat-isolation stack (ledger 131–134), the five 135 fixes (136–139) and the readiness gate fix (140) landed, each after a green pre-push floor. The hot checkout's local `main` has lagged before; read the remote, not a local ref | `git fetch origin main` then `git rev-parse FETCH_HEAD`; `GIT_TERMINAL_PROMPT=0 git ls-remote upstream refs/heads/main` |
| Relay at hive.agiterra.org | `build_time` `2026-09-12T00:05:56Z`, checked 2026-09-13; `software_commit` is `unknown`, a disclosed non-answer from the stale deployer (ledger §3a, "hive's `software_commit` is `unknown`") | `curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq '{software_commit, build_time}'` |
| Installed Mac dev bundle | `/Users/brian/Applications/Beekeeper Dev.app`, running `19b23b9d9`; bundled CLI says `bee 0.1.0 (19b23b9d)` built `2026-09-16T17:15:59Z`; signature verified and app relaunched 2026-09-16 13:30 EDT. The seat-bundles experiment passed on `1459c1186` (ledger 135); the hire fixes were exercised live on `7fcc45a04` (ledger 136 addendum); the readiness fix (140) is installed but not yet exercised live; the walkthrough UI acceptance and the hiring rollout's live association step remain pending | `scripts/app-from.sh 19b23b9d9`; installed `bee --version`; `codesign --verify --deep --strict`; [walkthrough evidence](history/2026-09-13-tank-loop-walkthrough.md) |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last recorded claim: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

The last product landing before Andy's two 2026-09-11 fixes was `0fc2beaf9`
on 2026-09-09, with `just ci` and `just test` green on that exact commit
(ledger § "September 9 main published").

## Active work and owners

| Work | Owner | State (check date in row) | Where |
| --- | --- | --- | --- |
| Project team setup and neutral baseline | Astra integrates Terra/Sol work | 2026-09-13: activation landed as `1820d238d`; Tank Loop published and six roles installed; Loom ran with the adopted pack; walkthrough corrections landed at `566e31075`. Existing-source maintenance deliberately refuses without provenance | [`PROJECT_TEAM_ACTIVATION_SLICE.md`](PROJECT_TEAM_ACTIVATION_SLICE.md), [activation evidence](history/2026-09-13-project-team-activation.md), [`PROJECT_TEAM_SETUP_IMPL.md`](PROJECT_TEAM_SETUP_IMPL.md) |
| Tank Loop setup walkthrough fixes | Opus built; Astra reviews and integrates | 2026-09-13: landed on both remotes at `566e31075`; Mac bundle installed and relaunched, signature verified. Live UI acceptance remains pending. Founder view-only was a client false negative for a project owner in a project transport (confirmed live); fixed to mirror the relay's write rule. Installed project agents, lead picker, rename, saved-group demotion, setup stages and role-pack reconciliation guidance addressed. Integration closes the unlocked publication-read journal race; live roster refresh remains a separate UI follow-up | [walkthrough report](history/2026-09-13-tank-loop-walkthrough.md), [`TANK_LOOP_WALKTHROUGH_IMPL.md`](TANK_LOOP_WALKTHROUGH_IMPL.md) |
| Project agents: association-scoped hiring, discovery and Agents/picker/setup UI (includes the Agents tab) | Opus built; Astra reviewed and landed | 2026-09-15: landed on both remotes as `d698c7773` and installed (row above). Root cause ledger 128; Astra's four review findings closed (ledger 129); the public-to-private withdrawal blocker found in the corrections review is ledger 130 and is fixed in the landed tip. Live rollout not yet done: Beekeeper's agents must be explicitly associated before its leads hire | [`PROJECT_AGENT_HIRING_IMPL.md`](PROJECT_AGENT_HIRING_IMPL.md), [report](history/2026-09-14-project-agent-hiring.md), [rollout](history/2026-09-14-project-agent-rollout.md), [corrections review](history/2026-09-15-project-hiring-corrections-review.md), ledger 127–130 |
| Seat bundles outside the checkout: role skills materialize to an execution-owned directory, briefing names absolute paths, no `.agents/` in the seat tree | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack, installed, and **proven live** at 09:10 EDT: a Claude seat and a Codex seat each read their own pack's `marker-skill` and its `notes.md` from the bundle by absolute path, trees stayed clean, and the bundles survived a Cmd-Q relaunch unchanged (ledger 135, runbook results filled in). The run's five host bugs are ledger 135 and have their own rows below. Skills live in `<app data>/agents/seats/<session id>/skills/` with a manifest; the `.agents/` exclude write is gone | ledger 132, [experiment runbook](history/2026-09-15-seat-bundles-experiment.md) |
| Verification inputs name an exact commit: verifier and runner assignments must carry `base_sha`, and the host establishes that commit in the seat's worktree | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; not exercised against a live seat. Finding and contract are ledger 131. The assignment rule is strict at publication and tolerant of an *absent* `base_sha` for readers, because one already-published verifier assignment (`0aaf33876f78`) would otherwise fail its whole mission's fold; a malformed value is still refused everywhere. The relay validates 44244 at ingest through the reader path, so enforcement rests on conforming publishers at sign time. Establishment is **not** ordered against the assignee's wake — only the CLI wakes a seat — and the disclosure says so; the real turn-level fence is the next slice | ledger 131, [`SESSION_STATE.md`](SESSION_STATE.md) |
| Seat bundles are cleaned up: a seat's skill bundle is removed when the host removes its worktree, and orphaned bundles are listed before any are removed | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; no real bundle has been removed yet. Closes both follow-ups of ledger 132; the finding is ledger 134. The bundle goes with the **tree**, not with the build output, so a held tree keeps the skills its execution was running on. A record with no session id names no bundle and says so rather than guessing. The directory names, the session-id sanitizer and the path composition now live once in `buzz-core`'s `coding_session_seat_bundle`; the provider and the host both import them, so a drift between creator and remover cannot be written. The manifest filename stayed in `buzz-persona`, which writes it | ledger 134, [`SESSION_STATE.md`](SESSION_STATE.md) |
| `HIRE_CHECKOUT_NOT_RECORDED` joins the contract; summary discloses *effective* runtime | Sonnet built; Fable landed | 2026-09-16: landed as `85062988c`, installing. Closes 136/138 outside-lane gaps; hiring/Agents tab now read `effective_runtime`, not the raw pin | ledger 139 |
| Two of the five 135 host bugs: the project Agents tab discloses runtime, `bee packs status` names its cache-dir source | Sonnet built; Fable landed | 2026-09-16: landed as `e28864c86`, installing. Each project-agent card now shows the local record's runtime/model (`runtime not set` when absent), reusing `formatCodingSessionRuntimeLabel`; `bee packs status` reads `BUZZ_MANAGED_AGENT` before falling back to the hard-coded release identifier, and reports `cache_dir_source: "default" \| "env" \| "override"` instead of asserting the release path silently. Card lives in `project-agents/`, not `projects-container/` as briefed — no `projects-container` file changed | ledger 138, ledger 135(d)/(e) |
| Verification turns refuse an unestablished input: the provider will not open a verifier or runner assignment turn unless the seat's `HEAD` is the assignment's `base_sha` and the tree is clean | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; unproven against a live wake. Fenced in the provider because it is the only party holding both the seat's checkout and the turn; five refusal codes, `base_sha` read as an `Option` so older assignments still resolve. Facts about the tree or the pointer refuse durably; a relay that cannot answer leaves the turn undecided, consuming nothing, bounded by the command horizon. Owed: binding the established commit to the assignment in the receipt, which needs a `buzz-core` key-set change | ledger 133, `crates/buzz-session-provider/src/verification_input.rs` |
| Hires cut from the project's checkout and honour the agent's runtime | Opus built; Fable landed | 2026-09-16: landed as `3e22f8ef8`, installed in `7fcc45a04`, and **exercised live at 12:57 EDT**: both Tank Loop hires succeeded first try, Kiln on Codex, both worktrees cut from the project checkout (ledger 136 addendum). Closes ledger 135(a) and (b): the checkout rule is one pure function that has no `mru` parameter, a project session with no recorded repository folder is **refused** `HIRE_CHECKOUT_NOT_RECORDED` naming Project settings → This computer → Repository folder rather than seated on a guess, and the seat's create is preceded by an umbrella line saying which folder it was cut from. An agent's runtime is resolved record pin → effective harness → provider, the way the Agents screen resolves it, so an agent that inherits its harness from its persona is no longer seated on Claude. Desktop tests only; no live hire exercised. The refusal code joined the contract in ledger 139 | ledger 136 |
| Readiness reads the project's pack source, and a deleted session's worktrees are disposed of | Opus built; Fable landed | 2026-09-16: landed as `139e83334` (project name wired into the button in `7fcc45a04`), installing; nothing exercised against hive yet. Closes ledger 135(c) and 135(f): "Use roles" reads the project's kind:30624 source through the staging code, `personas/roles` stays the fallback, the model registry downgrades to Limited where packs carry the runtime, and a whole-session deletion settles its trees as a closure does. Detail, including the new `bee sessions close`, is ledger 137 | ledger 137, `crates/buzz-core/src/worktree_lifecycle.rs`, `desktop/src-tauri/src/commands/team_readiness.rs` |
| Catalog coverage no longer fakes Unknown for an absent registry, and Unknown only gates Start when required for the first session | Sonnet built | 2026-09-16: gates green in `work/readiness-catalog-sonnet`, not landed. Closes the two ledger-137 contradictions Brian hit live 12:05 EDT | ledger 140 |
| Native mid-turn steering for Claude sessions | Fable/Opus; landed by Astra | 2026-09-13: static checks and fresh E2E build passed; selected browser matrix was 44 passed/1 failed, only dense history. Isolated dense repeat also failed (336/450; matrix 286/450), so diagnosis remains open. Brian accepted landing with that limitation; final CI completed successfully; [recovered evidence](history/2026-09-13-final-ci-recovery.md) records every recipe leg. Push completed as `e12495c63`; Mac installation completed at `1820d238d`; installed UI acceptance remains pending | [startup evidence](history/2026-09-13-startup-smoke-corrections.md), [integration evidence](history/2026-09-12-steering-integration.md), `NATIVE_STEERING_IMPL.md` |
| Automatic project context beside ordinary agent work | paused by Brian on 2026-09-10 | findings saved; next step is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, ordered steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of steps 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two items) |
| Revealed-redaction badge as an icon; a streaming transcript resolves every marker | Andy | landed 2026-09-13 as `343ea8bd9`, included in current main | ledger § "Fixed 2026-09-13 — the revealed-redaction badge is an icon" |
| A Solo session's goal is one line: summarized by the naming model, clamped with a chevron | Andy | landed 2026-09-14 as `3fde2db15` | ledger § "Fixed 2026-09-14 — a Solo session's goal is one line" |
| The Dashboard shows the relay's machine (CPU, memory, disk) from `GET /health/system`, stewards only | Andy | landing 2026-09-14 on `feat/relay-health`; hive shows it once the relay redeploys | ledger § "Built 2026-09-14 — the Dashboard shows the relay's machine" |
| Project teams, composable roles and project actions | Andy | 2026-09-17: A1–A3 landed `1395770ff` (ledger 142–148); restart needs a relay redeploy; no live hire yet | [`PROJECT_TEAMS_AND_ACTIONS_SPEC.md`](PROJECT_TEAMS_AND_ACTIONS_SPEC.md), ledger 141, D18 |
| Project To-Do lists: kind 44248, `bee todos`, Desktop tab, Mobile page | Andy with Opus | 2026-09-17: built on `feat/project-todo-list`; relay, CLI and Desktop verified live on a local relay; landing pending | [`nips/NIP-TD.md`](nips/NIP-TD.md), ledger 149 |
| Delegated agent-directory visibility follow-up | unassigned | "can resume separately on main" (Astra, 2026-09-09) | ledger § "September 9 main published" |
| This map, the ledger split and its size gate | Fable | landed 2026-09-11 (`f80781969`); lead pack landed on `agiterra-packs` (`5f4ae76fa`) | `AGENTS.md` top block; `scripts/check-current-state-size.mjs` |

## Decisions in force

Rules, each with where it is written down.

- **Project teams are optional (Brian, 2026-09-11).** Ordinary Solo sessions
  require neither setup nor a dedicated lead/managed identity, before or after
  a project team exists. See `PROJECT_TEAM_SETUP_IMPL.md`.
- **Git is relay-canonical.** Push to `origin` (hive) only; the bridge mirrors
  GitHub. Topic branches rebase onto `main`, never merge; `vanilla/main`
  merges, never rebases. Commit with `-s`. `AGENTS.md` top block,
  [`INTEGRATION.md`](INTEGRATION.md).
- **No human gates a landing.** `main` is admitted by the relay's push gate
  from observed gate rows and verifier verdicts. A refusal names a missing
  fact; the remedy is a command or a hire, never a person. Beekeeper’s specialized project pack in `agiterra-packs`; this is not
  a policy imposed by the neutral shipped foundation.
- **Honesty in the product is a first-class concern.** A control that lies
  about what it enforces is a bug of crash severity. `AGENTS.md` § Working
  agreements.
- **Findings go into the ledger as numbered items; this map is updated, never
  appended; session reports go under `docs/history/`.** `AGENTS.md` top block.
- **Identities are durable and named, seats are ephemeral, role packs are
  signed and versioned relay records** (D11–D16). Plan § "Decisions already
  made"; `VISION_COLLABORATION.md` § "Roles evolve with the project".
- **"Crew" is "team"** in Beekeeper’s user-facing copy. This is a Beekeeper
  product convention, not a mandatory vocabulary for other projects.
- **Product authorities:** `VISION.md`, `VISION_COLLABORATION.md`,
  [`SESSION_VISION.md`](SESSION_VISION.md), and the table in ledger §4.

## Blockers and owed confirmations

- **Live confirmations still owed** are the list under the ledger heading of
  that name: the write sides of items 104–108, item 1's duplicate-create
  re-test on a current build, the 2026-08-19 rehydration runs, the seat case
  of `bee git check`. Run one, then strike it there.
- **Windows acceptance** is blocked on hardware (above).
- **hive's `software_commit` is `unknown`** until the deployer is refreshed;
  `/health` prints `ok unknown`. Read it as a disclosed non-answer, not a
  failure.
- ~~**Native steering** is blocked on proving adapter support.~~ Proven
  2026-09-11 against the installed adapters (ledger item 114); what remains
  owed is live use in an installed build with a real
  session; the completed gate and selected browser evidence are linked above. The queued-steer prevention race found during integration is corrected
  and has a failing-before/passing-after process test. codex-acp 1.6.2 has no idle guard and stays in boundary mode.
- **Automatic context** is blocked on Brian resuming it; the Claude hook
  transport is a candidate, not proven in the installed adapter.
- ~~**Unknown:** whether the 62 e2e-smoke failures that ledger §1 calls
  "inherited, not caused" (2026-08-19) still exist.~~ Settled 2026-09-12/13:
  six reproduced on base `77b792de9` (ledger item 115), all nine later cases
  pass on the combined candidate, and only dense history still fails (44/1
  selected; isolated repeat 286/450 and 336/450), which Brian accepted while
  diagnosis continues — [September 13 evidence](history/2026-09-13-startup-smoke-corrections.md).
  Never run smoke while CI builds the desktop: the plain build overwrites
  the E2E bundle. Prevent idle sleep during unattended checks.

## Next, in order

The steering/startup/setup-authoring stack is published on main. The activation
slice is published as `1820d238d`; it preserves ordinary Solo sessions and
separates publication, installation and provider-confirmed lead startup.

1. Brian completed Tank Loop publication and six-role installation, then ran
   Loom at the adopted role revision. The subsequent walkthrough corrections
   landed and are installed at `566e31075`; bundled version and signature verified.
   See the [walkthrough report](history/2026-09-13-tank-loop-walkthrough.md).
2. After installation, reopen Project State Planning as Brian and confirm
   control without joining its transport channel. Check Tank Loop's installed
   agents, Loom's default selection and setup progress. Existing-source pack
   maintenance and automatic setup completion remain deferred. Tank Loop's
   already-published role text is unchanged.
3. Project agents slice (ledgers 128–130) is landed and installed at
   `d698c7773`. What remains is the live
   [rollout](history/2026-09-14-project-agent-rollout.md): confirm Tank Loop's
   backfill, then explicitly associate Beekeeper's agents before its leads
   hire. The seat-isolation stack (four rows above, ledger 131–134) is
   landed and installed at `1459c1186`, and the
   [seat-bundles experiment](history/2026-09-15-seat-bundles-experiment.md)
   passed live on 2026-09-16 (ledger 135). The orchestration and what is
   still owed are in the
   [two-lane report](history/2026-09-15-seat-bundles-two-lane-orchestration.md).
   All five host bugs 135 found are fixed and landed (ledger 136–139), and
   Tank Loop's pack source is back at `f0132d1`. Next, in order: Brian
   re-runs a Tank Loop team hire on the installed build to see the new
   refusals, the "worktree cut from" line and the readiness panel live;
   then bind the established commit into the turn receipt (a `buzz-core`
   key-set change, ledger 133); the Claude write fence moves to `_meta`
   settings; a shared cargo target directory per repository to cut
   per-tree disk.
4. Diagnose the accepted dense-history limitation after landing: the selected
   matrix was 44 passed/1 failed (286/450 rows), and its isolated repeat reached
   336/450. No repeated full smoke marathon is required for the activation slice.
5. The lead pack is already published in `agiterra-packs` at `5f4ae76fa`.
   Running seats retain their staged revision; use `bee packs status` to inspect
   a project's source before claiming that a live seat has the new instructions.
6. Project teams spec: A1–A3 landed (row above); redeploy the relay, run a
   live hire and restart on the installed app, then A4 and C1.
7. When Brian resumes automatic context, run the hook-marker experiment in the
   ledger's September 10 section. Collaborative workspace plan steps 5 and 6
   remain separate.

## Environment facts most likely to bite first

The full list is ledger §3a, about sixty entries. These are the ones a fresh
agent hits in the first hour.

- The main checkout at `/Users/brian/Projects/beekeeper/beekeeper` is Brian's
  live dev checkout. Never rebase or switch branches there while a dev server
  runs; `pgrep -fl 'tauri dev'` says. On 2026-09-11 the running app was the
  installed bundle, not a dev server.
- `bee` on `PATH` is a stale build. Use `./target/debug/bee` or the bundle's.
- Activate hermit as its own command, `. ./bin/activate-hermit`, before
  cargo, pnpm, just or hooks; or call `bin/cargo` directly.
- The host records a gate row only for a bare command run after
  `git commit`. A pipe or a redirect on the line records nothing, silently.
- Pushing to `origin` needs `just install-git-credentials`; `bee git status`
  says whether it is wired. Without it a fetch hangs on a prompt: set
  `GIT_TERMINAL_PROMPT=0`.
- `just desktop-check`, clippy with `--all-targets` and the file-size ratchet
  are what CI runs. Run them before claiming green.
- macOS has no `timeout`. pnpm 11 may reinstall a shared `node_modules` when
  a worktree's state file is stale. For already-installed matching dependencies,
  `PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN=false` prevents that automatic install;
  the actual hook checks still run. Verified with pnpm 11.4.0 on 2026-09-12.

## Where evidence lives

| Kind | Place | Tracked |
| --- | --- | --- |
| Numbered findings, items 1–149 and counting | `SESSION_STATE.md` §2; `grep -n '^<n>\. ' docs/SESSION_STATE.md` | yes |
| Session reports 2026-09-06 to 2026-09-11 | the top of `SESSION_STATE.md`, above §1; written before this map and left in place | yes |
| Session reports from 2026-09-11 on | [`history/`](history/README.md), `YYYY-MM-DD-<slug>.md` | yes |
| Plans and specs | `COLLABORATIVE_WORKSPACE_PLAN.md`, `*_SPEC.md`, `*_IMPL.md` in `docs/` | yes |
| Lane logs, build logs, acceptance transcripts | `../review-*/` beside the repo | no, this machine only |
| Astra-to-Fable mailbox and briefs | `~/Desktop/BEEKEEPER-*.md` | no, this machine only |

Untracked evidence may be cited, but a claim that rests on it alone says so.

## Maintaining this file

Update it in the same commit as any change that alters what is deployed, the
active work, a decision, a blocker or the next steps. Keep it under 300 lines
and 24,000 bytes; `just current-state-check` fails otherwise, and the answer
is to move detail out, never to raise the limit. Date every observation. When
this file and the ledger disagree, check the code or the relay, record the
result here with the date, and strike the losing claim where it stands. If
the evidence cannot settle it, write unknown and keep both links.
