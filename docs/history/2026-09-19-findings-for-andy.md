# Findings for Andy — the pivot and the hire path under live use, 2026-09-16 → 2026-09-19

Compiled by Fable for Brian. Everything here was found by using the installed
app against hive, not by reading code alone; each finding names its ledger
item, which carries the evidence and `file:line`. Status is as of 2026-09-19
14:25 EDT. Items 7–10 and 18 are on `main`; the bundle is being rebuilt from `6860e1574`. Everything in section B is on `main`; the installed bundle is being rebuilt from `f14200948`. "Lane" means a fix is being built on a topic branch and will land
on `main` rebased; "open" means nobody owns it yet.

## A. Fixed and landed since 2026-09-16 (for awareness; review welcome)

| # | Finding | Ledger | Landed as |
| --- | --- | --- | --- |
| 1 | Hired seats' worktrees were cut from the most recently used folder, not the project's recorded checkout (both Tank Loop seats landed in the Beekeeper repo). Hires now resolve project checkout first and refuse with `HIRE_CHECKOUT_NOT_RECORDED` naming the exact control. | 135(a), 136, 139 | `3e22f8ef8`, `85062988c` |
| 2 | Hires ignored the agent's configured runtime; the summary published the raw pin, empty when inherited. The host now publishes `effective_runtime` + `runtime_source`; hires and the Agents tab read it. | 135(b), 136, 139 | `3e22f8ef8`, `85062988c` |
| 3 | "Use roles" readiness enforced the in-checkout `personas/roles` layout while the hire staged from the relay source; the catalog check blocked Start on a registry that did not exist. | 135(c), 137, 140 | `139e83334`, `19b23b9d9` |
| 4 | A deletion was not a closure: `bee sessions worktree status` said "not closed" after `bee sessions delete`, so trees and bundles were never reaped; no CLI could close a session. Now a deletion settles trees; `bee sessions close` exists. | 135(f), 137 | `139e83334` |
| 5 | The project Agents tab hid the runtime; the bundled `bee packs status` read the release app's cache. | 135(d)(e), 138 | `e28864c86` |
| 6 | A hire answered from a cached workdir snapshot: the checkout was recorded at 10:56:53Z, both hires at 10:58Z were refused as if it were not. Hires now read the store at the moment they answer. | 167 | `29344c1ea` |

## B. Found on your pivot code, fixed and landed today (please review)

| # | Finding | Ledger | Lane |
| --- | --- | --- | --- |
| 7 | **git ≥ 2.46 is a hard requirement nobody states.** `git-credential-nostr` speaks the `authtype` credential protocol and prints nothing without `capability[]=authtype`. A Finder-launched bundle resolves `git` from `/usr/bin` (Apple git 2.39), so every credentialed fetch/clone failed with "could not read Username". A branch-pinned agents repository fetches on every hire, so the pivot hits this on every machine that lacks a newer git on the GUI PATH. Reproduced with the exact `GIT_CONFIG_COUNT` environment; Homebrew git 2.55 succeeds with the same config. | 168 | **Landed** `37dfdec4e`: git-version probe for remote ops, honest refusal, About-row and readiness disclosure, README requirement |
| 8 | **The seat's agents-repository clone is refused by the file-transport rule.** `cut_seat_agents_clone` clones from the packs cache with `build_git_auth_config`, which sets `protocol.file.allow=never`; git answers "transport 'file' not allowed". Reproduced by hand. | 169 | **Landed** `86dde1d3f`: local clone config with file transport allowed; `origin` still re-pointed at the relay |
| 9 | **A staging failure after the worktree is cut never becomes a refusal.** The lead's `bee sessions hire` waited 120 s and reported "unconfirmed"; no refusal on the relay, the worktree left recorded. | 169 | **Landed** `86dde1d3f`: `HIRE_SEAT_STAGING_FAILED` with the native error, the cut tree disposed of |
| 10 | **The provider outbox is poisoned by rows the relay can never accept.** 150 pending rows (149 × 44223, 1 × 44225) from 2026-09-04/05 sit outside the relay's ±900 s window; `flush_limit` breaks on the first failure, so fresh events (the host-step claim, seat metadata) rarely get a turn. The host-step run sat in `waiting_host` until the rows were acknowledged by hand. | 170 | **Landed** `f14200948`: definitive relay rejections and out-of-window rows are parked, the drain continues, the parked count is disclosed |

