# Beekeeper — current state

Read this whole file first. It is gated to 300 lines and 24,000 bytes
(`just current-state-check`) and says where the project is and where to look.
Detail lives in the linked plan, the numbered ledger item, or the dated
report; read what your task links to, not everything it could. After a
compaction, re-read this file and the plan it names.

Every observation carries the date it was checked and, where re-runnable, the
command. A line without a date is a rule, not an observation. Where two
sources disagreed and the evidence could not settle it, the state is marked
unknown and both links kept. "Ledger §" means a heading in
[`SESSION_STATE.md`](SESSION_STATE.md); find it with `grep -n`.

## What is live (check dates in observations)

| Surface | Observed | How |
| --- | --- | --- |
| `main` on the relay | `a5be5c1a5`, checked 2026-09-20 17:58 EDT on both remotes; 180–200 landed gated bare, 189+ via `just push`. Read the remote, not a local ref | `GIT_TERMINAL_PROMPT=0 git ls-remote origin refs/heads/main` (`upstream` mirrors to GitHub) |
| Relay at hive.agiterra.org | `build_time` `2026-09-20T21:52:14Z`, `/health` `ok`, 17:58 EDT. hive self-deploys from a green pipeline ~25 min post-push ([`INTEGRATION.md`](INTEGRATION.md) § Deploying) | `curl -sH 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq .` |
| Installed Mac dev bundle | `~/Applications/Beekeeper Dev.app`, running `a5be5c1a5`; `bee 0.1.0 (a5be5c1a)`, installed 2026-09-20 18:00 EDT, signature verified. Live: granted lead reconnects (187–188), kettle terminal (178(o)) | `scripts/app-from.sh <sha>`; `bee --version` |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last claim recorded: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

The last product landing before Andy's two 2026-09-11 fixes was `0fc2beaf9`
on 2026-09-09, with `just ci` and `just test` green on that exact commit
(ledger § "September 9 main published").

## Active work and owners

