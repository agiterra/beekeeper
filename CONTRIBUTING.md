# Contributing to Beekeeper

> **Where this guide came from** — Beekeeper began as a fork of block/buzz, and
> most of this guide is inherited from there. It still describes the code
> accurately, which is why it is kept. What it does *not* describe is how this
> repo is run: a single `main` branch, ordinary short-lived topic branches that
> are rebased and never merged, no upstream to track, and CI on
> ci.agiterra.org. For that, see
> [docs/INTEGRATION.md](docs/INTEGRATION.md).

Welcome, and thank you for your interest in contributing! This guide will help
you get from zero to a merged pull request.

If you have questions that aren't answered here, [open an issue](https://github.com/agiterra/beekeeper/issues/new).

---

## Table of Contents

1. [Code of Conduct](#code-of-conduct)
2. [Before You Open a PR](#before-you-open-a-pr)
3. [Setting Up the Development Environment](#setting-up-the-development-environment)
4. [Running Tests](#running-tests)
5. [Code Style](#code-style)
6. [Making a Pull Request](#making-a-pull-request)
7. [Architecture Overview](#architecture-overview)
8. [Ecosystem](#ecosystem)
9. [How to Add a New Event Kind](#how-to-add-a-new-event-kind)
10. [How to Add a New MCP Tool](#how-to-add-a-new-mcp-tool)
11. [How to Add a New API Endpoint](#how-to-add-a-new-api-endpoint)
12. [License and CLA](#license-and-cla)

---

## Code of Conduct

This project follows the [Contributor Covenant v2.1](CODE_OF_CONDUCT.md).
By participating you agree to uphold these standards. Please report
unacceptable behavior to **conduct@buzz-relay.org**.

---

## Before You Open a PR

Before starting, search [open PRs](https://github.com/agiterra/beekeeper/pulls) and [open issues](https://github.com/agiterra/beekeeper/issues) for duplicates — someone may already be working on the same thing. When you open your PR, link the closest existing one in the description (or say "none found").

For anything beyond a small fix, opening an issue first is strongly recommended. Describe the problem and proposed solution so a maintainer can acknowledge the approach before you build — it avoids two people building the same thing in parallel.

Beekeeper is an agent platform, so AI-assisted PRs are welcome. No need to disclose the tools you used, but you own and must have reviewed the final code. Submissions that are clearly unreviewed may be closed with a pointer here.

We squash-merge, so your PR title becomes the commit subject in `main`. Use [Conventional Commits](https://www.conventionalcommits.org/) format: `feat(mcp): add get_feed_actions tool`. The type prefix (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`) is required. See the [Commit Messages](#commit-messages) section for the full reference.

### Sign Your Commits

```bash
git commit -s
```

Every commit needs a Developer Certificate of Origin (DCO) sign-off. The `-s` flag appends a `Signed-off-by` trailer that certifies you wrote the change and can contribute it under the project license. The **DCO Check** will block your PR without it.

#### Fix unsigned commits already pushed

```bash
git rebase --signoff main
git push --force-with-lease
```

#### Auto-setup for future commits

```bash
just hooks
```

This installs a `commit-msg` hook that adds the sign-off trailer automatically for `git commit` and `git merge`. Other flows (`git rebase`, `git cherry-pick`) still need their own flag — `--signoff` and `-s` respectively.

We review as capacity allows — focused PRs that follow this guide move fastest.

---

## Setting Up the Development Environment

### Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Rust | 1.88+ | Install via [rustup](https://rustup.rs/) |
| Node.js | 24+ | Required for desktop app commands and `just ci` |
| pnpm | 10+ | Required for desktop app commands and `just ci` |
| Flutter | 3.41+ | Required for mobile app — install via [flutter.dev](https://docs.flutter.dev/get-started/install) |
| git | 2.46+ | The relay authenticates git with a Nostr credential helper that answers over git's `authtype` credential protocol, which exists from 2.46 — Apple's bundled git 2.39 cannot authenticate at all |
| Docker | 24+ | For Postgres, Redis, RustFS (S3) |
| `just` | latest | Task runner — `cargo install just` |
| `lefthook` | 2.1.3 (Hermit-pinned) | Auto-installed by `just hooks` — no manual install needed |
| `sqlx` migrations | workspace crate | `just migrate` applies embedded migrations from `migrations/` |

This repo uses [Hermit](https://cashapp.github.io/hermit/) for toolchain
pinning. Activate it once per shell session:

```bash
. ./bin/activate-hermit
```

Hermit pins Rust, `just`, Node, pnpm, and other tools to the versions in
`bin/`. Each tool is downloaded on first use. You can also run `just bootstrap`
(which `just setup` calls automatically) to pre-download all required tools
upfront. If you don't use Hermit, ensure your toolchain meets the minimum
versions in the table above.

#### Linux: Tauri system libraries

Hermit pins language toolchains, not system libraries. On Linux, the desktop
app's Rust crates link against GTK and WebKitGTK, so `just ci` (and any
`just desktop-tauri-*` recipe) needs these installed system-wide first. On
Debian/Ubuntu:

```bash
sudo apt-get install -y --no-install-recommends \
  build-essential curl file libasound2-dev libayatana-appindicator3-dev \
  libgtk-3-dev librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev libxdo-dev \
  patchelf wget
```

This is the list the inherited `.github/workflows/ci.yml` installs for its
Linux desktop jobs. GitHub Actions are disabled on agiterra's GitHub copy, so
that workflow is a reference rather than a running check; the live gate is
Woodpecker (`.woodpecker/`). Other distributions ship these
under different package names — see the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for the
equivalents.

Without them, `just ci` fails partway through `just check` with a pkg-config
error such as:

```
The system library `gdk-pixbuf-2.0` required by crate `gdk-pixbuf-sys` was not found.
```

If you're only touching the relay, CLI, or other server-side crates, you can
skip this and run the narrower recipes instead — `just fmt-check`, `just
clippy`, `just test-unit`, and `just test` need no GTK.

### First-Time Setup

```bash
# 1. Clone the repo
git clone https://github.com/agiterra/beekeeper.git
cd beekeeper

# 2. Activate Hermit (optional but recommended)
. ./bin/activate-hermit

# 3. Bootstrap tools + infrastructure
just setup

# 4. Install Git hooks (optional, recommended)
just hooks
```

`just setup` runs `just bootstrap` first — it copies `.env.example` to `.env`
if it doesn't already exist, and invokes `cargo`, `node`, and `pnpm` to trigger
Hermit's lazy tool download (each tool is fetched once on first invocation and
cached thereafter). You can also run `just bootstrap` independently at any time;
it is safe to re-run.

`just setup` then starts Docker services (Postgres on `:5432`, Redis on `:6379`,
Adminer on `:8082`, Keycloak on `:8180` for local OAuth/OIDC testing, RustFS on
`:9000` for media and git object storage, and Prometheus on `:9090` for metrics) and runs all
pending database migrations.

### Running the Relay and Desktop App

```bash
just dev   # starts the relay + desktop app in one command
```

`just dev` builds all agent tools, starts the relay (`ws://localhost:3000`) in
the background, and launches the Tauri desktop app. The relay process is
automatically killed when you quit the app or press Ctrl+C.

For a split-terminal workflow (relay logs visible separately from Vite output):

```bash
just relay        # terminal 1 — relay on ws://localhost:3000
just desktop-dev  # terminal 2 — Vite dev server only (no Tauri shell)
```

### Stopping / Resetting

```bash
just down    # Stop Docker services, keep data
just reset   # Wipe all dev state and recreate it; installed Beekeeper is preserved
```

Development desktop state uses separate bundle identifiers
(`io.agiterra.beekeeper.app.dev` and per-worktree variants), a separate keyring service
(`beekeeper-desktop-dev`), and `~/.buzz-dev`. `just reset` removes those dev-only
locations and the local Docker volumes. It does not touch the installed app's
`io.agiterra.beekeeper.app` data, `beekeeper-desktop` keyring service, or
`~/.beekeeper` nest.

---

## Running Tests

### Unit Tests (no infrastructure required)

```bash
just test-unit
```

Unit tests are self-contained and run without Docker. They cover event
parsing, filter matching, auth logic, workflow YAML parsing, and more.

### Integration Tests (requires running infrastructure)

```bash
just test
```

Integration tests spin up the relay and exercise the full stack — WebSocket
connections, NIP-42 auth, event ingestion, search indexing, and workflow
execution. `just test` starts Docker services automatically if they're not
already running.

### End-to-End Tests

End-to-end tests live in `crates/beekeeper-test-client/tests/`:

- `e2e_relay.rs` — WebSocket relay tests
- `e2e_mcp.rs` — MCP tool tests
- `e2e_nostr_interop.rs` — Nostr protocol interoperability tests
- `e2e_media.rs` — media upload/download tests
- `e2e_media_extended.rs` — extended media tests (GIF, image processing)

Run them with (requires running infrastructure):

```bash
cargo test -p beekeeper-test-client -- --ignored
```

See `TESTING.md` for the full multi-agent E2E testing guide.

### CI Gate

Before opening a PR, run the full CI gate locally:

```bash
just ci
# Runs: check + unit tests + desktop build + Tauri check + mobile tests
```

This is the same check that runs in CI. PRs that fail `just ci` will not be
merged. If `just ci` fails on formatting, `just fix-all` fixes it in one shot (`rustfmt` + Tauri fmt + desktop, web, and mobile formatters).

### What pre-push runs, and what it does not

`pre-push` runs a **floor**, not the gate. It is scoped to what your push
actually changed, measured against the merge base with `main`:

- Rust `fmt --check` and unit tests for the crates whose own sources moved.
- `clippy --all-targets -- -D warnings` for those crates **and their
  dependents** — an API break shows in the crates that depend on you, not in
  the one you edited. The dependent set comes from `cargo metadata`, across
  both workspaces, so a `beekeeper-core` change reaches `beekeeper-desktop` too.
- `pnpm check`, `pnpm typecheck` and `pnpm test` when `desktop/` outside
  `src-tauri` moved; `just web-test` for `web/`; `just mobile-test` for
  `mobile/`.
- The repository-wide file-size ratchet, always and unfiltered — its own
  merge-base diff is its path filter.

A path that maps to no scope selects the **full floor**, never nothing, and
the run says which path did it. Three things are guards rather than tests, so
they are unconditional and unbudgeted: the push-destination tripwire, the
branch-skew check, and the `commit-msg` DCO trailer.

**`cargo test --workspace`, every e2e project, `just ci` and `just check` are
CI-only.** They do not run on your machine at push time. That is a deliberate
trade: a class of failure — a change that compiles and passes its own crate's
tests but breaks a crate downstream of it, or an e2e path — now reaches CI
instead of the pushing machine. It buys a push that finishes in about two
minutes instead of one that times out, which is what stopped everyone from
passing `--no-verify` and running no gate at all. Run `just ci` yourself before
you open a PR; the floor does not replace it.

Every push prints one line saying what ran, what it skipped and why, and what
it cost against a 120-second budget:

```
pre-push floor: beekeeper-cli (file-size, fmt, clippy, 847 tests) 71s / budget 120s · skipped: desktop — no desktop/ change, web — no web/ change, mobile — no mobile/ change, workspace tests — CI, e2e — CI — run in CI
```

The budget is **disclosed, not enforced**: going over prints `over budget by
Ns` and still reports the real result. A floor that killed a nearly-finished
clippy run would teach everyone to pass `--no-verify` again.

Push normally — `git push origin <branch>`. Do not pass `--no-verify` to skip
the floor. The one place it is still correct is landing a batch, where `just
ci` has already passed on the exact SHA and the hooks would outlive the
relay's NIP-98 timestamp window; see `docs/INTEGRATION.md` § "Landing a batch".

The floor's own proofs: `bash scripts/test-pre-push-floor.sh` (end to end,
stubbed toolchain) and `node --test scripts/pre-push-floor-scope.test.mjs`
(the path → scope mapping).

---

## Code Style

### Formatting

We use `rustfmt` with default settings. Format your code before committing:

```bash
cargo fmt --all
```

To check without modifying:

```bash
cargo fmt --all -- --check
```

### Linting

We use `clippy` with warnings-as-errors:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Fix all clippy warnings before submitting a PR. If you believe a warning is
a false positive, add a targeted `#[allow(...)]` with a comment explaining
why.

### No Unsafe Code

All crates enforce `#![deny(unsafe_code)]`. Do not add unsafe blocks. If you
believe unsafe is genuinely necessary, open an issue first to discuss the
approach.

### Error Handling

- Use `thiserror` for library error types.
- Use `anyhow` for binary / application-level error propagation.
- Do not use `unwrap()` or `expect()` in production code paths. Use `?` or
  explicit error handling. `unwrap()` is acceptable in tests.

### Logging and Tracing

Use the `tracing` crate for all instrumentation. Prefer structured fields
over string interpolation:

```rust
// Good
tracing::info!(channel_id = %id, event_kind = kind, "Event ingested");

// Avoid
tracing::info!("Event ingested: channel={id} kind={kind}");
```

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(mcp): add get_feed_actions tool
fix(auth): reject expired NIP-42 challenges
docs(agents): document workflow MCP tools
refactor(db): extract channel queries into channel.rs
test(workflow): add approval gate integration test
```

The type prefix (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`) is
required. The scope (in parentheses) is optional but encouraged.

---

## Making a Pull Request

### What a Good PR Looks Like

1. **Focused** — one logical change per PR. If you're fixing a bug and
   refactoring a module, split them into two PRs.

2. **Tested** — new behavior has tests. Bug fixes include a regression test.
   If a test is impractical, explain why in the PR description.

3. **Documented** — public APIs, new event kinds, new MCP tools, and new
   config variables are documented. Update `README.md`, `AGENTS.md`, or
   `VISION.md` as appropriate.

4. **CI passing** — `just ci` passes locally before you push.

5. **Clear description** — the PR description explains:
   - What problem this solves (or what feature it adds)
   - How it was implemented (key decisions, trade-offs)
   - How to test it manually (if applicable)
   - Any follow-up work deferred to a future PR

6. **Shows the UI** — any PR that changes the desktop or mobile UI includes
   before/after screenshots (or a short recording for interactions) in the
   description. We can't run every branch locally — screenshots let us review
   UI changes same-day instead of waiting for someone to build your branch.

### PRs We're Unlikely to Merge

Some kinds of PRs usually get closed — not because they're bad ideas, but
because we can't safely review them without prior discussion:

- **Large refactors or dependency swaps** without a prior issue agreeing on
  the direction
- **Cosmetic renames or style-only churn** that doesn't fix a bug or improve
  clarity
- **Entirely new features** with no prior discussion
- **Drive-by changes bundled into an unrelated fix** — split them out

If you're considering any of these, open an issue first and we'll tell you
quickly whether it's a direction we'd merge. That saves your time as much as
ours.

### What to Expect After You Open a PR

- Maintainers triage new PRs on a best-effort cadence. Focused PRs that
  follow this guide move fastest.
- Duplicates and PRs that skip this guide may be closed with a pointer here
  rather than a full review. A close isn't a rejection of you or the idea —
  address the gaps and reopen (or open a fresh PR) anytime.
- Address review comments by pushing new commits (don't force-push during
  review; it makes it hard to see what changed).
- Once approved, a maintainer will squash-merge your PR.

---

## Architecture Overview

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full system design and
[AGENTS.md](AGENTS.md#repo-structure) for the complete crate map. The key
design principles:

**The relay is the single source of truth.** All state flows through the
event store. Crates communicate through the database and Redis pub/sub — not
through direct function calls across crate boundaries (with the exception
of `beekeeper-core` types, which are shared everywhere).

**Event kinds are the only switch.** Every action in the system — a message,
a reaction, a workflow step, a canvas update — is a Nostr event with a kind
integer. Adding a new feature means defining a new kind. No breaking changes
to existing clients.

---

## Ecosystem

This repo holds all of the application code — the relay, desktop app, web
client, mobile app, CLI and agent harness — and its own deploy configuration
under `deploy/`. The project's roles, plans and ledger live in a second
repository on the relay; everything else is here.

See [AGENTS.md § Ecosystem](AGENTS.md#ecosystem) for the repo and pipeline
diagram, and [RELEASING.md](RELEASING.md) for the release process.

**Contributors:** fork [agiterra/beekeeper](https://github.com/agiterra/beekeeper),
open a PR, and CI runs automatically. No special access is required. Note that
GitHub is a mirror: `hive.agiterra.org` is canonical and is where maintainers
push, so a merged PR reaches GitHub by way of the relay.

---

## How to Add a New Event Kind

1. **Define the kind constant** in `beekeeper-core/src/kind.rs`:

   ```rust
   /// My new event kind — description of what it represents.
   pub const KIND_MY_FEATURE: u32 = 4XXXX;
   ```

   Pick a kind number in the appropriate sub-range defined in `kind.rs`.
   Check the `ALL_KINDS` array for collisions. Each sub-range is documented
   with comments in the file.

2. **Define the payload type** in the appropriate module in `beekeeper-core/src/`
   (e.g., alongside `event.rs`) if the content field is structured JSON:

   ```rust
   #[derive(Debug, Serialize, Deserialize)]
   pub struct MyFeaturePayload {
       pub field_one: String,
       pub field_two: Option<u64>,
   }
   ```

3. **Register the kind's required scope** in
   `crates/beekeeper-relay/src/handlers/ingest.rs` inside
   `required_scope_for_kind()`. This controls which auth scope a caller
   needs to submit the event:

   ```rust
   KIND_MY_FEATURE => Ok(Scope::MessagesWrite),
   ```

4. **Handle post-storage side effects** by adding a match arm in
   `crates/beekeeper-relay/src/handlers/side_effects.rs` inside
   `handle_side_effects()`:

   ```rust
   KIND_MY_FEATURE => handle_my_feature(event, state).await?,
   ```

   `handle_side_effects()` runs after the event is stored — use it for
   notifications, cache invalidation, or derived data. If the new kind
   also needs an HTTP bridge surface (for example, a protocol helper that
   cannot practically use WebSocket), add a handler in
   `crates/beekeeper-relay/src/api/` and register it in
   `crates/beekeeper-relay/src/router.rs`.

5. **Persist to the database** — if the event needs to be queryable, add a
   handler in `beekeeper-db/src/` (e.g., `beekeeper-db/src/my_feature.rs`) with
   the appropriate `INSERT` and `SELECT` queries.

6. **Index for search** (if applicable) — Postgres FTS indexes persisted
   events automatically via the `events.search_tsv` generated column. To
   exclude a privacy-sensitive kind from search, add it to the `CASE WHEN
   kind IN (...)` exclusion in the `search_tsv` definition (see the initial
   schema migration) rather than wiring a separate indexer.

7. **Audit** — the audit log captures all events automatically; no changes
   needed unless you need custom audit metadata.

8. **Write tests** — add a unit test for payload serialization in
   `beekeeper-core` and an integration test in `beekeeper-test-client` that sends
   the new event kind and verifies the expected behavior.

9. **Document** — `kind.rs` is the authoritative registry of all kind numbers.
   Update `README.md` if it's a user-facing feature.

---

## How to Add a New API Endpoint

Prefer a signed Nostr event and the existing WebSocket/`POST /events` ingest
path over adding endpoint-specific JSON APIs. The relay intentionally exposes
only a narrow HTTP surface: NIP-11/NIP-05 metadata, `/events`, `/query`,
`/count`, `/hooks/{id}`, Blossom media, git smart HTTP, git policy hooks, and
health probes.

If an HTTP endpoint is still necessary:

1. **Define the handler** in the appropriate module under
   `crates/beekeeper-relay/src/api/`. Resolve the request tenant before any auth or
   data lookup, use NIP-98 when the endpoint accepts user credentials, and keep
   community scoping explicit.

2. **Register the route** in `crates/beekeeper-relay/src/router.rs` using the
   narrowest path possible. Do not add new `/api/*` compatibility routes unless
   the product decision explicitly calls for one.

3. **Add database queries** in `beekeeper-db/src/` only when the endpoint cannot be
   expressed through the existing event query paths.

4. **Handle errors** using the `api_error()`, `internal_error()`, and
   `not_found()` helpers in `beekeeper-relay/src/api/mod.rs`. Return
   `(StatusCode, Json<Value>)` tuples.

5. **Write tests** with the `beekeeper-test-client` harness in
   `crates/beekeeper-test-client/tests/`, covering auth, community scoping, and the
   relevant success path.

6. **Document** any public endpoint in `ARCHITECTURE.md` and user-facing docs.

---

## License and CLA

Beekeeper is licensed under the **Apache License, Version 2.0**. See
[LICENSE](LICENSE) for the full text.

By submitting a pull request, you agree that your contribution is licensed
under the Apache 2.0 license and that you have the right to submit it.

If your employer has rights to intellectual property you create, you may need
their sign-off. When in doubt, check with your legal team.

---

*Thank you for contributing to Beekeeper. Every bug report, documentation fix,
and code contribution makes the project better for everyone. 🐝*

---

## Sharing local commits (opt-in)

Project Pulse can show what you have committed locally but not yet opened a PR
for. It learns this from a git ref, never from anybody being asked to report:
`just hooks` installs a `post-commit` hook that pushes the commit you just made
to `refs/heads/wip/<your-pubkey8>/<branch>` — a namespace only your key writes,
so two people on the same branch never overwrite each other.

**It is off until you turn it on.** The hook exits immediately unless this
checkout has `buzz.wipShare=true`, and only one thing sets that:

```bash
just wip-share-on    # start sharing from this checkout
just wip-share-off   # stop
```

`just setup` and `just hooks` install the hook; neither arms it.

What the hook does, and does not do:

- It pushes **`HEAD`** — the commit git has already made. It never inspects
  your working tree, stages anything, or stashes anything, so uncommitted work
  cannot leave your machine.
- It force-pushes to `refs/heads/wip/*` and refuses any other ref name.
- It resolves the remote from your own push configuration
  (`branch.<name>.pushRemote`, `remote.pushDefault`, `branch.<name>.remote`, or
  the single remote when there is exactly one) — never a hard-coded name.
  Set `buzz.wipRemote` to pin it.
- It **pushes** under your own Nostr key, via `git-credential-nostr`. See
  [docs/INTEGRATION.md](docs/INTEGRATION.md) § Pushing to the relay, and run
  `just install-git-credentials` first.
- It does **not** configure commit signing. `just wip-share-on` sets only
  `buzz.wipShare` and `buzz.wipIdentity`; your commits are signed only if this
  checkout is already configured for `git-sign-nostr` (`gpg.format`,
  `gpg.x509.program`, `commit.gpgsign`, `user.signingkey`). A seat's hooks are
  different — the hire host writes that signing config into the seat's own
  worktree, because it minted that seat's key. Nothing here will sign as you
  without your having asked for it.
- `just wip-share-on` refuses rather than guessing when it cannot resolve your
  pubkey, because a ref shared between people is worse than no ref.
- Every failure is silent to your commit and logged to
  `.git/buzz-wip-push.log`. A post-commit hook that fails a commit is worse
  than no hook.

Wip refs are pruned by `bee pulse prune-wip` when their branch merges, or after
**30 days** without moving.

If you never turn this on, Pulse says `{Who}'s local commits: not shared`. That
sentence is about **what the relay holds** — there is no wip ref on the wire for
that person — and not a claim about how anyone configured their machine.