## C. Open, no lane yet (your call on priority)

| # | Finding | Ledger | Where |
| --- | --- | --- | --- |
| 11 | **Actions tab shows "No actions are published"** for a project whose workflow the relay returns to `bee workflows list` on the same channel, after a remount. The tab's request never produced a hit; the channel set it computes is the suspect. | 171(a) | `desktop/src/features/project-actions/lib/useProjectActions.ts:140-162` |
| 12 | **The inbox renders a host-step approval request as raw JSON** with a reply composer and no approve control, although `inbox.ts` recognises kind 46010. The only working approval path today is the desktop's `grant_approval` command, which no surface reaches for this request. | 171(b) | `desktop/src/features/home/lib/inbox.ts:165-187` |
| 13 | **`bee workflows approve` cannot answer a host-step request.** It hashes a UUID token; a synthetic approval's `approval_ref` is already the stored hash and the relay matches the `d` tag directly. A hand-signed 46030 with `d = approval_ref` was accepted. | 171(c) | `crates/buzz-cli/src/commands/workflows.rs:232` |
| 14 | **The Actions tab's empty state hands a person a CLI command with a placeholder** instead of a control or a pointer to where `actions.yml` lives. | 135 addendum / spec § 6 item 10 | `desktop/src/features/project-actions/ui/` |
| 15 | **Two hires in the same second race the authority fold:** "authority receipt references a missing transition", answered by `seat-repair`. The fold treats a not-yet-visible transition as a failure instead of unknown-and-retry; the host could also serialize authority publications per session. | 136 addendum | `crates/buzz-cli/src/commands/sessions/operations_authority.rs:285` |
| 16 | **A seat's process does not carry its own session reference**, so an identity seated in two sessions needs `--session-ref` on every send and each seat learns this by a failed call. | 136 addendum | provider seat env (`actor_seats.rs:195`), `bee sessions send` |
| 18 | **A seat's agents clone is cut from the packs cache's local `main`, which never advances.** `sync_packs_checkout` fetches into `refs/remotes/origin/*` and checks out detached, so `git clone --branch main <cache>` yields the first-ever sync (`6405b21`) while the same hire's bundle came from `d148d54`; the clone's `team.yml` lacked the grant that seated it. | 172 | **Landed** `6860e1574`: the clone lands on the staged commit (`git checkout -B <branch> <sha>`), reuse path advances an existing clone |
| 17 | **Hive returned 502 for ~6 s at 11:47:47Z** and rate-gated the provider's catch-up burst after a relaunch ("relay rate gate blocked acknowledged publication"). Both recovered; worth knowing what restarted. | 171 | relay / ingress |

## D. What passed, so you know the ground is solid

- Project creation on hive: head with both repositories, seeded agents
  repository, eight default agents pinned to a runtime (P1/P2, 164, 165).
- A Team session with a template-composed hire: worktrees from the project
  checkout, bundles with `composeRef` and `packRef` at the agents-repo commit
  (A1–A4).
- A manual `run_on_host` action end to end once the outbox was cleared:
  publish → trigger → approval → host claim → exit 0 with captured output at
  the checkout's head, clean (C1–C6).
- A seat with an `agents_repo: read` grant (P4): hire answered within two
  minutes, the clone beside its worktree with `origin` on the relay, the
  briefing read-only — at the stale commit noted in 18.
- Seat bundles and the verification fence from 2026-09-16 kept working
  under the pivot.

Run record: [2026-09-19-pivot-live-proofs.md](2026-09-19-pivot-live-proofs.md).
Earlier run: [2026-09-15-seat-bundles-experiment.md](2026-09-15-seat-bundles-experiment.md).
