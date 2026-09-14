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
| `main` on the relay and on GitHub | `566e31075`, checked 2026-09-13 on relay; walkthrough corrections and serialized publication reads landed; both remotes verified | `git fetch origin main` then `git rev-parse FETCH_HEAD`; `GIT_TERMINAL_PROMPT=0 git ls-remote upstream refs/heads/main` |
| Relay at hive.agiterra.org | `build_time` `2026-09-12T00:05:56Z`, checked 2026-09-13; `software_commit` is `unknown`, a disclosed non-answer from the stale deployer (ledger §3a, "hive's `software_commit` is `unknown`") | `curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq '{software_commit, build_time}'` |
| Installed Mac dev bundle | `/Users/brian/Applications/Beekeeper Dev.app`, installed/relaunched 2026-09-13 from `566e31075`; bundled CLI says `bee 0.1.0 (566e3107)` built `2026-09-14T02:14:26Z`; signature verified. Walkthrough UI acceptance remains pending | `scripts/app-from.sh 566e31075`; installed `bee --version`; `codesign --verify --deep --strict`; [walkthrough evidence](history/2026-09-13-tank-loop-walkthrough.md) |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last recorded claim: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

The last product landing before Andy's two 2026-09-11 fixes was `0fc2beaf9`
on 2026-09-09, with `just ci` and `just test` green on that exact commit
(ledger § "September 9 main published").

## Active work and owners

| Work | Owner | State (check date in row) | Where |
| --- | --- | --- | --- |
| Project team setup and neutral baseline | Astra integrates Terra/Sol work | 2026-09-13: first-project checked snapshot → publication → local installation → lead handoff landed as `1820d238d`. Nine native tests, twelve UI tests, typecheck and one fresh browser flow pass. Mac bundle installed. Brian completed Tank Loop authoring, host validation and snapshot save; Publish exposed a native JSON naming mismatch, corrected in `b2e4c35c2` with failing-before/passing-after native decoder tests; corrected Mac bundle installed and relaunched. Tank Loop publication and six-role installation subsequently succeeded; Loom ran with the adopted pack. The client then incorrectly blocked Brian on channel membership; walkthrough corrections landed and are installed at `566e31075`. Existing-source maintenance deliberately refuses without provenance | [`PROJECT_TEAM_ACTIVATION_SLICE.md`](PROJECT_TEAM_ACTIVATION_SLICE.md), [activation evidence](history/2026-09-13-project-team-activation.md), [`PROJECT_TEAM_SETUP_IMPL.md`](PROJECT_TEAM_SETUP_IMPL.md) |
| Tank Loop setup walkthrough fixes | Opus built; Astra reviews and integrates | 2026-09-13: landed on both remotes at `566e31075`; Mac bundle installed and relaunched, signature verified. Live UI acceptance remains pending. Founder view-only was a client false negative for a project owner in a project transport (confirmed live); fixed to mirror the relay's write rule. Installed project agents, lead picker, rename, saved-group demotion, setup stages and role-pack reconciliation guidance addressed. Integration closes the unlocked publication-read journal race; live roster refresh remains a separate UI follow-up | [walkthrough report](history/2026-09-13-tank-loop-walkthrough.md), [`TANK_LOOP_WALKTHROUGH_IMPL.md`](TANK_LOOP_WALKTHROUGH_IMPL.md) |
| Native mid-turn steering for Claude sessions | Fable/Opus; landed by Astra | 2026-09-13: static checks and fresh E2E build passed; selected browser matrix was 44 passed/1 failed, only dense history. Isolated dense repeat also failed (336/450; matrix 286/450), so diagnosis remains open. Brian accepted landing with that limitation; final CI completed successfully; [recovered evidence](history/2026-09-13-final-ci-recovery.md) records every recipe leg. Push completed as `e12495c63`; Mac installation completed at `1820d238d`; installed UI acceptance remains pending | [startup evidence](history/2026-09-13-startup-smoke-corrections.md), [integration evidence](history/2026-09-12-steering-integration.md), `NATIVE_STEERING_IMPL.md` |
| Automatic project context beside ordinary agent work | paused by Brian on 2026-09-10 | findings saved; next step is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, ordered steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of steps 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two items) |
| Revealed-redaction badge as an icon; a streaming transcript resolves every marker | Andy | landed 2026-09-13 as `343ea8bd9`, included in current main | ledger § "Fixed 2026-09-13 — the revealed-redaction badge is an icon" |
| A Solo session's goal is one line: summarized by the naming model, clamped with a chevron | Andy | landing 2026-09-14 on `fix/goal-summary` | ledger § "Fixed 2026-09-14 — a Solo session's goal is one line" |
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
  "inherited, not caused" (2026-08-19) still exist.~~ Checked 2026-09-12: a
  prior full smoke on the steering branch reported six failures also
  reproduced on base `77b792de9` (ledger item 115). Checked 2026-09-13:
  all nine cases from Opus's later report now pass on the combined candidate.
  The fresh selected matrix is 44 passed/1 failed, only dense history; its
  isolated repeat also fails (286/450 and 336/450). Brian accepted landing
  with that known limitation while diagnosis continues. The overnight sleep
  failure passed in the completed 77-case remainder. See [September 13 evidence](history/2026-09-13-startup-smoke-corrections.md).
  Never run smoke while CI builds the desktop: the plain build overwrites
  the E2E bundle. Prevent idle sleep during unattended checks; low battery
  interrupted this run.

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
3. Diagnose the accepted dense-history limitation after landing: the selected
   matrix was 44 passed/1 failed (286/450 rows), and its isolated repeat reached
   336/450. No repeated full smoke marathon is required for the activation slice.
4. The lead pack is already published in `agiterra-packs` at `5f4ae76fa`.
   Running seats retain their staged revision; use `bee packs status` to inspect
   a project's source before claiming that a live seat has the new instructions.
5. When Brian resumes automatic context, run the hook-marker experiment in the
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
| Numbered findings, items 1–112 and counting | `SESSION_STATE.md` §2; `grep -n '^<n>\. ' docs/SESSION_STATE.md` | yes |
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
