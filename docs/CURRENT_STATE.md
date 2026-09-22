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
| `main` on the relay | `dec7f2d3d`, checked 2026-09-21 22:40 EDT; 231–232 landed on the A9–A11 plan commits, gates bare and green (`/tmp/gate-final-232.log`), pushed with the floor (`/tmp/push-final-232.log`, run after this commit); prior tip `f4bfdffd4` (227–228). Read the remote, not a local ref | `GIT_TERMINAL_PROMPT=0 git ls-remote origin refs/heads/main` (`upstream` mirrors to GitHub) |
| Relay at hive.agiterra.org | **`build_time` `2026-09-21T22:24:42Z`, the CI-green deploy of `f4bfdffd4`**, checked 2026-09-21 18:35 EDT; the prior builds were `2026-09-21T19:05:50Z` (`2708f64a8`) and `15:46:41Z` (`aa055b233`). A changed `build_time` is the only deploy signal — `software_commit` and `/health` both answer `unknown` here | `curl -sH 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq .`; `curl -s https://hive.agiterra.org/health` |
| Installed Mac dev bundle | `~/Applications/Beekeeper Dev.app`, running `f4bfdffd4` (count 3726); `bee 0.1.0 (f4bfdffd)` built 22:29:49Z, installed 2026-09-21 18:32 EDT (`/tmp/app-from-228-install.log`). Live: run `8f9552c9` reports `checkout` `6ea2e0db…` with `checkout_reported: true` (206) | `scripts/app-from.sh <sha>`; `bee --version` |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last claim recorded: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

## Active work and owners

