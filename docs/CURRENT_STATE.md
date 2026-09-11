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

## What is live (checked 2026-09-11)

| Surface | Observed | How |
| --- | --- | --- |
| `main` on the relay and on GitHub | Andy's `49b4f81c4` ("a founded session files under its project, not General") plus this map's landing commit on top of it | `GIT_TERMINAL_PROMPT=0 git ls-remote origin refs/heads/main`, same for `upstream` |
| Relay at hive.agiterra.org | `build_time` `2026-09-10T19:48:29Z`; `software_commit` is `unknown`, a disclosed non-answer from the stale deployer (ledger §3a, "hive's `software_commit` is `unknown`") | `curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \| jq '{software_commit, build_time}'` |
| Installed Mac dev bundle | `/Users/brian/Applications/Beekeeper Dev.app`, built 2026-09-11 from `77b792de9`, two commits behind `main`; its bundled `bee --version` reports `77b792de` | ledger § "September 11 — latest main rebuilt"; log `../review-2026-09-11-local-rebuild/build.log` |
| Windows | unavailable: Brian no longer has the Windows machine (2026-09-09). Native Windows agent setup and cross-account acceptance are deferred, not done | ledger § "September 9 checkpoint published; Windows testing deferred" |
| Mobile | not independently checked for this map. Last recorded claim: the phone drops a deleted session only on its next channel refresh (Andy, 2026-09-11) | ledger § "Fixed 2026-09-11 — Discard deletes a never-started session" |

The last product landing before Andy's two 2026-09-11 fixes was `0fc2beaf9`
on 2026-09-09, with `just ci` and `just test` green on that exact commit
(ledger § "September 9 main published").

## Active work and owners

| Work | Owner | State (2026-09-11) | Where |
| --- | --- | --- | --- |
| Native mid-turn steering for Claude and Codex sessions | unassigned; begins when Brian delegates it | planning only: seams, sequence, lane ownership and acceptance written; no code changed | ledger § "September 11 — latest main rebuilt; native steering plan for Claude" |
| Automatic project context beside ordinary agent work | paused by Brian on 2026-09-10 | findings saved; next step is proving a harmless marker reaches a fresh Claude session through a hook | ledger § "September 10 — automatic project context: findings saved, work paused" |
| Collaborative workspace plan, ordered steps 0–6 | Astra finalizes and lands `main`; Fable takes delegated slices; Andy lands his own topic branches | steps 0–4 have integrated candidates per the ledger's September 7–9 sections; no landing of steps 5 or 6 is recorded anywhere found, so treat them as open | [`COLLABORATIVE_WORKSPACE_PLAN.md`](COLLABORATIVE_WORKSPACE_PLAN.md) |
| Founded-session fixes: Discard deletes, project filing | Andy | landed on `main` 2026-09-11 | ledger § "Fixed 2026-09-11 —" (two items) |
| Delegated agent-directory visibility follow-up | unassigned | "can resume separately on main" (Astra, 2026-09-09) | ledger § "September 9 main published" |
| This map, the ledger split and its size gate | Fable | landed 2026-09-11; pack restaging still owed | `AGENTS.md` top block; `scripts/check-current-state-size.mjs` |

## Decisions in force

Rules, each with where it is written down.

- **Git is relay-canonical.** Push to `origin` (hive) only; the bridge mirrors
  GitHub. Topic branches rebase onto `main`, never merge; `vanilla/main`
  merges, never rebases. Commit with `-s`. `AGENTS.md` top block,
  [`INTEGRATION.md`](INTEGRATION.md).
- **No human gates a landing.** `main` is admitted by the relay's push gate
  from observed gate rows and verifier verdicts. A refusal names a missing
  fact; the remedy is a command or a hire, never a person. Lead pack,
  `personas/roles/lead/skills/beekeeper-project/SKILL.md` § "Gates the host
  can see".
- **Honesty in the product is a first-class concern.** A control that lies
  about what it enforces is a bug of crash severity. `AGENTS.md` § Working
  agreements.
- **Findings go into the ledger as numbered items; this map is updated, never
  appended; session reports go under `docs/history/`.** `AGENTS.md` top block.
- **Identities are durable and named, seats are ephemeral, role packs are
  signed and versioned relay records** (D11–D16). Plan § "Decisions already
  made"; lead pack § "The team model".
- **"Crew" is "team"** in anything a person reads. Lead pack § "What the
  operator will not accept".
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
- **Native steering** is blocked on proving adapter support.
  `crates/buzz-session-provider/src/session.rs:58` hard-disables native
  delivery; nothing may be enabled on a constant flip or on green mock tests.
- **Automatic context** is blocked on Brian resuming it; the Claude hook
  transport is a candidate, not proven in the installed adapter.
- **Unknown:** whether the 62 e2e-smoke failures that ledger §1 calls
  "inherited, not caused" (2026-08-19) still exist. No later section says.
  The 2026-09-09 landing ran a targeted 9/9 browser rerun, not the full suite.

## Next, in order

1. Land this map and its gate. Then restage the lead pack revision so running
   seats read the new pointer; the landing alone does not do that, and the
   restaging evidence is owed separately.
2. Rebuild the installed Mac bundle from `main` when Andy's two fixes need
   live use: `scripts/app-from.sh <sha>`. Brian's call.
3. When Brian delegates native steering: step 1 of the ledger plan, prove
   adapter support with a held-open prompt fixture before any provider change.
4. When Brian resumes automatic context: the hook-marker experiment exactly as
   the ledger's September 10 section specifies it.
5. Plan steps 5 and 6.

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
- macOS has no `timeout`. pnpm 11 deletes a shared `node_modules` when a
  worktree's state file is stale.

## Where evidence lives

| Kind | Place | Tracked |
| --- | --- | --- |
| Numbered findings, items 1–109 and counting | `SESSION_STATE.md` §2; `grep -n '^<n>\. ' docs/SESSION_STATE.md` | yes |
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