| Work | Owner | State (check date in row) | Where |
| --- | --- | --- | --- |
| Project team setup and neutral baseline | Astra integrates Terra/Sol work | 2026-09-13: activation landed `1820d238d`; Tank Loop published and six roles installed; Loom ran with the adopted pack; walkthrough corrections landed `566e31075`. Existing-source maintenance deliberately refuses without provenance | [`PROJECT_TEAM_ACTIVATION_SLICE.md`](PROJECT_TEAM_ACTIVATION_SLICE.md), [activation evidence](history/2026-09-13-project-team-activation.md), [`PROJECT_TEAM_SETUP_IMPL.md`](PROJECT_TEAM_SETUP_IMPL.md) |
| Tank Loop setup walkthrough fixes | Opus built; Astra landed | 2026-09-13: landed at `566e31075`, installed; live UI acceptance pending | [walkthrough report](history/2026-09-13-tank-loop-walkthrough.md) |
| Project agents: association-scoped hiring, discovery and Agents/picker/setup UI (includes the Agents tab) | Opus built; Astra reviewed and landed | 2026-09-15: landed on both remotes as `d698c7773` and installed (row above). Root cause ledger 128; Astra's four review findings closed (129); the corrections review's public-to-private withdrawal blocker is 130, fixed in the landed tip. Live rollout not done: Beekeeper's agents must be explicitly associated before its leads hire | [`PROJECT_AGENT_HIRING_IMPL.md`](PROJECT_AGENT_HIRING_IMPL.md), [report](history/2026-09-14-project-agent-hiring.md), [rollout](history/2026-09-14-project-agent-rollout.md), [corrections review](history/2026-09-15-project-hiring-corrections-review.md), ledger 127–130 |
| Seat bundles outside the checkout: role skills materialize to an execution-owned directory, briefing names absolute paths, no `.agents/` in the seat tree | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack, installed, and **proven live** at 09:10 EDT: a Claude seat and a Codex seat each read their own pack's `marker-skill` and its `notes.md` from the bundle by absolute path, trees stayed clean, and the bundles survived a Cmd-Q relaunch unchanged (ledger 135, runbook filled in). That run's five host bugs are also 135, with their own rows below. Skills live in `<app data>/agents/seats/<session id>/skills/` with a manifest; the `.agents/` exclude write is gone | ledger 132, [experiment runbook](history/2026-09-15-seat-bundles-experiment.md) |
| Verification inputs name an exact commit: verifier and runner assignments carry `base_sha`, and the host establishes that commit in the seat's worktree | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; not exercised against a live seat. Finding and contract are ledger 131. The rule is strict at publication, tolerant of an *absent* `base_sha` for readers (one published verifier assignment, `0aaf33876f78`, would otherwise fail its mission's whole fold); a malformed value is refused everywhere. The relay validates 44244 at ingest through the reader path, so enforcement rests on conforming publishers at sign time. Establishment is **not** ordered against the assignee's wake — only the CLI wakes a seat — and the disclosure says so; the turn-level fence is the next slice | ledger 131, [`SESSION_STATE.md`](SESSION_STATE.md) |
| Seat bundles are cleaned up: a seat's bundle is removed when the host removes its worktree, and orphaned bundles are listed before any are removed | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; no real bundle removed yet. Closes both follow-ups of ledger 132; the finding is 134. The bundle goes with the **tree**, not with the build output, so a held tree keeps the skills its execution was running on. A record with no session id names no bundle and says so rather than guessing. Directory names, session-id sanitizer and path composition live once in `buzz-core`'s `coding_session_seat_bundle`, imported by provider and host, so creator/remover drift cannot be written. The manifest filename stayed in `buzz-persona`, which writes it | ledger 134, [`SESSION_STATE.md`](SESSION_STATE.md) |
| `HIRE_CHECKOUT_NOT_RECORDED` joins the contract; summary discloses *effective* runtime | Sonnet built; Fable landed | 2026-09-16: landed as `85062988c`, installing. Closes 136/138 outside-lane gaps; hiring/Agents tab now read `effective_runtime`, not the raw pin | ledger 139 |
| Two of the five 135 host bugs: the project Agents tab discloses runtime, `bee packs status` names its cache-dir source | Sonnet built; Fable landed | 2026-09-16: landed as `e28864c86`, installing. Each project-agent card shows the local record's runtime/model (`runtime not set` when absent) via `formatCodingSessionRuntimeLabel`; `bee packs status` reads `BUZZ_MANAGED_AGENT` before the hard-coded release identifier, and reports `cache_dir_source: "default" \| "env" \| "override"` instead of silently asserting the release path. | ledger 138, ledger 135(d)/(e) |
| Verification turns refuse an unestablished input: the provider will not open a verifier or runner assignment turn unless the seat's `HEAD` is the assignment's `base_sha` and the tree is clean | Opus built; Fable coordinated and landed | 2026-09-16: landed in the `1459c1186` stack and installed; unproven against a live wake. Fenced in the provider, the only party holding both the seat's checkout and the turn; five refusal codes, `base_sha` read as an `Option` so older assignments resolve. Tree or pointer facts refuse durably; a relay that cannot answer leaves the turn undecided, consuming nothing, bounded by the command horizon. Owed: binding the established commit into the receipt, a `buzz-core` key-set change | ledger 133, `crates/buzz-session-provider/src/verification_input.rs` |
| Hires cut from the project's checkout and honour the agent's runtime | Opus built; Fable landed | 2026-09-16: landed as `3e22f8ef8`, installed in `7fcc45a04`, **live at 12:57 EDT**: both Tank Loop hires succeeded first try, Kiln on Codex (ledger 136 addendum). Closes 135(a)/(b): no recorded repository folder is **refused** `HIRE_CHECKOUT_NOT_RECORDED`; runtime resolves record pin → effective harness → provider; the refusal code joined the contract in 139. Stale-cache hire refusal fixed 2026-09-19 (ledger 167) | ledger 136, 167 |
| Readiness reads the project's pack source, and a deleted session's worktrees are disposed of | Opus built; Fable landed | 2026-09-16: landed as `139e83334` (project name wired into the button in `7fcc45a04`), installing; nothing exercised against hive yet. Closes 135(c) and 135(f): "Use roles" reads the project's kind:30624 source through the staging code, `personas/roles` stays the fallback, the model registry downgrades to Limited where packs carry the runtime, and a whole-session deletion settles its trees as a closure does. Detail, including the new `bee sessions close`, is ledger 137 | ledger 137, `crates/buzz-core/src/worktree_lifecycle.rs`, `desktop/src-tauri/src/commands/team_readiness.rs` |
| Catalog coverage no longer fakes Unknown for an absent registry, and Unknown only gates Start when required for the first session | Sonnet built | 2026-09-16: gates green in `work/readiness-catalog-sonnet`, not landed. Closes the two ledger-137 contradictions Brian hit live 12:05 EDT | ledger 140 |
| Native mid-turn steering for Claude sessions | Fable/Opus; landed by Astra | 2026-09-13: static checks and a fresh E2E build passed; the selected browser matrix was 44 passed/1 failed, only dense history. The isolated dense repeat also failed (336/450; matrix 286/450), so diagnosis stays open. Brian accepted landing with that limitation; final CI succeeded; [recovered evidence](history/2026-09-13-final-ci-recovery.md) records every recipe leg. Push completed as `e12495c63`; Mac installation completed at `1820d238d`; installed UI acceptance remains pending | [startup evidence](history/2026-09-13-startup-smoke-corrections.md), [integration evidence](history/2026-09-12-steering-integration.md), `NATIVE_STEERING_IMPL.md` |
| Automatic project context beside ordinary agent work | paused by Brian 2026-09-10 | findings saved; next is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of step 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two) |
| Revealed-redaction badge as an icon; a streaming transcript resolves every marker | Andy | landed 2026-09-13 as `343ea8bd9`, included in current main | ledger § "Fixed 2026-09-13 — the revealed-redaction badge is an icon" |
| A Solo session's goal is one line: summarized by the naming model, clamped with a chevron | Andy | landed 2026-09-14 as `3fde2db15` | ledger § "Fixed 2026-09-14 — a Solo session's goal is one line" |
| The Dashboard shows the relay's machine (CPU, memory, disk) from `GET /health/system`, stewards only | Andy | landed 2026-09-14; on hive since 2026-09-18 (image `71efd0da1`), live check owed | ledger § "Built 2026-09-14 — the Dashboard shows the relay's machine" |
| Project teams, composable roles and project actions | Andy | 2026-09-18: A1–A4, C1–C6 built (ledger 142–148, 150–156), on hive; live runs owed. **Pivot**: roles and plans move to `<slug>-beekeeper-agents` (spec § 4.11), built (158–163); dev-stack retest fixed missing default agents (164) and the `buzz-agent` harness hire refusal (165); live proofs ran 2026-09-19 (171); the first RPG Test session found five host defects, all fixed and proved live that day (173–177): seats run bash, project agents join the roster, the code repo is seeded, cloned and recorded; setup-flow redesign open (§ 6 item 10) | [`PROJECT_TEAMS_AND_ACTIONS_SPEC.md`](PROJECT_TEAMS_AND_ACTIONS_SPEC.md), ledger 141, 158, D18–D19 |
| System proof: the acceptance story as a measured run | Brian with Fable; Astra audits | 2026-09-20 kettle run: landed in 10 min; goal→terminal 20,042 s; six repairs is an operator count — the wire shows 7 founder-signed non-host-answer commands (178, 178(r)); Andy's audited (179); 180–191 landed, mission closed (178(o)); 189–191 fix 178(j)/(k)/(n), see 178(p); 190's run-status endpoint is on hive (19:40Z). Owed: live proofs for 180, 186, 189–191; Actions tab / inbox approval (171 a, b); CI-result listener: no probe cadence (189) | ledger 178–179, [runbook](history/2026-09-20-system-proof-runbook.md), [audits](history/2026-09-20-astra-kettle-audit.md), [durable plan](history/2026-09-20-astra-durable-work-plan.md) |
| Durable work: governing plan | Fable finalizes; lanes build | 2026-09-20: **Wave 0 closed** after Astra's review (A2) — 192 host-answer fallback; 193 binds a run to its definition; 198 contract A2+A3; 199 two approval races, each reproduced 1→0; 200 exact tag match. **Wave 1 landed** `a5be5c1a5` 17:32 EDT, relay-first — 195 work records + relay admission for 44249, 13 contract sequences under permutation; 196 role templates 1.1.0 (existing projects take them at their next hire, `@^1.0.0`); 197 `bee sessions measure` reproduces both audits. Live: 44249 queries on hive; `workflows runs` still reads. Next: Wave 2 (W2 CLI, W3a provider preparation, W5 surface) | [`UNIFIED_WORK_PLAN.md`](UNIFIED_WORK_PLAN.md), ledger 192–200 |
| Project To-Do lists: kind 44248, personal/project visibility, pins, `bee todos`, Desktop tab + sidebar rows, Mobile page | Andy with Opus | 2026-09-17: on `main` (`0cbfcf296`); relay, CLI, Desktop, Mobile (iOS 26.5 sim) verified live | [`nips/NIP-TD.md`](nips/NIP-TD.md), ledger 149 |
| Delegated agent-directory visibility follow-up | unassigned | "can resume separately on main" (Astra, 2026-09-09) | ledger § "September 9 main published" |
| This map, the ledger split and its size gate | Fable | landed 2026-09-11 (`f80781969`); lead pack on `agiterra-packs` (`5f4ae76fa`) | `AGENTS.md` top block; `scripts/check-current-state-size.mjs` |

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
- **A project's roles and plans live in `<slug>-beekeeper-agents`, never in
  its code repository (Andy, 2026-09-18).** Spec § 4.11, ledger 158, D19.
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
  2026-09-11 against the installed adapters (ledger 114); owed is live use in
  an installed build with a real session; the completed gate and selected
  browser evidence are linked above. The queued-steer prevention race found
  during integration is corrected and has a failing-before/passing-after
  process test. codex-acp 1.6.2 has no idle guard and stays in boundary mode.