| Work | Owner | State (check date in row) | Where |
| --- | --- | --- | --- |
| Project team setup and neutral baseline | Astra integrates Terra/Sol work | 2026-09-13: activation landed `1820d238d`; Tank Loop published and six roles installed; Loom ran with the adopted pack; walkthrough corrections landed `566e31075`. Existing-source maintenance deliberately refuses without provenance | [`PROJECT_TEAM_ACTIVATION_SLICE.md`](PROJECT_TEAM_ACTIVATION_SLICE.md), [activation evidence](history/2026-09-13-project-team-activation.md), [`PROJECT_TEAM_SETUP_IMPL.md`](PROJECT_TEAM_SETUP_IMPL.md) |
| Tank Loop setup walkthrough fixes | Opus built; Astra landed | 2026-09-13: landed at `566e31075`, installed; live UI acceptance pending | [walkthrough report](history/2026-09-13-tank-loop-walkthrough.md) |
| Project agents: association-scoped hiring, discovery and Agents/picker/setup UI (includes the Agents tab) | Opus built; Astra reviewed and landed | 2026-09-15: landed as `d698c7773`, installed. Root cause 128; Astra's findings closed (129); the public-to-private withdrawal blocker is 130, fixed in the landed tip. Live rollout not done: Beekeeper's agents must be associated before its leads hire | [`PROJECT_AGENT_HIRING_IMPL.md`](PROJECT_AGENT_HIRING_IMPL.md), [rollout](history/2026-09-14-project-agent-rollout.md), [corrections review](history/2026-09-15-project-hiring-corrections-review.md), ledger 127–130 |
| Seat bundles outside the checkout: role skills materialize to `<app data>/agents/seats/<session id>/skills/`, briefing names absolute paths | Opus built; Fable landed | 2026-09-16: landed in the `1459c1186` stack, installed, **proven live** on a Claude and a Codex seat (ledger 135; its five host bugs have rows below) | ledger 132, [runbook](history/2026-09-15-seat-bundles-experiment.md) |
| Verification inputs name an exact commit (`base_sha` on verifier/runner assignments; the host establishes it in the seat's worktree) | Opus built; Fable landed | 2026-09-16: landed in the `1459c1186` stack and installed; not exercised against a live seat. Strict at publication, tolerant of an absent value for readers (one legacy assignment); establishment is not ordered against the wake — the turn-level fence is the row two below | ledger 131 |
| Seat bundles are cleaned up with the worktree; orphans are listed before removal | Opus built; Fable landed | 2026-09-16: landed in the `1459c1186` stack and installed; no real bundle removed yet. Names and path composition live once in `buzz-core`'s `coding_session_seat_bundle` | ledger 134 |
| `HIRE_CHECKOUT_NOT_RECORDED` joins the contract; summary discloses *effective* runtime | Sonnet built; Fable landed | 2026-09-16: landed as `85062988c`, installing. Closes 136/138 outside-lane gaps; hiring/Agents tab now read `effective_runtime`, not the raw pin | ledger 139 |
| Two of the five 135 host bugs: the project Agents tab discloses runtime, `bee packs status` names its cache-dir source | Sonnet built; Fable landed | 2026-09-16: landed as `e28864c86`, installing. Each project-agent card shows the local record's runtime/model (`runtime not set` when absent); `bee packs status` reads `BUZZ_MANAGED_AGENT` before the hard-coded release identifier and reports `cache_dir_source` instead of silently asserting the release path | ledger 138, ledger 135(d)/(e) |
| Verification turns refuse an unestablished input (seat `HEAD` must be the assignment's `base_sha`, tree clean) | Opus built; Fable landed | 2026-09-16: landed in the `1459c1186` stack and installed; unproven against a live wake. Owed: binding the established commit into the receipt (a `buzz-core` key-set change) | ledger 133, `crates/buzz-session-provider/src/verification_input.rs` |
| Hires cut from the project's checkout and honour the agent's runtime | Opus built; Fable landed | 2026-09-16: landed as `3e22f8ef8`, installed in `7fcc45a04`, **live at 12:57 EDT**: both Tank Loop hires succeeded first try, Kiln on Codex (ledger 136 addendum). Closes 135(a)/(b): no recorded repository folder is **refused** `HIRE_CHECKOUT_NOT_RECORDED`; runtime resolves record pin → effective harness → provider; the refusal code joined the contract in 139. Stale-cache hire refusal fixed 2026-09-19 (ledger 167) | ledger 136, 167 |
| Readiness reads the project's pack source, and a deleted session's worktrees are disposed of | Opus built; Fable landed | 2026-09-16: landed as `139e83334` (project name wired into the button in `7fcc45a04`), installing; nothing exercised against hive yet. Closes 135(c) and 135(f): "Use roles" reads the project's kind:30624 source through the staging code, `personas/roles` stays the fallback, and a whole-session deletion settles its trees as a closure does | ledger 137, `crates/buzz-core/src/worktree_lifecycle.rs` |
| Catalog coverage no longer fakes Unknown for an absent registry, and Unknown only gates Start when required for the first session | Sonnet built | 2026-09-16: gates green in `work/readiness-catalog-sonnet`, not landed. Closes the two ledger-137 contradictions Brian hit live 12:05 EDT | ledger 140 |
| Native mid-turn steering for Claude sessions | Fable/Opus; landed by Astra | 2026-09-13: landed `e12495c63`, installed at `1820d238d`; installed UI acceptance pending. Static checks and a fresh E2E build passed; only dense history still fails (selected matrix 44/1, 286/450 rows; isolated repeat 336/450), accepted by Brian with diagnosis open. [Recovered CI evidence](history/2026-09-13-final-ci-recovery.md) records every recipe leg | [startup evidence](history/2026-09-13-startup-smoke-corrections.md), [integration evidence](history/2026-09-12-steering-integration.md), `NATIVE_STEERING_IMPL.md` |
| Automatic project context beside ordinary agent work | paused by Brian 2026-09-10 | findings saved; next is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of step 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two) |
| Revealed-redaction badge as an icon; a streaming transcript resolves every marker | Andy | landed 2026-09-13 as `343ea8bd9`, included in current main | ledger § "Fixed 2026-09-13 — the revealed-redaction badge is an icon" |
| A Solo session's goal is one line: summarized by the naming model, clamped with a chevron | Andy | landed 2026-09-14 as `3fde2db15` | ledger § "Fixed 2026-09-14 — a Solo session's goal is one line" |
| The Dashboard shows the relay's machine (CPU, memory, disk) from `GET /health/system`, stewards only | Andy | landed 2026-09-14; on hive since 2026-09-18 (image `71efd0da1`), live check owed | ledger § "Built 2026-09-14 — the Dashboard shows the relay's machine" |
| Project teams, composable roles and project actions | Andy | 2026-09-18: A1–A4, C1–C6 built (ledger 142–148, 150–156), on hive; live runs owed. **Pivot**: roles and plans move to `<slug>-beekeeper-agents` (spec § 4.11), built (158–163); dev-stack retest fixed missing default agents (164) and the `buzz-agent` harness hire refusal (165); live proofs ran 2026-09-19 (171); the first RPG Test session found five host defects, all fixed and proved live that day (173–177): seats run bash, project agents join the roster, the code repo is seeded, cloned and recorded; setup-flow redesign open (§ 6 item 10) | [`PROJECT_TEAMS_AND_ACTIONS_SPEC.md`](PROJECT_TEAMS_AND_ACTIONS_SPEC.md), ledger 141, 158, D18–D19 |
| System proof: the acceptance story as a measured run | Brian with Fable; Astra audits | 2026-09-20 kettle run: landed in 10 min; goal→terminal 20,042 s; six repairs is an operator count — the wire shows 7 founder-signed commands (178, 178(r)); Andy's audited (179); 180–191 landed, mission closed (178(o)); 189–191 fix 178(j)/(k)/(n), see 178(p); 190's run-status endpoint is on hive (19:40Z). Run two (205) proved 180, 186, 189, 190 and 171(c) live; 203 closed 171(a)/(b). Owed: 191's proof, the CI listener's probe cadence (189) | ledger 178–179, [runbook](history/2026-09-20-system-proof-runbook.md), [audits](history/2026-09-20-astra-kettle-audit.md), [durable plan](history/2026-09-20-astra-durable-work-plan.md) |
| Durable work: governing plan | Fable finalizes; lanes build | 2026-09-21: Waves 0–2 closed (192–207; Wave 2 `228da8f9c`, 206–207 `942eb00dc`). **Wave 3** landed as `e8c552cf6`, then 212(h)+217 as `ce6df5633`, CI green and deployed 13:44:49Z: 208 flakes; 209 the host-assembled work brief; 210 a no-ask approving disposition settles without an ACK; 211 approval truth; 212 four preparation fences; 214/213 contract A5/A6; 215–216 shared vectors, every strict reader agreeing with `buzz-core`; 217 five more flaky files. **Astra's Wave 2 re-check held R1–R5**; the rulings are plan § 8 A7 and the fixes landed as **218–223** (native read hashes its own bytes; custody belongs to the turn; a deferred wake is re-admitted; the oracle and the fold that earns it; raw-content vectors, `web/` joined), deployed and installed at `aa055b233`. **Astra's third look closed R1/R3/R5 and held R2/R4**; rulings § 8 A8, fixed as **227–228** at `d209432aa` (built as lanes 224/225 and renumbered on landing — `origin/main` had claimed 224–226): 227 one custody identity per seat, the holder ended not replaced, and an aborted actor's process group proven dead or the seat stays fenced; 228 every admission refusal stages its answer before the door shuts. **231–232 landed tonight**: templates 1.2.0, and the five root-bypassable desktop tests fixed by construction. Astra's [fourth look](history/2026-09-21-astra-wave2-fourth-look.md) is filed and **R2/R4 stay open** — lanes 229/230 in flight (refuter, then Astra, before landing), as is drift disclosure 234/235. Next per plan § 8 A11: the `kettle-control` run on this build, a measurement and not a safety claim | [`UNIFIED_WORK_PLAN.md`](UNIFIED_WORK_PLAN.md) § 8 A8, [third look](history/2026-09-21-astra-wave2-third-look.md), ledger 192–228 |
| Project To-Do lists: kind 44248, personal/project visibility, pins, `bee todos`, Desktop tab + sidebar rows, Mobile page | Andy with Opus | 2026-09-17: on `main` (`0cbfcf296`); relay, CLI, Desktop, Mobile (iOS 26.5 sim) verified live | [`nips/NIP-TD.md`](nips/NIP-TD.md), ledger 149 |
| Shared drafts for the agents repository: kind 44250 (renumbered from 44249 on landing; NIP-PW took 44249), `bee agents-repo`/`bee plans`, relay `tree`/`raw`, desktop **Files** tab, mobile Files page | Andy with Opus | 2026-09-21: landing on `main`; Andy drafted, committed and created a plan in the dev app on a local relay; `bee` cycle and mobile reader proved live there; nothing against hive yet | [`nips/NIP-AD.md`](nips/NIP-AD.md), spec § 4.12, ledger 224–226, [run](history/2026-09-21-agents-repo-drafts.md) |
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
- **Edits to the agents repository are shared drafts through the relay;
  nothing is real until someone commits (Andy, 2026-09-21).** Spec § 4.12,
  NIP-AD, ledger 224.
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
  an installed build with a real session, evidence linked above. The
  queued-steer race is corrected with a failing-before/passing-after process
  test. codex-acp 1.6.2 has no idle guard and stays in boundary mode.
- ~~**Five desktop tests make their failure out of a permission, which root
  bypasses** (ledger 212(h)).~~ Fixed 2026-09-21 by lane 232: each fails by
  construction (`EISDIR`), which no uid bypasses; grep found no others (232).
- **A webhook-triggered action cannot be approved from its card (2026-09-21,
  ledger 218).** Its stored hash covers a secret the public event does not
  carry, so the card answers `definition_hash: null` with the reason and
  offers no grant, rather than a hash matching nothing; an unparsable
  definition withholds one the same way. No lane owns the approval path.
- **Provider outbox**: final relay refusals park, not retry (ledger 170); unproven live.
- **Automatic context** is blocked on Brian resuming it; the Claude hook
  transport is a candidate, not proven in the installed adapter.
- ~~**Unknown:** whether the 62 e2e-smoke failures ledger §1 calls
  "inherited, not caused" (2026-08-19) still exist.~~ Settled 2026-09-12/13:
  six reproduced on base `77b792de9` (ledger 115), the nine later cases pass
  on the combined candidate, and only dense history still fails (44/1
  selected; isolated repeat 286/450 and 336/450), accepted by Brian while
  diagnosis continues — [evidence](history/2026-09-13-startup-smoke-corrections.md).
  Never run smoke while CI builds the desktop: the plain build overwrites the
  E2E bundle. Prevent idle sleep during unattended checks.

## Next, in order

The steering/startup/setup-authoring stack is published on main. The activation
slice is published as `1820d238d`; it preserves ordinary Solo sessions and
separates publication, installation and provider-confirmed lead startup.

1. Done: Tank Loop publication, six-role installation, the Loom run at the
   adopted revision, walkthrough corrections installed at `566e31075`.
2. After installation, reopen Project State Planning as Brian and confirm
   control without joining its transport channel; check Tank Loop's installed
   agents, Loom's default selection and setup progress. Existing-source pack
   maintenance and automatic setup completion remain deferred.
3. Project agents (ledger 128–130) is installed at `d698c7773`; the live
   [rollout](history/2026-09-14-project-agent-rollout.md) is owed — confirm
   Tank Loop's backfill, then associate Beekeeper's agents before its leads
   hire. The seat-isolation stack (131–134) is installed at `1459c1186` and the
   [seat-bundles experiment](history/2026-09-15-seat-bundles-experiment.md)
   passed live 2026-09-16 (135); orchestration is in the
   [two-lane report](history/2026-09-15-seat-bundles-two-lane-orchestration.md).
   All five host bugs 135 found are fixed (136–139) and Tank Loop's pack source
   is back at `f0132d1`. Next, in order: Brian re-runs a Tank Loop team hire on
   the installed build; bind the established commit into the turn receipt (a
   `buzz-core` key-set change, 133); move the Claude write fence to `_meta`
   settings; share one cargo target directory per repository.
4. Diagnose the accepted dense-history limitation (counts in the blocker
   above). No repeated full smoke marathon is required for the activation slice.
5. The lead pack is published in `agiterra-packs` at `5f4ae76fa`. Running
   seats retain their staged revision; use `bee packs status` to inspect a
   project's source before claiming a live seat has the new instructions.
6. Project teams spec: the pivot is built (ledger 158–165); rebuild and
   install the app, create a project on hive, run the live proofs
   (ledger 150–156, 160–165).
7. When Brian resumes automatic context, run the hook-marker experiment in the
   ledger's September 10 section. Collaborative workspace plan steps 5 and 6
   remain separate.
8. Durable work: 227–228 landed at `d209432aa`, 231–232 tonight at `dec7f2d3d`.
   The fourth look holds R2/R4; lanes 229/230 and drift 234/235 are in flight.
   Next is the `kettle-control` run on this build, which per plan § 8 A11
   measures the shipped templates and claims no safety. The empty-set waiver at
   `project_work_fold.rs:419` stays documented: an observation, not a lane.

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
| Lane logs, gate logs, acceptance transcripts | `/tmp/gate-*.log`, `../review-*/` (the older ones were deleted 2026-09-16); mailbox `~/Desktop/BEEKEEPER-*.md` | no, this machine only |

Untracked evidence may be cited, but a claim that rests on it alone says so.

## Maintaining this file

Update it in the same commit as any change that alters what is deployed, the
active work, a decision, a blocker or the next steps. Keep it under 300 lines
and 24,000 bytes; `just current-state-check` fails otherwise, and the answer
is to move detail out, never to raise the limit. Date every observation. When
this file and the ledger disagree, check the code or the relay, record the
result here with the date, and strike the losing claim where it stands. If
the evidence cannot settle it, write unknown and keep both links.
