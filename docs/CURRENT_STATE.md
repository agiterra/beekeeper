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
| `main` on the relay and on GitHub | `9aebb1262`, checked 2026-09-12; no newer commit arrived during integration | `git fetch origin main` then `git rev-parse FETCH_HEAD`; `GIT_TERMINAL_PROMPT=0 git ls-remote upstream refs/heads/main` |
| Relay at hive.agiterra.org | `build_time` `2026-09-10T19:48:29Z`; `software_commit` is `unknown`, a disclosed non-answer from the stale deployer (ledger §3a, "hive's `software_commit` is `unknown`") | `curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq '{software_commit, build_time}'` |
| Installed Mac dev bundle | `/Users/brian/Applications/Beekeeper Dev.app`, built 2026-09-11 from `77b792de9`, two commits behind `main`; its bundled `bee --version` reports `77b792de` | ledger § "September 11 — latest main rebuilt"; log `../review-2026-09-11-local-rebuild/build.log` |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last recorded claim: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

The last product landing before Andy's two 2026-09-11 fixes was `0fc2beaf9`
on 2026-09-09, with `just ci` and `just test` green on that exact commit
(ledger § "September 9 main published").

## Active work and owners

| Work | Owner | State (check date in row) | Where |
| --- | --- | --- | --- |
| Project team setup and neutral baseline | Astra | 2026-09-12: authoring candidate through `048dc4e02` checked locally; conditional source transaction passes focused core/CLI/database/relay tests, typecheck and lint; combined CI passed on `0e4c2cd84`; broad `just test` passed on `939607485`; browser acceptance pending; no installed or published changes | [`PROJECT_TEAM_SETUP_IMPL.md`](PROJECT_TEAM_SETUP_IMPL.md), [authoring evidence](history/2026-09-11-project-team-setup.md), [publication checkpoint](history/2026-09-12-project-team-publication.md) |
| Native mid-turn steering for Claude sessions | Fable/Opus candidate; Astra integrates | 2026-09-12: runtime queued-steer fence correction passes 58 ACP/46 provider tests; three original browser failures and five steering cases pass. Both short-window cases pass; authority and persistence review closed with focused tests/clippy green. Combined CI and broad `just test` passed; real Claude adapter steer and idle guard passed (marker in linked report). Browser/installed UI acceptance pending. No push or installation | [integration evidence](history/2026-09-12-steering-integration.md), `NATIVE_STEERING_IMPL.md` |
| Automatic project context beside ordinary agent work | paused by Brian on 2026-09-10 | findings saved; next step is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, ordered steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of steps 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two items) |
| Revealed-redaction badge as an icon; a streaming transcript resolves every marker | Andy | landing 2026-09-13 on `fix/redaction-pill` | ledger § "Fixed 2026-09-13 — the revealed-redaction badge is an icon" |
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
  owed includes broad integration/browser gates and live use in an installed build with a real
  session. The queued-steer prevention race found during integration is corrected
  and has a failing-before/passing-after process test. codex-acp 1.6.2 has no idle guard and stays in boundary mode.
- **Automatic context** is blocked on Brian resuming it; the Claude hook
  transport is a candidate, not proven in the installed adapter.
- ~~**Unknown:** whether the 62 e2e-smoke failures that ledger §1 calls
  "inherited, not caused" (2026-08-19) still exist.~~ Checked 2026-09-12: a
  full `just smoke` on `main` `77b792de9` plus the steering branch reports
  1309 passed, 6 failed, 1 skipped; the six fail identically on the untouched
  base (ledger item 115). Never run `just smoke` while `just ci` builds the
  desktop: the plain build overwrites the e2e bundle (item 115).

## Next, in order

Project team setup milestone 1 is checked locally. On 2026-09-11 authoring passed 3,250 Tauri tests (18 ignored), 9,267 desktop tests, two provider subprocess/fence tests and seven mock-bridge browser cases. Final `just test` passed, including both pack-source admission database tests. Corrected neutral operation instructions also passed 188 persona tests and a fresh 63-test native setup run. No push, installation or relay deployment is claimed. Next: integrate the checked conditional source transaction (2026-09-12), wire snapshot publication using an isolated Git candidate ref, then project-qualified installation, lead handoff and live Tankloop acceptance. Ordinary Solo remains independent. The linked plan records missing first-project channel creation and explicit resume of setup identities.

1. Done 2026-09-11: the lead pack revision landed on the packs repository
   (`agiterra-packs` `main` at `5f4ae76fa`, the tree seats stage from; this
   repo's `personas/roles` is only its source). The project's pack source
   pins `refs/heads/main`, which the host fetches on every staging
   (`desktop/src-tauri/src/managed_agents/packs_cache.rs`, `sync_packs_checkout`),
   so a seat hired from now on reads the new pointer. Seats already running
   keep the revision they staged; none were restarted. Check with
   `bee packs status --project <coordinate> --role lead`.
2. Rebuild the installed Mac bundle from `main` when Andy's two fixes need
   live use: `scripts/app-from.sh <sha>`. Brian's call.
3. The reviewed steering and project-setup candidates are combined on
   `work/steering-integration-astra`. Combined `just ci` passed on `0e4c2cd84`.
   Broad `just test` passed on `939607485`; all six unchanged baseline browser cases pass. Final CI exposed a corrected test-fixture race (ledger 120). Rerun CI and full smoke before landing and
   installed Claude acceptance. All three reported browser failures now pass
   in the focused integration run. The original smoke findings and the
   separate umbrella layout remain open; see the integration evidence above.
4. When Brian resumes automatic context: the hook-marker experiment exactly as
   the ledger's September 10 section specifies it.
5. Implement host publication from `PROJECT_TEAM_PUBLICATION_IMPL.md`: one
   scoped reservation, exact output snapshot, isolated Git push and conditional
   SHA adoption. Then project-qualified installation/lead handoff and Tankloop
   acceptance. Collaborative workspace plan steps 5 and 6 remain separate.

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