- **Provider outbox**: final relay refusals park, not retry (ledger 170); unproven live.
- **Automatic context** is blocked on Brian resuming it; the Claude hook
  transport is a candidate, not proven in the installed adapter.
- ~~**Unknown:** whether the 62 e2e-smoke failures that ledger §1 calls
  "inherited, not caused" (2026-08-19) still exist.~~ Settled 2026-09-12/13:
  six reproduced on base `77b792de9` (ledger 115), all nine later cases pass
  on the combined candidate, and only dense history still fails (44/1
  selected; isolated repeat 286/450 and 336/450), accepted by Brian while
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
3. Project agents slice (ledger 128–130) is landed and installed at
   `d698c7773`. Owed is the live
   [rollout](history/2026-09-14-project-agent-rollout.md): confirm Tank Loop's
   backfill, then explicitly associate Beekeeper's agents before its leads
   hire. The seat-isolation stack (four rows above, ledger 131–134) is landed
   and installed at `1459c1186`, and the
   [seat-bundles experiment](history/2026-09-15-seat-bundles-experiment.md)
   passed live 2026-09-16 (ledger 135); orchestration and what is still owed
   are in the
   [two-lane report](history/2026-09-15-seat-bundles-two-lane-orchestration.md).
   All five host bugs 135 found are fixed and landed (136–139), and Tank
   Loop's pack source is back at `f0132d1`. Next, in order: Brian re-runs a
   Tank Loop team hire on the installed build to see the new refusals, the
   "worktree cut from" line and the readiness panel live; then bind the
   established commit into the turn receipt (a `buzz-core` key-set change,
   ledger 133); the Claude write fence moves to `_meta` settings; a shared
   cargo target directory per repository to cut per-tree disk.
