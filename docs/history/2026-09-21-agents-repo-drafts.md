# 2026-09-21 — Shared drafts for the agents repository: build and live runs

Andy with Opus. Plan: `~/.claude/plans/i-d-like-to-plan-cozy-lamport.md`
(Andy's machine). Ledger 224–226; spec § 4.12; `docs/nips/NIP-AD.md`.

## What was built, in order

1. `buzz-core`: kind 44250, validator, fold; `conformance/agents-repo-draft-fold/`
   (15 vectors). `8c4584141`.
2. `buzz-relay`: the `ad-repo` and `commit.record` checks at ingest; the
   `tree`/`raw` read routes. Same commit.
3. `buzz-persona::agents_repo::validate_root`; `buzz-sdk` builders;
   `bee agents-repo` / `bee plans`; the seat briefing sentence; NIP-AD, spec
   § 4.12, the runbook § 6.13b, ledger 224. `248d12318`.
4. Desktop host commands (`agents_repo_ls`, `agents_repo_read`,
   `agents_repo_commit_drafts`). `174fd22b8`.
5. Desktop Files tab, TypeScript fold and binder. `d438a2b67`; e2e, Roles
   link and the head-conflict fix `3d8f8d8d4`.
6. Mobile Files page. `be107060a`.

## Live run 1 — `bee` against a local relay on :3010 (`8c4584141` + CLI)

Relay: `./target/debug/buzz-relay` with `BUZZ_BIND_ADDR=0.0.0.0:3010`,
`RELAY_URL=ws://localhost:3010`, `BUZZ_HEALTH_PORT=18095` (8080 and 8090
are held by node processes on this Mac); `/health` → `ok 8c458414`. A fresh
hex key; git credentials through `GIT_CONFIG_COUNT` entries scoping
`credential.http://localhost:3010/git.helper=nostr` and
`NOSTR_PRIVATE_KEY`; `BUZZ_TEMPLATES_DIR=$PWD/personas/templates`.

| Step | Result |
| --- | --- |
| `bee projects create draft-live` | project and code repo created; **seed failed**: `could not create <tmp>/bee-packs-init-…/.: No such file or directory` — the root-path seed bug, fixed in `248d12318` |
| `bee packs init --project $P` (after the fix) | seeded `5c2bf839…` on `refs/heads/main`, 30624 `ref: refs/heads/main, path: .` |
| `bee agents-repo ls` | 15 blobs at `5c2bf839…` (the `tree` route) |
| `bee plans edit rpg --message "first cut"` | accepted `e20d09e9…`, `prev: null`, `base: null` |
| `bee plans edit rpg` (no `--prev`) | **exit 5** `conflict: plans/rpg.md: bdb18242 saved a newer draft (e20d09e9a09b); … --prev e20d09e9a09b` |
| `bee plans edit rpg --prev e20d09e9a09b` | accepted `83182dfc…` |
| `bee agents-repo draft archive roles/poker.md` | accepted `ed3c05fa…` (`file.move` → `roles/archive/poker.md`) |
| `bee agents-repo commit --all` | `pushed: "no"`, `invalid-tree` at `team.yml`: `role "poker" names roles/poker.md, which does not exist` |
| `bee agents-repo draft put team.yml --file …` (poker dropped) | accepted `b87503bd…` |
| `bee agents-repo commit --all --message "docs(agents): first plan, poker archived"` | `pushed: "yes"`, commit `7c079ad1…`, paths `A plans/rpg.md`, `A roles/archive/poker.md`, `D roles/poker.md`, `M team.yml`, `actions: checked (0 actions)`; `commit.record` accepted `a1e645f5…` (the relay's main-tip check passed) |
| `git ls-remote … refs/heads/main` | `7c079ad1…` |
| `bee agents-repo drafts` | one straggler: the *superseded* first draft was still open — fixed the same hour (a commit now closes the whole chain); repaired live with `bee agents-repo commit-record 7c079ad1… --draft e20d09e9a09b` → `open: 0, commits: 2` |
| `bee plans show rpg` | `source: main`, blob `29aadf90…`, commit `7c079ad1…`, the v2 text |
| `bee plans edit rpg` (new draft on the new main) | accepted with `base` = `29aadf90…` |
| `bee agents-repo draft withdraw <id>` | kind 5 accepted; `open: 0` |

The relay e2e `crates/buzz-test-client/tests/e2e_agents_repo_drafts.rs`
(viewer 403, collaborator admitted and folded, wrong repo and no-source
refused, off-main record refused, stranger withheld) is green on the same
relay.

## Live run 2 — the mobile reader against the same relay

`BUZZ_AGENTS_REPO_LIVE='http://localhost:3010|<hex>|<owner>|draft-live-beekeeper-agents|plans/rpg.md'
flutter test test/features/agents_repo/data/agents_repo_http_client_live_test.dart`
→ `main 7c079ad1… · 21 entries · plans/rpg.md blob 29aadf90…`, one pass.
The mobile NIP-98 token is bound to the repository root exactly as the
credential helper binds it, and the relay's git extractor accepted it.

## What was proved without the relay

- Desktop host: four tests against a bare `file://` remote seeded from the
  shipped templates (`agents_repo_commit_tests.rs`): listing and read from
  the fetched tip; a landed commit with `Co-authored-by`, `Signed-off-by`
  and `Beekeeper-Drafts` trailers after an `invalid-tree` refusal at
  `team.yml`; stale-base and main-moved refusals; a lost lease.
- Desktop renderer: nine node tests; the conformance binder; three
  Playwright smoke tests (`project-agents-repo.spec.ts`) — the second of
  which found that the screen passed the *current* head as the editor's
  `openedOn`, so the refusal could never fire; fixed in `3d8f8d8d4`.
- Mobile: the fold vectors, the codec, five widget tests; the whole suite
  (2,123) green.

## Not proved, disclosed

- ~~The desktop Files tab has not been driven in the installed app against
  a relay.~~ Andy drove it in the dev app on the local relay the same
  afternoon: roles drafted, a plan created, saved and committed. It found
  the dev app silently on hive (one `lsof` connection, to hive:443; the
  deployed relay refused the unknown kind) and a dead **New plan** button
  (`window.prompt` is a no-op in Tauri's webview) — fixed with a dialog and
  a spec that presses it. On landing the kind was renumbered to **44250**:
  `main` had taken 44249 for the project work record (NIP-PW).
- The mobile page has not been run on a simulator; its reader and its
  widgets are proved separately. The first simulator run is owed.
- Nothing has run against hive.
