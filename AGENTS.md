# AGENTS.md — AI Agent Contributor Guide

> **The map, the ledger and the plans are not in this repository.** They live
> in Beekeeper's own agents repository, `bee-keeper-beekeeper-agents` on hive,
> which is where a project's roles and plans belong (spec § 1 decision 5, and
> ledger 233–234 for the move itself). Two ways in:
>
> - **A person or a session working in this checkout**: run `just agents-repo`.
>   It clones or fast-forwards the repository to the sibling directory
>   `../agiterra-beekeeper-agents` — beside this checkout, never inside it, so
>   the code tree stays free of the plans.
> - **A team seat**: it is already there, cloned beside the seat's worktree as
>   `<worktree>-agents`. Nothing to run.
>
> Paths below are relative to that repository's root.
>
> **Read `plans/CURRENT_STATE.md` first, all of it; it is gated to 300 lines
> and 24,000 bytes** by the `limits:` block in that repository's `team.yml`,
> enforced on every commit path. It is the current-state map: what is deployed
> (with the command that proves it and the date it was last checked), what is
> being worked on and by whom, the decisions in force, the blockers, and the
> ordered next steps. Anything needing more than a paragraph links out: to the
> plan that governs it, to a numbered item in the ledger, or to a dated report
> under `plans/archive/`. Read what your task links to, not everything it could
> link to. After your context is compacted, re-read the map and the plan it
> names before continuing.
>
> **Findings go into `plans/SESSION_STATE.md`, the ledger**, as a numbered item
> with the code or transcript that proves it, the day they are found. Item
> numbers are frozen: code and tests **in this repository** cite them, and
> those citations now point across repositories — a comment reading
> `plans/SESSION_STATE.md item 103` means that file in the agents repository,
> and the numbers did not change when it moved. The ledger preserves its
> historical content; a claim it got wrong is struck where it stands with a
> pointer to what supersedes it, never deleted. It is far too large to read:
> find an item with `grep -n '^<n>\. ' plans/SESSION_STATE.md` and read that
> window. Session reports and lane transcripts are not findings: they go under
> `plans/archive/` as dated files that a ledger item or the map links to, never
> only on a Desktop or in a review directory this machine alone can see.
>
> **The map is updated, never appended.** Whoever lands a change that alters
> active work, decisions, blockers, or what is deployed updates the map in the
> agents repository — a second commit, in a second repository, in the same
> landing as the code. Where the map disagrees with an older document about
> *current state*, the map wins. Where the map and the ledger disagree, check
> the code or the relay, record the result in the map with the check date, and
> strike the losing claim where it stands; if the evidence cannot settle it,
> mark the state unknown and keep both links.