4. Diagnose the accepted dense-history limitation: the selected matrix was
   44 passed/1 failed (286/450 rows), its isolated repeat 336/450. No repeated
   full smoke marathon is required for the activation slice.
5. The lead pack is published in `agiterra-packs` at `5f4ae76fa`. Running
   seats retain their staged revision; use `bee packs status` to inspect a
   project's source before claiming a live seat has the new instructions.
6. Project teams spec: the pivot is built (ledger 158–165); rebuild and
   install the app, create a project on hive, run the live proofs
   (ledger 150–156, 160–165).
7. When Brian resumes automatic context, run the hook-marker experiment in the
   ledger's September 10 section. Collaborative workspace plan steps 5 and 6
   remain separate.

## Environment facts most likely to bite first

The full list is ledger §3a (~60 entries). These bite in the first hour.

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
  `GIT_TERMINAL_PROMPT=0`. The helper needs **git 2.46+**; a bundle on
  Apple's 2.39.5 failed every relay git op with
  `could not read Username` (ledger 168, 2026-09-19).
- `just desktop-check`, clippy with `--all-targets` and the file-size ratchet
  are what CI runs. Run them before claiming green.
- macOS has no `timeout`. pnpm 11 may reinstall a shared `node_modules` when
  a worktree's state file is stale; `PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN=false`
  prevents it, hook checks still run (pnpm 11.4.0, 2026-09-12).

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