> **This repo is Beekeeper.** It began in 2025 as a fork of
> [block/buzz](https://github.com/block/buzz), which is why so much of the tree
> is still named `buzz-*`, but it is its own product now: nothing is pulled from
> that repository and nothing is pushed to it. There is no vanilla mirror to
> merge and no upstream to track. A `block/buzz` link in an older document is
> history, not a process — if you find one that reads as an instruction, it is
> stale and worth deleting.
>
> It is a single-branch repo: `main` is the product and topic branches are
> ordinary and short-lived. There is no assembly ceremony and no split step;
> build on a topic branch, let the user test it, land it.
>
> **Topic branches are rebased onto `main`, never merged into it.** They are
> short-lived and single-author, so rewriting them costs nothing and keeps
> `main` linear. Use `git rebase --signoff main` — rebase onto the local branch
> rather than a remote-tracking ref, because remote names have moved before and
> any command naming one dates quickly. A plain `git rebase` preserves existing
> trailers, but any commit it recreates without one fails the DCO gate.
> Force-push the topic branch afterwards; that is expected.
>
> **Two remotes, both ours.** `origin` is the relay's own git hosting
> (`hive.agiterra.org`) and is canonical; `upstream` is
> [agiterra/beekeeper](https://github.com/agiterra/beekeeper), the GitHub copy
> CI watches, which a bridge fills from `origin` within seconds of a push — so
> you push to `origin` only. Beware the name: in documents written before
> 2026-08-24, and in the ledger, "upstream" means `block/buzz`; here it means
> agiterra's own GitHub copy. Run `git remote -v` rather than trusting memory,
> and **never hard-code a remote name in tooling** — two pre-push guards did and
> both broke silently the day the names moved.
> See [docs/INTEGRATION.md](docs/INTEGRATION.md) § Remotes.
>
> **Pushing to `origin` needs Nostr credentials — run `just
> install-git-credentials`.** The relay authenticates git with NIP-98, not a
> password, so without the helper `git fetch origin` waits on a username prompt
> that can never be answered. `bee git status` says whether it is set up.
> The recipe writes no key material: the key file is the human's to create.
> See [docs/INTEGRATION.md](docs/INTEGRATION.md) § Pushing to the relay.

This guide is for AI agents contributing to the Beekeeper codebase. It covers
agent-specific context and conventions. For general contributor info (setup,
code style, PR process, architecture), see [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Product Contract

Before planning or reviewing a non-trivial change:

1. Read [VISION.md](VISION.md).
2. Read the `VISION_*.md` documents relevant to the affected product surface.
3. Read the applicable guidance in [TESTING.md](TESTING.md) and any
   package-local `TESTING.md`.
4. Check that the proposed design advances, or at least does not contradict,
   that product intent. Call out any intentional tension explicitly.

Implementation describes the product today; the vision documents describe the
product it is becoming. A locally correct change can still be wrong if it works
against that direction. Scale validation to the change's risk and exercise the
real workflow for user-visible or integration behavior when practical; green CI
and runtime evidence answer different questions.

---

## Ecosystem

Beekeeper is two repositories and one pipeline, all agiterra's own. Everything
that used to live in Block's build and deploy repos is either in this tree under
`deploy/` or on the relay host.

| Where | What |
|-------|------|
| this repo | relay, desktop, web, mobile, CLI, agent harness — and its own deploy configuration under `deploy/` |
| `bee-keeper-beekeeper-agents` on hive | the project's roles, plans, ledger and `team.yml`. See the top of this file; `just agents-repo` clones it beside this checkout |
| [ci.agiterra.org](https://ci.agiterra.org) | Woodpecker runs `.woodpecker/` on pushes to `main` and on PRs |
| the relay host | `beekeeper-autodeploy.timer` deploys the newest green `main` pipeline into `/opt/beekeeper` |

```
hive.agiterra.org          (origin — canonical, NIP-98 auth)
  └─► bridge ──► agiterra/beekeeper on GitHub   (upstream — what CI watches)
                   └─► ci.agiterra.org          (.woodpecker gate)
                         └─► beekeeper-autodeploy  (newest green main → the relay)
```

Nothing flows to or from `block/buzz` in either direction, and there is no
internal build pipeline to coordinate with: a push to `origin` is the whole
release path. See [RELEASING.md](RELEASING.md) for the desktop release flow and
[docs/INTEGRATION.md](docs/INTEGRATION.md) § CI and § Deploying for the gate and
the deployer.

---

## Repo Structure

```
crates/
  # Relay + core
  beekeeper-relay        # WebSocket relay server — main entry point; also hosts git + huddle audio
  beekeeper-core         # Core types, event verification, filter matching, kind registry
  beekeeper-db           # Postgres event store and data access layer
  beekeeper-auth         # Authentication and authorization
  beekeeper-pubsub       # Redis pub/sub fan-out, presence, typing indicators
  beekeeper-search       # Postgres FTS full-text search
  beekeeper-audit        # Hash-chain audit log
  beekeeper-media        # Blossom/S3 media storage
  # Agent surface
  beekeeper-host         # Headless daemon owning this machine's agents; starts at
                         # login, at boot on a headless Mac (`--system`), or as a
                         # server's user unit (docs/agent-host.md)
  beekeeper-host-core    # The launcher contract the host and the desktop share
  beekeeper-acp          # ACP harness bridging Beekeeper events to AI agents
  beekeeper-agent        # Minimal ACP-compliant agent (non-streaming, tool-calls-as-output)
  beekeeper-dev-mcp      # Developer MCP server — shell + file-edit tools
  beekeeper-persona      # Agent persona packs
  beekeeper-workflow     # YAML-as-code workflow engine (evalexpr conditions)
  # Clients + interop
  beekeeper-pair-relay   # Ephemeral sidecar relay for NIP-AB device pairing
  beekeeper-pairing-cli  # CLI for NIP-AB device pairing interop testing
  git-sign-nostr         # Sign git objects with a Nostr key
  git-credential-nostr   # Git credential helper for Nostr-authed push/fetch
  # Tooling + shared
  beekeeper-cli          # Agent-first CLI
  beekeeper-sdk          # Typed Nostr event builders
  beekeeper-admin        # Operator CLI for relay administration
  beekeeper-ws-client    # Shared NIP-42 WebSocket client (connect, auth, publish)
  beekeeper-test-client  # Integration test client and E2E test suite
  sprig                  # All-in-one harness bundling ACP, agent, and dev MCP

desktop/              # Tauri 2 + React 19 desktop app
web/                  # Browser web client (repo browser, served by the relay)
mobile/               # Flutter mobile app
migrations/           # SQL migrations (auto-applied on relay startup)
scripts/              # Dev tooling
.env.example          # Config template — copy to .env before running
```

---

## Getting Started

```bash
. ./bin/activate-hermit   # activate hermit toolchain (Rust, Node, etc.)
cp .env.example .env      # configure local environment
just setup                # install deps, run migrations
just relay                # start relay at ws://localhost:3000
just ci                   # run before any PR
```

See CONTRIBUTING.md for full setup details and dependency requirements.

**In a worktree you just cut, seed it instead of rebuilding from cold.**
`sandbox.yml` at the repository root declares this project's build state, and
`just sandbox-seed <tree> --confirm` carries it out: the two `target/`
directories and the four `node_modules` are copy-on-write clones (85 GB of
logical directories for under 500 MB of real disk), `CARGO_HOME` is a link into
one pool per repository rather than a fresh registry download, and the two
setup steps that otherwise read as product bugs — the sidecar stubs and the
`desktop/` install — are recipes it runs for you. `just sandbox-plan` prints
what is declared without touching anything. The Beekeeper launcher does the
same thing for a seat's worktree, so a hire's receipt says what it got and what
it did not. See [docs/INTEGRATION.md](docs/INTEGRATION.md) § Seeding a sandbox
for the five things about it that will mislead you — chiefly that `du` is not
what a clone costs, and that `CARGO_HOME` cannot be redirected with an
environment variable in a hermit project.

---

## Working agreements

Brian sets direction; you implement. Optimize for velocity within that.

- **Make the call.** Do not present a menu of options and wait — decide, act,
  and report the outcome: *"Did X. Result. Next: Y."* Terse is good.
- **Check in only for genuine irreversibility**: destroying someone else's
  work, outward-facing communication that notifies a person, money or
  production deploys, or a change of direction. Anything another commit can
  undo — just do it.
- **Honesty in the product is a first-class concern.** A control that lies
  about what it enforces, a badge pointing at a message you cannot find, a
  "default" label hiding the real model, a status that reads Idle over a
  disconnected provider — these are bugs of the same severity as a crash, and
  several of the best findings in this project came from exactly that kind of
  poking. Prefer disclosing an unpleasant truth over presenting a comfortable
  guess.
- **Multi-agent orchestration is pre-authorized.** The pattern that works:
  written spec → parallel build lanes with **strict file ownership** → full
  gate → adversarial review against named constraints → a single finalizer
  that commits. Lanes never commit; only the finalizer does.
- **Draft before sending.** Anything addressed to Andy or anyone outside this
  machine gets written, shown to Brian, and sent only on his word.
- **A completion report is not evidence.** Cite `file:line`, or the run that
  produced the output. This applies to your own prior claims and to any
  handoff document, including the ones in `docs/`.

## Quality Gates

Run `just ci` before every PR — it runs repository-wide formatting, lint,
and static checks; Rust, Tauri, desktop, and mobile tests; and desktop and web
builds. Clippy passing does not mean fmt passes; run both.

Run `just test` for integration tests if you touched `beekeeper-relay`,
`beekeeper-db`, or `beekeeper-auth` — these require a running Postgres and Redis.

**Pre-commit hooks** are installed automatically by `just setup` and auto-fix
formatting via `stage_fixed`. Pre-commit runs fix variants in parallel (Rust
fmt, Tauri Rust fmt, desktop biome fix, web biome fix, mobile dart format).
Auto-fixable issues are fixed and re-staged; unfixable lint issues block the
commit. **Pre-push hooks** run the repository-wide differential file-size gate,
clippy (workspace + Tauri), desktop TypeScript typechecking (`tsc --noEmit`),
and fast unit tests in parallel (Rust, desktop JS, Tauri Rust, mobile Flutter)
— no overlap with pre-commit. Builds are CI-only. Run `just fix-all` to auto-fix
all formatting in one shot. Run `just ci` for the full local gate. Run `just
hooks` to re-install hooks after env changes. Before agents run Git or hooks,
activate the repo's Hermit environment (`. ./bin/activate-hermit`); do not
rewrite hook commands to compensate for an unconfigured shell `PATH`. **Push
with `just push`, not a plain `git push`**, on any SHA that touches crates —
git mints the NIP-98 credential before the pre-push floor runs and reuses it
for the whole push, so a floor long enough to outlast the relay's token
window (as a crate change's clippy+tests can) fails `HTTP 401` with every
check green; `just push` runs the same floor first and pushes only once it
passes (see `docs/INTEGRATION.md` § Pushing to the relay, ledger 178(n)/191).

**Commit with `git commit -s`.** The required **DCO Check** fails any PR with a commit missing a `Signed-off-by` trailer, and `just hooks` installs a `commit-msg` hook that adds it to commits you create locally (`git rebase` and `git cherry-pick` still need `--signoff`) — if you build commit commands programmatically, include `-s` every time. To repair a branch that already has unsigned commits: `git rebase --signoff main`, then force-push.

Additional rules:
- No `unsafe` code
- Do not introduce new `unwrap()` or `expect()` in production paths — use `?` and proper error types
- New public API must have doc comments

---

## Key Patterns

**Nostr-first HTTP surface**: Beekeeper's primary API is NIP-29 over WebSocket. The relay also exposes a narrow HTTP surface: NIP-11/NIP-05 metadata, `POST /events`, `POST /query`, `POST /count`, workflow webhooks at `/hooks/{id}`, Blossom media, git smart HTTP, git policy hooks, and health probes. These HTTP paths all preserve the same host-derived community boundary.

**Prefer Nostr events over new HTTP endpoints**: For new feature work, model
the operation as a Nostr event (new kind in `beekeeper-core/src/kind.rs`, handler
in `beekeeper-relay`) rather than adding endpoint-specific JSON APIs. HTTP is
reserved for things that genuinely need an HTTP-only surface: media upload/download
(Blossom), webhooks, git smart HTTP, NIP-11/NIP-05 metadata, health checks,
and the generic Nostr bridge endpoints:

- `POST /events` — submit any signed event (same path the WebSocket uses).
- `POST /query` — Nostr REQ filters over HTTP. NIP-50 `search` filters
  are routed to `beekeeper-search` (Postgres FTS) automatically.
- `POST /count` — Nostr COUNT filters over HTTP.

If you find yourself reaching for a new HTTP endpoint, first check whether
an event kind would do the job — it usually will, and you get realtime
fan-out, NIP-29 scoping, and the existing auth pipeline for free.

Reference https://github.com/nostr-protocol/nips

**Event kinds**: All event kind integers are defined in
`beekeeper-core/src/kind.rs`. New features get new kind integers — add them here
first, then implement handling in the relay.

**Channel scoping**: Channels use `h` tags (NIP-29 group tag), not `e` tags.
Filters and queries must scope to `h` tags when operating within a channel.
This applies to events *inside* a channel. Addressable events that describe a
channel carry its id in their `d` tag instead: kind:39000 (metadata),
kind:39001, kind:39002 (membership). `get_channels` resolves a user's channels
from the `d` tag of their kind:39002 events, not from `h`.

**Agent-facing operations go in `beekeeper-cli`**: New agent-facing features belong in `beekeeper-cli` — add a subcommand there first, then wire the REST/WebSocket call in `client.rs`. `beekeeper-dev-mcp` (shell + file tools for `buzz-agent`) is separate.

**Workflow conditions**: `beekeeper-workflow` uses
[evalexpr](https://docs.rs/evalexpr) for condition evaluation. Keep expressions
simple and testable.

**Thread counters**: `reply_count` and `descendant_count` are materialized on
thread root events. Any code that inserts replies must update these counters —
check existing reply handlers for the pattern.

**Checking what a relay is actually running**: ask the relay, over NIP-11 —
it is public, unauthenticated, and answers before any WebSocket handshake.

```bash
curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \
  | jq '{software, software_commit, software_commit_count, build_time}'
curl -s https://hive.agiterra.org/health          # -> "ok <sha8>"
```

`software_commit` is the full 40-hex commit the binary was built from and
`software_commit_count` is `git rev-list --count` of that same commit. Both
are **disclosed non-answers rather than errors** when the build could not
determine them: `unknown` and `null` respectively. Never read either as a
failure, and never substitute a guess — that distinction is the whole point
of the fields (finding 32).

Three things that will mislead you:

- **A count is a set size, not a position.** `count(relay) − count(mine)` is a
  distance only when your commit is an ancestor of the relay's. On a topic
  branch, or after a rebase or squash-merge, it understates or changes
  meaning. If you report a number, report how you got it — see
  `EnforcementCheckMethod` in `crates/beekeeper-cli/src/commands/git_setup.rs`.
- **A shallow clone will lie to you.** `git rev-list --count` in a
  `fetch-depth: 1` checkout returns the graft's size, and `merge-base` /
  `rev-list --max-parents=0` will report "no common ancestor" and a false
  root. Run `git rev-parse --is-shallow-repository` before concluding
  anything about history; `git fetch --unshallow origin` fixes it.
- **`/_status` is not reachable.** It carries more build detail, but it is
  served only on the health port (8080), which compose does not publish and
  the ingress does not route. NIP-11 and `/health` are the whole public
  surface.

There is no `bee` subcommand that prints this today; `software_commit` is
consumed internally by `bee git check`. Full field reference:
`docs/INTEGRATION.md` § NIP-11.

---

## Agent CLI (`beekeeper-cli`)

`bee` is the agent-first CLI. Auth env vars
(`BEEKEEPER_RELAY_URL`, `BEEKEEPER_PRIVATE_KEY`, `BEEKEEPER_AUTH_TAG`) are auto-injected
by the ACP harness into managed agent subprocesses. In development, set
`BEEKEEPER_PRIVATE_KEY` and `BEEKEEPER_RELAY_URL` in your environment manually.

### Building the CLI

```bash
cargo build --release -p beekeeper-cli
```

Binary location: `./target/release/bee`. Add `./target/release` to `PATH`
or invoke with the full path.

### Deep Links

`beekeeper://message?channel=<uuid>&id=<hex>` links reference a specific message
thread. To read the linked thread:

```bash
bee --format compact messages thread --channel <uuid> --event <hex>
```

Extract `channel` and `id` from the URL query parameters. The optional
`thread` parameter (root event ID) can be ignored — `messages thread` resolves
the full thread from the event ID alone.

All reads return sig-stripped JSON arrays; all writes return
`{event_id, accepted, message}`; creates add the entity ID. Exit codes:
0=ok, 1=input error, 2=network/relay, 3=auth, 4=other, 5=write conflict (NIP-33 LWW).

`--format compact` is a **global** flag — it goes before the subcommand:
`bee --format compact channels list`, NOT `bee channels list --format compact`.

See `crates/beekeeper-cli/TESTING.md` for the full live-testing runbook.

---

## Testing

```bash
just test-unit    # unit tests, no infrastructure needed
just test         # full integration suite (requires Postgres + Redis)
```

E2E tests live in `crates/beekeeper-test-client/tests/`:
- `e2e_relay.rs` — WebSocket relay protocol
- `e2e_media.rs` — media upload/download (Blossom)
- `e2e_media_extended.rs` — extended media scenarios
- `e2e_nostr_interop.rs` — Nostr interop (NIP-50 search, NIP-10 threads, NIP-17 gift wraps)

Desktop E2E: `cd desktop && pnpm test:e2e:smoke` for mock-bridge smoke
coverage, or `pnpm test:e2e:integration` for relay-backed coverage. These
scripts build the required E2E bridge before running Playwright.

See [TESTING.md](TESTING.md) for the full multi-agent E2E guide.

### PR Screenshots

> **Do NOT use `bee upload`, the relay media endpoint, or any third-party
> image host for PR screenshots.** Relay media URLs fail through GitHub's camo
> proxy. Always use `scripts/post-screenshots.sh` for PNGs before linking them
> from a PR body/comment. If you hand-edit PR markdown, run
> `scripts/check-pr-image-urls.sh <markdown-file>` first to catch relay URLs.

For mobile simulator screenshots, save the PNGs in a local directory and run
`./scripts/post-screenshots.sh <PR-number> <png-dir>` or use the third argument
with a markdown template containing `{{filename}}` placeholders.

The desktop app requires the E2E mock bridge to render — it cannot run in a plain
browser. Use `just desktop-screenshot` to capture screenshots (builds frontend,
starts preview server, runs Playwright automatically):

```bash
just desktop-screenshot --name home
just desktop-screenshot --name channel --route /channels/general
just desktop-screenshot --name search --click open-search
just desktop-screenshot --name settings --click open-settings
```

Options: `--name` (filename), `--route` (client route), `--active-channel`
(channel to view), `--click` (left-click data-testid or CSS selector),
`--right-click` (right-click for context menus), `--hover` (hover before
capture), `--clip` (crop region as `x,y,w,h` — e.g. `0,0,256,720` for sidebar
only), `--wait` (ms, default 2000), `--viewport` (WxH, default 1280x720),
`--outdir` (default `test-results/screenshots`), `--messages` (JSON file path).
Output is a PNG path on stdout.

Use `--messages` to inject content into a channel before capture. The JSON file
is an array of objects — `channelName` and `content` are required, all other
fields are optional and passed through to `__BEEKEEPER_E2E_EMIT_MOCK_MESSAGE__`:

```json
[
  {
    "channelName": "random",
    "content": "Hey @tyler check this out",
    "pubkey": "953d...",
    "kind": 40002,
    "mentionPubkeys": ["deadbeef..."],
    "extraTags": [["broadcast", "1"], ["e", "some-root-id"]],
    "parentEventId": "abc123"
  }
]
```

Without `--active-channel`, all messages must target the same channel and the
helper navigates to that channel (useful for showing message content). With
`--active-channel`, messages can target multiple channels while the "camera"
stays on the specified channel (useful for unread indicators, badges, etc.).

```bash
# Messages in the channel you're viewing (code blocks, formatting, etc.)
just desktop-screenshot --name code-blocks --messages /tmp/msgs.json

# Messages in OTHER channels to trigger unread state
just desktop-screenshot --name unread-dot \
  --active-channel general --messages /tmp/badge-msgs.json

# Cropped to sidebar only (256px wide)
just desktop-screenshot --name sidebar-unread \
  --active-channel general --messages /tmp/badge-msgs.json \
  --clip 0,0,256,720

# Context menu on an unread channel (wider crop to include popup)
just desktop-screenshot --name ctx-mark-read \
  --active-channel general --messages /tmp/badge-msgs.json \
  --right-click channel-random --clip 0,200,320,300

# Hover state (e.g. copy button reveal)
just desktop-screenshot --name copy-hover \
  --messages /tmp/code-msgs.json --hover "[data-testid='copy-code']"
```

Available mock channels: `general`, `random`, `design`, `sales`, `engineering`,
`agents`, `watercooler`, `announcements`, `alice-tyler`, `bob-tyler`.

`scripts/post-screenshots.sh` hosts PNGs on a per-developer branch
(`agent-screenshots/<github-username>`) and posts a PR comment with
commit-SHA-based image URLs (immutable — safe from later overwrites):

```bash
./scripts/post-screenshots.sh 803 test-results/screenshots
./scripts/post-screenshots.sh 803 test-results/screenshots body.md  # custom body prepended
```

The body file supports `{{filename}}` placeholders (without `.png`) to inline
images at specific positions. Images not referenced by any placeholder are
appended at the end. Without placeholders, all images are appended (backward
compatible).

```markdown
### Unread dot
A message arrives in `#random`.

{{01-unread-dot}}

### Context menu
Right-click shows "Mark as read".

{{02-context-menu}}
```

Re-runs overwrite the image blobs on the `agent-screenshots/<username>`
branch, but the script **appends a new PR comment** — it does not edit or
delete the previous one. After reposting, delete the superseded comment so
only the current set remains, otherwise reviewers still see the stale images:

```bash
# List screenshot comments to find the stale one's id
gh pr view <pr> --repo agiterra/beekeeper --json comments \
  --jq '.comments[] | select(.body | test("pr-<pr>--")) | {id, url}'
gh api -X DELETE repos/agiterra/beekeeper/issues/comments/<stale-comment-id>
```

Branch cleanup when fully done: `git push origin --delete agent-screenshots/<username>`.

### Writing E2E Screenshot Specs

When screenshots need seeded state, live messages, or UI interaction before
capture, write a Playwright spec instead of using `just desktop-screenshot`.
Add specs to `desktop/tests/e2e/` and register them in `playwright.config.ts`
(`smoke` project `testMatch`). Every test calls `installMockBridge(page)` for
mock Tauri IPC. Mock pubkey, channel names, and UUIDs live in `e2eBridge.ts`.

**Always build with `pnpm build:e2e`, never `pnpm run build`.** The mock Tauri
bridge is compiled in only for `--mode e2e` (see `installE2eBridgeIfConfigured`
in `desktop/src/main.tsx`). A plain `pnpm run build` strips it, so
`window.__TAURI_INTERNALS__` is never defined and **every** mock-mode spec fails
with `Cannot read properties of undefined (reading 'invoke')` — the app renders
"Community connection failed" instead of the UI under test. That looks exactly
like a product bug rather than a build mistake, so it burns real time.
`pnpm test:e2e:smoke` and `pnpm test:e2e:integration` run the right build for
you; prefer them over a manual build plus `playwright test`.

**Stale server:** `reuseExistingServer: true` means a previous build's server
serves old code. Kill port 4173 and re-run `pnpm build:e2e` before re-running
tests after code changes.

**`addInitScript` before bridge:** `page.addInitScript` (localStorage seeding)
must run BEFORE `installMockBridge(page)` — React reads state on mount, the
bridge triggers mount.

**Live messages:** Call `waitForMockLiveSubscription(page, channelName)` before
`__BEEKEEPER_E2E_EMIT_MOCK_MESSAGE__` — messages are silently dropped without a
subscription. Navigate to the channel first (triggers subscription), then away
(so unread indicators appear), then inject.

**Animation timing:** Radix components animate in via CSS. `toBeVisible()`
resolves mid-animation — wait for completion before screenshotting. Use the
shared helper (mandatory before any `page.screenshot()` or
`locator.screenshot()` in specs):

```ts
import { waitForAnimations } from "../helpers/animations";

// ... after the element is visible but before capturing:
await waitForAnimations(page);
await page.screenshot({ path: "...", clip: { ... } });
```

The `just desktop-screenshot` path (`screenshot.mjs`) calls
`waitForAnimations` automatically — no manual step needed there.

For per-element waits (rare — prefer the page-level helper above):

```ts
await menuItem.evaluate((el) =>
  Promise.all(
    el.closest("[data-state]")?.getAnimations().map((a) => a.finished) ?? [],
  ),
);
```

**Cropping:** Use `clip` — full-window (1280x720) screenshots are unreadable
for sidebar features. Sidebar = 256px; context menus ~450px.

**Distinct states — verify before posting:** when one view renders many
elements at once (e.g. all team cards in a single grid), an unscoped
full-page `page.screenshot()` captures the *same* pixels for every shot, so
multiple PNGs come out byte-identical. Scope each shot to its subject with
`locator.screenshot()` (full-page `clip` only when an overlay like an open
dropdown must be included). Then gate on hash distinctness before posting:

```bash
shasum -a 256 test-results/<dir>/*.png   # every hash must be unique
```

Identical hashes mean two shots captured the same state — fix the spec, do
not post. This catches the most common screenshot regression.

**`general` has pre-seeded messages** making `hasUnread` always true. Use
`engineering` for "muted + no unread" visual states.

**PR comments:** Use a body template (3rd arg to `post-screenshots.sh`) with
`{{filename}}` placeholders. Each screenshot gets a `###` heading + one-line
description.

---

## Common Gotchas

1. **Kind `39000` for channel metadata, not `41`** — kind 41 is NIP-01 (unused). All kinds defined in `beekeeper-core/src/kind.rs`.
2. **Relay queries must specify `kinds`** — omitting `kinds` triggers the p-gate (403). Always include explicit kind filters.
3. **`messages search` chooses its own supported kinds** — do not add a `--kinds` option; the current command does not accept one. This differs from raw relay filters, which still need explicit kinds.
4. **Worktrees: `cd` in the same command** — shell CWD doesn't persist between tool calls. Use `cd /path && cargo build` as one command.
5. **Desktop crate excluded from root workspace** — `cargo test` at repo root does NOT run desktop tests. Use `cargo test --manifest-path desktop/src-tauri/Cargo.toml` explicitly.
6. **React render perf: `React.memo` is all-or-nothing** — it only skips a re-render when *every* prop is reference-stable; one unstable prop (inline arrow/JSX, or a hook returning a fresh `{}`/`[]`/`Map` each render) defeats it. Two repeat offenders: (a) React Query results (`useMutation`/`useQuery`) are a **new object each render** — depend on the stable method (`mutation.mutateAsync`), not the object; (b) derived `Map`/array state that recomputes on a version bump — wrap in a content-equality ref cache (`shared/hooks/useStableReference.ts`). When chasing interaction lag, **measure with DevTools closed and no perf probes** (an open Web Inspector + per-keystroke `console.log` inflate the numbers), and isolate by removing one suspect at a time rather than guessing.
7. **`pgschema` omits seed DML and some storage parameters** — Fresh desired-state bootstraps use `./bin/pgschema apply`, which does not execute `INSERT` statements or preserve every table storage parameter from `schema/schema.sql`. Put each unsupported invariant in `scripts/reconcile-schema-after-pgschema.sql` as an idempotent convergence statement plus a live catalog or data assertion. Every `pgschema apply` caller must run that script. A string assertion against `schema.sql` alone does not prove the pgschema-created database has the intended state.

---

## Desktop App

The desktop app is Tauri 2 + React 19 + Vite + Tailwind CSS. Features are
organized under `desktop/src/features/`. Biome handles linting and formatting.

```bash
just desktop-dev   # web-only dev server (faster iteration)
just dev           # full Tauri app with native shell
```

### Text sizing & zoom (use rem, never px)

The desktop app implements Cmd +/- zoom by scaling the root `<html>`
font-size (`desktop/src/app/useWebviewZoomShortcuts.ts`) and pinning the native
webview zoom. **Only rem-based text scales with zoom — hardcoded px text sizes
are frozen.**

So for any readable text, reach for rem-based Tailwind tokens, never arbitrary
px:

- ✅ Stock rem tokens (`text-base`, `text-sm`, `text-xs`, …). **Chat body/author
  text === `text-base` (16px) — chat is the app's base type size**, and the
  surrounding timeline elements (timestamps, system rows, code, reactions) are
  deliberate steps on that same stock ramp.
- ✅ The `text-2xs` (0.6875rem / 11px) and `text-3xs` (0.5rem / 8px) meta-text
  tokens (in `desktop/tailwind.config.js` under `theme.extend.fontSize`) for the
  sub-`text-xs` ramp — timestamps, count badges, tracking labels, tiny glyphs.
  These replaced the dozens of arbitrary `text-[…rem]` literals that had drifted
  apart pixel-by-pixel; keep meta text on these two tokens, not new arbitrary
  values.
- ❌ `text-[15px]`, `text-[13px]`, CSS `font-size: 15px` — px froze against zoom
  and caused the message-timeline regression (PR #891).
- ❌ Arbitrary rem literals too: `text-[0.6875rem]`, `text-[0.9rem]`, etc. They
  zoom fine but re-fragment the scale we consolidated. Use a named token.

Prefer stock tokens — they're rem and zoom-safe. Only if a design genuinely
needs a size the stock/`2xs`/`3xs` scale can't express should you **add a
rem-based token** (in `desktop/tailwind.config.js` under `theme.extend.fontSize`)
rather than an arbitrary literal. A CI guard (`pnpm check:px-text`, in
`desktop/scripts/check-px-text.mjs`) scans all of `desktop/src` and fails on any
new arbitrary text-size literal — px **or** rem/em. Genuinely decorative glyphs
(e.g. the `text-[6rem]` avatar emoji) are allowlisted by `path:line` in that
script.

### Community Switching

The desktop app supports multiple communities (each backed by a different relay).
Switching communities does **not** reload the page — it uses React key-based
remounting. `<AppReady key={communityKey} />` in `App.tsx` forces the entire
community-scoped subtree to unmount and remount with fresh state.

**Module-level singletons must be explicitly reset.** React remounting only
clears React state (useState, useRef, context). Module-level variables (Maps,
class instances, cached promises) survive across remounts. Every community-scoped
singleton needs a reset function wired into `resetCommunityState()` in
`desktop/src/features/communities/useCommunityInit.ts`.

`resetCommunityState()` is the canonical inventory of community-scoped
singletons. **If you add a new module-level cache, Map, or class instance that
holds community-scoped data, add its reset there in the same change.** Failure
to do so causes data from the old community to leak into the new one. Avoid
duplicating its complete reset list here; the implementation is the source of
truth.

Key files:
- `desktop/src/app/App.tsx` — community key, init gate, remount boundary
- `desktop/src/features/communities/useCommunityInit.ts` — `resetCommunityState()`, applies config to Tauri backend
- `desktop/src/main.tsx` — provider hierarchy (`QueryClientProvider` > `App`)

---

## Mobile App (Flutter)

The mobile app lives in `mobile/` — a Flutter app using Riverpod + Hooks.

### Architecture

- **State management:** Riverpod + `flutter_hooks` (`HookConsumerWidget`)
- **Theme:** Catppuccin Latte (light) / Macchiato (dark) — matches desktop
- **Features:** Isolated under `lib/features/`, shared code in `lib/shared/`
- **Nostr models:** `lib/shared/relay/nostr_models.dart` — event kinds must
  stay in sync with `desktop/src/shared/constants/kinds.ts`

### Rules

- **NEVER use `StatefulWidget`** — favor Riverpod for state and always use
  `HookConsumerWidget` or `ConsumerWidget` with `flutter_hooks` for local state.
- Agents may build and run the Flutter app when it materially helps implement,
  debug, or validate mobile changes. Prefer the smallest relevant command and
  reuse an already-running simulator/emulator and the app's configured staging
  or production community when that is sufficient. Do not start or rebuild
  local relay services unless the task specifically requires relay-side or
  isolated integration behavior.
- For iOS runtime validation, prefer `just mobile-dev`; it applies the
  worktree-specific debug identity and runs `flutter run`. Direct `flutter run`
  or IDE workflows are also allowed. Use `just mobile-build-android` only when
  an APK build is relevant to the task.
- Do not rebuild, reinstall, or relaunch merely for ceremony. Preserve Flutter's
  incremental build cache and use hot reload/restart where appropriate. Use
  `flutter clean` only when stale build artifacts are a credible cause. Run
  `flutter upgrade` only when the task explicitly requires a toolchain change.
- For user-visible or integration changes, exercise the affected workflow in a
  real app when practical and report the device/simulator, connected community,
  and workflow actually tested.
- **Do NOT use `print()`** — use `debugPrint()` or structured logging.
- Prefer `context.colors` and `context.textTheme` (via theme extensions)
  over raw `Theme.of(context)` calls.
- **Keep widgets small and composable.** One public widget per file; push
  private sub-widgets (`_Foo`) into sibling `part` files under a
  `<page>/` folder rather than growing the page file. Hard ceiling:
  **1000 lines/file**, enforced across Desktop, Web, and Mobile by the
  repository-level `just file-size-check` gate (`just check`, CI, and every
  pre-push). If the guard trips, **split the file — never bump the limit or add
  an override to slip under it.**
- Feature modules must not import from other feature modules — only from
  `shared/`.
- Use `Grid` tokens for spacing, `Radii` for border radius.

### Quality Checks

```bash
cd mobile
dart format --output=none --set-exit-if-changed .
flutter analyze
flutter test
```

Or from repo root: `just mobile-fmt` (auto-fix), `just mobile-check` (lint + fmt check), `just mobile-test` (tests).

To run the app locally with a worktree-specific debug identity and a
started or reused iOS Simulator:

```bash
just mobile-dev
```

This runs `flutter run` against the app's configured community; it does not
start Docker or local relay services.

When run from a git worktree, `just mobile-dev` (and `just
mobile-build-android`) give the debug build a per-worktree app identifier
(keyed to the worktree directory name) and a branch-labelled app name via
`scripts/mobile-worktree-overrides.sh`, so builds from multiple worktrees
install side by side. Release builds are unaffected. `just mobile-clean`
removes stale worktree-suffixed installs from simulators/emulators. See
[mobile/README.md](mobile/README.md) for direct Xcode / Android Studio
usage.

### Testing Conventions

- Prefer **widget tests** over unit tests for UI components — test the
  whole widget tree, not individual methods.
- Use `ProviderScope(overrides: [...])` to inject fake notifiers.
- Fake notifiers should extend the real notifier class and override `build()`.
- Use the `WidgetHelpers.testable()` wrapper for simple widget tests or
  build a custom `ProviderScope` + `MaterialApp` when you need specific overrides.

---

## See Also

- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, code style, PR process, how to add event kinds / CLI subcommands / HTTP endpoints
- [TESTING.md](TESTING.md) — multi-agent E2E test guide
- [ARCHITECTURE.md](ARCHITECTURE.md) — system design and component relationships
- [RELEASING.md](RELEASING.md) — release process: what `just release-desktop` prepares, and which publishing and signing lanes (inherited from Block and removed) must be rebuilt before releases ship
- [README.md](README.md) — project overview and quick start
