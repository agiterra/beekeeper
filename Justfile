# Buzz — development task runner

set dotenv-load := true

desktop_dir := "desktop"
desktop_tauri_manifest := "desktop/src-tauri/Cargo.toml"
web_dir := "web"

# Opt-in mesh-llm. Off by default so `just dev`/`just staging`/`just production`
# skip ~420 extra crates + the llama.cpp native runtime build and stay fast to
# iterate on. Turn on to test mesh compute features: `just mesh=1 dev` /
# `just mesh=1 staging` / `just mesh=1 production`.
mesh := ""

# Reset only the current standalone desktop instance before launch.
# Usage: `just fresh=1 desktop-standalone`.
fresh := ""

# Disable the OS-keyring secret backend for dev desktop builds: secrets fall
# back to 0600 files under the instance's app-data dir, so rebuilt (unsigned)
# dev binaries never trigger macOS keychain password prompts. Opt in per shell
# (`just nokeyring=1 desktop-standalone`) or permanently via
# `export BUZZ_DESKTOP_NOKEYRING=1` in your shell profile.
nokeyring := env_var_or_default("BUZZ_DESKTOP_NOKEYRING", "")

# List all available tasks
default:
    @just --list

# ─── Dev Environment ─────────────────────────────────────────────────────────

# Install required dev tools via Hermit and create .env (safe to re-run)
bootstrap:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    # Hermit's bin/ symlinks auto-download pinned tool versions on first use.
    # Running each tool once triggers the download if not already cached.
    echo "Ensuring toolchain via Hermit..."
    cargo --version &
    node --version &
    pnpm --version &
    wait
    if ! command -v docker &>/dev/null; then
        echo "Error: Docker is required but not installed."
        echo "Install it from https://docs.docker.com/get-docker/"
        exit 1
    fi
    if [[ ! -f .env ]]; then
        cp .env.example .env
        echo "Created .env from .env.example — review it before running just dev."
    fi
    ./scripts/ensure-local-relay-key.sh .env

# Start Docker services, run migrations, install desktop deps
setup: bootstrap
    ./scripts/dev-setup.sh

# Install git hooks via lefthook (dispatches from the shared .git/hooks dir so all
# linked worktrees inherit the same hooks without a worktree-relative .hooks path)
hooks:
    #!/usr/bin/env bash
    set -euo pipefail
    # Use the Hermit-pinned lefthook (bin/lefthook self-downloads on first use):
    # works with no pre-installed lefthook and guarantees the pinned version
    # rather than whatever happens to be on PATH.
    export PATH="{{justfile_directory()}}/bin:$PATH"
    # --path-format=absolute guarantees an absolute path from every invocation context:
    # without it, --git-common-dir returns ".git" from the main checkout and a
    # relative hooksPath would break linked-worktree dispatch just like .hooks did.
    HOOKS_DIR="$(git rev-parse --path-format=absolute --git-common-dir)/hooks"
    git config --local core.hooksPath "$HOOKS_DIR"
    lefthook install --force

# Turn on sharing your local commits under a wip ref (opt-in, per checkout).
# `just hooks` installs the hook; this is the only thing that arms it.
#
# Records `buzz.wipIdentity` — the first 8 hex of your own pubkey — because the
# ref namespace is per person. An earlier draft used a literal `human` segment
# and two people on the same branch force-pushed over each other, with kind
# 30618 then naming only the last pusher. Refuses rather than guessing: a ref
# shared between people is worse than no ref.
wip-share-on:
    #!/usr/bin/env bash
    set -euo pipefail
    identity="$(git config --get user.signingkey 2>/dev/null || true)"
    if ! printf '%s' "$identity" | grep -Eq '^[0-9a-f]{64}$'; then
        identity=""
        if command -v bee >/dev/null 2>&1; then
            identity="$(bee git status 2>/dev/null                 | sed -n 's/.*"effective_pubkey"[[:space:]]*:[[:space:]]*"\([0-9a-f]\{64\}\)".*/\1/p'                 | head -n 1)"
        fi
    fi
    if ! printf '%s' "$identity" | grep -Eq '^[0-9a-f]{64}$'; then
        echo "Cannot resolve your Nostr pubkey, so this checkout has no ref of its own to push to." >&2
        echo "Set it with 'git config user.signingkey <your 64-hex pubkey>', or run 'just install-git-credentials' and 'bee git setup', then try again." >&2
        exit 1
    fi
    short="$(printf '%s' "$identity" | cut -c1-8)"
    git config --local buzz.wipIdentity "$short"
    git config --local buzz.wipShare true
    echo "Sharing local commits under refs/heads/wip/$short/<branch> — a namespace only your key writes."
    echo "Wip refs are pruned on branch merge or after 30 days. Turn it off with: just wip-share-off"

# Stop sharing your local commits. Refs already pushed stay until they are
# pruned — see CONTRIBUTING.md § Sharing local commits (opt-in).
wip-share-off:
    #!/usr/bin/env bash
    set -euo pipefail
    git config --local --unset buzz.wipShare || true
    echo "Local commits are no longer shared from this checkout."

# Wipe development state and recreate a clean environment. Installed Buzz is preserved.
[confirm("This will DELETE all development data and preserve installed Buzz. Continue? (y/N)")]
reset:
    ./scripts/dev-reset.sh --yes

# Stop all dev services (keep data)
down:
    docker compose down

# Show dev service status
ps:
    docker compose ps

# Run the pre-push floor BEFORE git opens the connection, then push. Fixes
# ledger 178(n): git mints its NIP-98 credential at ref discovery, before any
# pre-push hook runs, and reuses it for the whole push, so a floor long enough
# to outlive the relay's +-900s token window fails `HTTP 401` with every
# check green. Args pass straight to `git push` — `just push`, `just push
# origin main`, `just push origin work/my-branch:main`. See
# scripts/push-with-floor.sh and docs/INTEGRATION.md § Pushing to the relay.
push *args:
    ./scripts/push-with-floor.sh {{args}}

# Install git-credential-nostr and configure git to push to the relay's own git
# hosting. Config is URL-scoped to the relay's /git path, so whatever already
# serves GitHub (osxkeychain, gh, a PAT) is untouched.
#
# This writes NO key material. The helper reads `git config nostr.keyfile`;
# putting your nsec there is yours to do, deliberately — see the note the
# recipe prints when the file is missing.
install-git-credentials relay="":
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    RELAY="{{relay}}"
    if [[ -z "$RELAY" ]]; then
        # Derive from the remote rather than hard-coding a host: the relay URL
        # differs per developer and per community, and a wrong default would
        # write credentials scoped to a host that never sees a request.
        RELAY="$(git remote get-url origin 2>/dev/null || true)"
        if [[ -z "$RELAY" ]]; then
            echo "No 'origin' remote to derive the relay URL from." >&2
            echo "Pass one: just install-git-credentials https://relay.example" >&2
            exit 1
        fi
        # Keep scheme://host[:port]; the helper scope is derived from that.
        RELAY="$(printf '%s' "$RELAY" | sed -E 's#^([a-z]+://[^/]+).*#\1#')"
    fi
    # --root is load-bearing: hermit pins CARGO_HOME to
    # {{justfile_directory()}}/.hermit/rust, so a bare `cargo install` lands the
    # binary inside the repo, off PATH, and a `hermit clean` deletes it. Install
    # somewhere the user's shell already looks.
    INSTALL_ROOT="${CARGO_INSTALL_ROOT:-$HOME/.local}"
    cargo install --quiet --path crates/git-credential-nostr --root "$INSTALL_ROOT"
    HELPER="$INSTALL_ROOT/bin/git-credential-nostr"
    echo "Installed $HELPER"
    # Prefer the bare `nostr` shorthand when git can find it itself — that
    # survives a later reinstall moving the file. Fall back to the absolute
    # path when the install root is not on PATH, rather than writing a config
    # entry that silently resolves to nothing.
    if command -v git-credential-nostr >/dev/null 2>&1; then
        cargo run --quiet -p buzz-cli -- --relay "$RELAY" git setup
    else
        echo "note: $INSTALL_ROOT/bin is not on PATH — pinning the absolute path instead."
        cargo run --quiet -p buzz-cli -- --relay "$RELAY" git setup --helper "$HELPER"
    fi
    echo
    cargo run --quiet -p buzz-cli -- --relay "$RELAY" git status

# There is no other standalone install path for `bee` or `beekeeper-host`: the
# desktop app ships them as sidecars inside its bundle, which is no use on a
# server and no use to a shell. `bee host install` then registers the host to
# start at login (macOS) or as a systemd user service (Linux) — and it needs an
# absolute path to a binary that will still be there, which is what this gives.
#
# Install `bee` and `beekeeper-host` where your shell and launchd can find them
install-bee:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    # --root is load-bearing: hermit pins CARGO_HOME to
    # {{justfile_directory()}}/.hermit/rust, so a bare `cargo install` lands the
    # binaries inside the repo, off PATH, and a `hermit clean` deletes them —
    # taking the login registration's target with them. Install somewhere the
    # user's shell already looks. (Same reasoning as install-git-credentials.)
    INSTALL_ROOT="${CARGO_INSTALL_ROOT:-$HOME/.local}"
    cargo install --quiet --path crates/buzz-cli --root "$INSTALL_ROOT"
    cargo install --quiet --path crates/beekeeper-host --root "$INSTALL_ROOT"
    echo "Installed $INSTALL_ROOT/bin/bee"
    echo "Installed $INSTALL_ROOT/bin/beekeeper-host"
    if ! command -v bee >/dev/null 2>&1; then
        echo
        echo "note: $INSTALL_ROOT/bin is not on PATH — add it, or call the binaries by path."
    fi
    echo
    echo "Next, on a machine that should run agents without a desktop app:"
    echo "  bee host install          # register it to start at login"
    echo "  bee host installed        # confirm, and hear about anything missing"
    if [[ "$(uname -s)" == "Linux" ]]; then
        echo
        echo "On a server, also: loginctl enable-linger \"$USER\""
        echo "Without lingering the host stops the moment you log out."
    fi

# Tail all service logs
logs *ARGS:
    docker compose logs -f {{ARGS}}

# ─── Build & Check ───────────────────────────────────────────────────────────

# Build the Rust workspace
build:
    cargo build --workspace

# Build the Rust workspace in release mode
build-release:
    cargo build --workspace --release

# Run repo lint, formatting, and repository policy checks
check: fmt-check clippy desktop-check desktop-tauri-fmt-check desktop-tauri-clippy web-check web-test mobile-check file-size-check ignore-reasons-check autodeploy-test sidecar-parity-check

# Test the relay deployers (deploy/autodeploy). They stub incus, flock and
# sleep on PATH, so they need no host, no containers and no Woodpecker — and
# they run in about a second. Every case is a regression test for a failure
# that reached production, most of them silent ones.
autodeploy-test:
    ./deploy/autodeploy/tests/config-contract.sh
    ./deploy/autodeploy/tests/autodeploy-behavior.sh
    ./scripts/test-woodpecker-path-filter.sh

# A sibling, not a directory inside this checkout: the whole point of moving
# those documents out was that an agent working on the code should not trip
# over the plans, and a path under this tree would put them straight back.
# Needs relay credentials the same way `git fetch origin` does — run
# `just install-git-credentials` first if this asks for a username.
#
# Clone or fast-forward the agents repository to ../agiterra-beekeeper-agents
agents-repo:
    #!/usr/bin/env bash
    set -euo pipefail
    # The MAIN checkout, not this worktree. `justfile_directory()` in a
    # worktree under .worktrees/ would put the clone inside the code tree —
    # precisely what the sibling rule exists to prevent — and give every
    # worktree its own copy of a 1.8 MB ledger. The common git dir is shared
    # by every worktree and its parent is always the main checkout.
    main="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
    dest="$(cd "$(dirname "$main")" && pwd)/agiterra-beekeeper-agents"
    url="https://hive.agiterra.org/git/6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2/bee-keeper-beekeeper-agents"
    if [ -d "$dest/.git" ]; then
        git -C "$dest" fetch origin main
        # --ff-only, never a merge: a local edit that cannot fast-forward is
        # something to look at, not something to silently resolve.
        git -C "$dest" pull --ff-only origin main
    else
        git clone "$url" "$dest"
    fi
    echo "agents repository: $dest"
    echo "  the map    $dest/plans/CURRENT_STATE.md"
    echo "  the ledger $dest/plans/SESSION_STATE.md"

# Run the repository-wide differential file-size ratchet and its policy tests.
# The ratchet inspects only files changed from the merge base, so this stays
# cheap enough to run unconditionally without duplicating path filters.
file-size-check:
    node --test scripts/check-file-sizes-core.test.mjs
    node desktop/scripts/check-file-sizes.mjs
    node web/scripts/check-file-sizes.mjs
    node mobile/scripts/check-file-sizes.mjs

# Ratchet on bare `#[ignore]` attributes. An ignored test is invisible; with a
# reason string the gap is legible and greppable, which is how `test-genesis`
# is able to select ignored proofs by name at all. Ratchet DOWN only — add the
# reason rather than raising the baseline.
ignore-reasons-check:
    node scripts/check-ignore-reasons.mjs

# Format all Rust code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy with warnings as errors
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Install JS dependencies (pnpm workspace — installs all packages from root)
desktop-install:
    pnpm install

# Re-derive every banked conformance corpus against this repo's own
# implementation. Deliberately not wired into `just ci` here — CI wiring is
# the glue phase's call, and a corpus that gates the build before its owner
# has agreed to that is a surprise, not a guarantee.
conformance-check:
    node --test "conformance/**/*.test.mjs"

# The export-viewer release manifest script, checked on its own because the
# conformance corpus binds it and nothing else runs it.
export-viewer-manifest-test:
    node --test "scripts/export-viewer-release-manifest.test.mjs"

# Install JS dependencies reproducibly for CI (pnpm workspace)
desktop-install-ci:
    pnpm install --frozen-lockfile

# Run desktop lint and format checks
desktop-check:
    cd {{desktop_dir}} && pnpm check

# Fix desktop lint and format issues
desktop-fix:
    cd {{desktop_dir}} && pnpm exec biome check --write .

# Run desktop TS helper unit tests
desktop-test:
    cd {{desktop_dir}} && pnpm test

# Run desktop TypeScript checks
desktop-typecheck:
    cd {{desktop_dir}} && pnpm typecheck

# Build desktop frontend assets
desktop-build:
    cd {{desktop_dir}} && pnpm build

# Format desktop Tauri Rust code
desktop-tauri-fmt:
    cargo fmt --manifest-path {{desktop_tauri_manifest}} --all

# Check desktop Tauri Rust formatting
desktop-tauri-fmt-check:
    cargo fmt --manifest-path {{desktop_tauri_manifest}} --all -- --check

# Format all code (Rust + Tauri Rust + Dart)
fmt-all: fmt desktop-tauri-fmt mobile-fmt

# Fix all formatting and lint issues
fix-all: fmt desktop-tauri-fmt desktop-fix web-fix mobile-fix

# Ensure sidecar placeholder binaries exist (Tauri validates externalBin at compile time)
# Sidecar binary list must stay in sync with desktop-release-build below.
_ensure-sidecar-stubs:
    #!/usr/bin/env bash
    set -euo pipefail
    TARGET=$(rustc -vV | sed -n 's|host: ||p')
    mkdir -p desktop/src-tauri/binaries
    SIDECARS=(buzz-acp buzz-agent buzz-dev-mcp git-credential-nostr bee)
    if [[ "$TARGET" != *windows* ]]; then
        SIDECARS+=(buzz-backend-kubernetes buzz-shell-host beekeeper-host)
    fi
    for bin in "${SIDECARS[@]}"; do
        touch "desktop/src-tauri/binaries/${bin}-${TARGET}"
    done

# Ensure Docker dev services (Postgres, Redis, etc.) are running and healthy
_ensure-services:
    #!/usr/bin/env bash
    set -euo pipefail
    pg=$(docker inspect --format '{{"{{"}}.State.Health.Status{{"}}"}}' buzz-postgres 2>/dev/null || echo "not_found")
    redis=$(docker inspect --format '{{"{{"}}.State.Health.Status{{"}}"}}' buzz-redis 2>/dev/null || echo "not_found")
    if [[ "$pg" == "healthy" && "$redis" == "healthy" ]]; then
        echo "Services already healthy"
        exit 0
    fi
    echo "Starting services..."
    docker compose up -d || true
    echo -n "Waiting for services"
    for i in $(seq 1 40); do
        pg=$(docker inspect --format '{{"{{"}}.State.Health.Status{{"}}"}}' buzz-postgres 2>/dev/null || echo "not_found")
        redis=$(docker inspect --format '{{"{{"}}.State.Health.Status{{"}}"}}' buzz-redis 2>/dev/null || echo "not_found")
        if [[ "$pg" == "healthy" && "$redis" == "healthy" ]]; then
            echo " ready"
            exit 0
        fi
        echo -n "."
        sleep 3
    done
    echo " timed out"
    exit 1

# Apply database migrations and seed the local dev community if the dev database is running
_ensure-migrations: _ensure-services
    cargo run -p buzz-admin -- migrate
    ./scripts/seed-local-community.sh

# Run clippy on the desktop Tauri Rust crate
desktop-tauri-clippy: _ensure-sidecar-stubs
    cargo clippy --manifest-path {{desktop_tauri_manifest}} --workspace --all-targets -- -D warnings

# Check the desktop Tauri Rust crate compiles
desktop-tauri-check: _ensure-sidecar-stubs
    # `--workspace`, like the clippy and test gates beside it. Without it this
    # checks only the root package plus whatever it depends on — so a member
    # crate nothing depends on, such as the standalone menu bar app, would be
    # skipped and its gate would pass green-and-empty over a real defect.
    cargo check --manifest-path {{desktop_tauri_manifest}} --workspace

# Run desktop Tauri Rust unit tests
desktop-tauri-test: _ensure-sidecar-stubs
    cd desktop/src-tauri && cargo test --workspace

# Run the native terminal latency gate explicitly on a known-idle host.
# This is intentionally excluded from shared CI: scheduler contention makes a
# wall-clock assertion flaky, and the release profile is the shipped shape.
desktop-terminal-performance-test:
    cargo test --manifest-path desktop/src-tauri/crates/buzz-terminal/Cargo.toml --release --test latency g3_renderer_acquire_stays_within_frame_budget -- --ignored --exact --nocapture

# Verify compiled-flag behavior under both compile states (clean + capability set).
# Runs the auto-connect and owner-only access focused tests twice with
# independently supplied expected values; build.rs rerun-if-env-changed
# triggers recompilation.
desktop-tauri-test-compiled-flags: _ensure-sidecar-stubs
    #!/usr/bin/env bash
    set -euo pipefail
    cd desktop/src-tauri
    echo "=== Clean build (no flag) → expect false ==="
    env -u BUZZ_BUILD_AUTO_CONNECT_DEFAULT_RELAY \
      BUZZ_TEST_EXPECTED_AUTO_CONNECT_DEFAULT_RELAY=false \
      cargo test compiled_flag_matches_expected -- --ignored --nocapture
    env -u BUZZ_BUILD_AGENT_ACCESS_OWNER_ONLY \
      BUZZ_TEST_EXPECTED_AGENT_ACCESS_OWNER_ONLY=false \
      cargo test --lib
    env -u BUZZ_BUILD_AGENT_ACCESS_OWNER_ONLY \
      BUZZ_TEST_EXPECTED_AGENT_ACCESS_OWNER_ONLY=false \
      cargo test compiled_policy_matches_expected -- --ignored --nocapture
    echo "=== Internal build (flags set) → expect true ==="
    BUZZ_BUILD_AUTO_CONNECT_DEFAULT_RELAY=1 \
      BUZZ_TEST_EXPECTED_AUTO_CONNECT_DEFAULT_RELAY=true \
      cargo test compiled_flag_matches_expected -- --ignored --nocapture
    BUZZ_BUILD_AGENT_ACCESS_OWNER_ONLY=1 \
      BUZZ_TEST_EXPECTED_AGENT_ACCESS_OWNER_ONLY=true \
      cargo test --lib
    BUZZ_BUILD_AGENT_ACCESS_OWNER_ONLY=1 \
      BUZZ_TEST_EXPECTED_AGENT_ACCESS_OWNER_ONLY=true \
      cargo test compiled_policy_matches_expected -- --ignored --nocapture
    echo "Both compiled states verified."

# Build the full desktop Tauri app locally (unsigned, for testing)
# Sidecar binary list must stay in sync with _ensure-sidecar-stubs above.
# pnpm install is unconditional here: release builds must start from a clean dep tree.
desktop-release-build target="aarch64-apple-darwin":
    #!/usr/bin/env bash
    set -euo pipefail
    TARGET={{target}}
    mkdir -p desktop/src-tauri/binaries
    touch "desktop/src-tauri/binaries/buzz-acp-$TARGET"
    touch "desktop/src-tauri/binaries/buzz-agent-$TARGET"
    if [[ "$TARGET" != *windows* ]]; then
        touch "desktop/src-tauri/binaries/buzz-backend-kubernetes-$TARGET"
    fi
    touch "desktop/src-tauri/binaries/buzz-dev-mcp-$TARGET"
    touch "desktop/src-tauri/binaries/git-credential-nostr-$TARGET"
    touch "desktop/src-tauri/binaries/bee-$TARGET"
    if [[ "$TARGET" != *windows* ]]; then
        touch "desktop/src-tauri/binaries/buzz-shell-host-$TARGET"
        touch "desktop/src-tauri/binaries/beekeeper-host-$TARGET"
    fi
    pnpm install
    cd {{desktop_dir}} && pnpm tauri build --features mesh-llm --target {{target}}

# Build the local production Beekeeper.app from any commit-ish (default: newest
# build/* tag, else the tracked upstream, else HEAD) and install it to
# /Applications. See docs/local-desktop-instances.md.
prod-desktop rev="":
    ./scripts/local-prod-build.sh {{rev}}

# Run desktop checks suitable for CI / pre-push
desktop-ci: desktop-check desktop-test desktop-tauri-fmt-check desktop-build desktop-tauri-check desktop-tauri-test

# Seed deterministic channel data for desktop Playwright tests
desktop-e2e-seed: _ensure-migrations
    ./scripts/setup-desktop-test-data.sh

# Run desktop browser smoke tests
desktop-e2e-smoke:
    cd {{desktop_dir}} && pnpm test:e2e:smoke

# Build the e2e bundle and run the WHOLE desktop Playwright smoke suite.
#
# Deliberately NOT a dependency of `just ci`: the full browser run is a
# pre-merge / pre-release gate you run on purpose, not a per-commit one.
# TESTING.md records dated size/duration observations rather than a fixed ETA.
# Because nothing ran it automatically, 75 of its tests rotted unnoticed;
# run it before landing anything that touches desktop UI.
smoke: desktop-e2e-smoke

# Run desktop relay-backed e2e tests
desktop-e2e-integration: _ensure-migrations
    cd {{desktop_dir}} && pnpm test:e2e:integration

# Run the deterministic desktop correctness smoke against an isolated local relay
desktop-release-smoke:
    ./scripts/run-desktop-release-smoke.sh

# Run only the e2e specs changed vs origin/main (both projects) before pushing
desktop-e2e-pre-push: _ensure-migrations
    git fetch origin main
    cd {{desktop_dir}} && pnpm build:e2e && pnpm exec playwright test --only-changed=origin/main

# Run all checks suitable for CI / pre-push (no infra needed)
ci: check test-unit desktop-test desktop-build desktop-tauri-check desktop-tauri-test web-test web-build mobile-test

# ─── Test ─────────────────────────────────────────────────────────────────────

# Run all tests (unit + integration)
test: test-genesis test-git-push-gate test-ci-completion
    ./scripts/run-tests.sh all

# The push gate's Postgres-backed acceptance cases: the binding gate, the
# project/channel roster grants, the seat inheritance cap, and the
# `require-verdict` admission search.
#
# Same shape and the same reason as `test-genesis` below. These drive the real
# `hook_policy_check` through a live database, so they are
# `#[ignore = "requires Postgres"]`; `scripts/run-tests.sh` runs the workspace
# *without* `--ignored` and only ever names `-p buzz-db` for the DB-backed
# steps, so before this recipe existed nothing in the repo executed them. They
# decide who may push to a protected ref — they may not sit unexecuted.
#
# The filter is two module substrings passed as separate libtest filter
# arguments (libtest ORs them; this is not cargo's single-TESTNAME positional,
# so both go after `--`):
#   api::git::policy::tests::gate          — the push-gate cases
#   api::git::verdict_admission::tests     — the verdict-admission cases
#   …::observed_tests, …::verified_tests   — arms (B) and (C), each in its own
#                                            module and so its own substring
# Extend this list, not the database name, when the next Postgres-gated push
# proof joins them.
#
# A throwaway database for the same reason `test-genesis` needs one: these
# tests share the schema of the invoking worktree, and pointing them at the dev
# database would let a feature branch's migrations downgrade it.
#
# A recipe of its own rather than a second `cargo test` line inside
# `test-genesis` (which is how another lane proposed it, against the same
# database and the same `trap cleanup EXIT`). Either works; this way `just
# test-git-push-gate` names one thing and can be run alone, and the recipe's
# own filter is the whole set — the push-gate cases *and* the
# verdict-admission ones, 58 tests as of lane L28 (L21 added arm (A)'s two,
# L22 arm (B)'s ten in a sibling module, L27 arm (C)-plus-rows' six in another,
# L26 the founder-signed rule record's nine — eight push-gate cases under
# `policy::tests::gate::protection`, deliberately a *child* of `gate` so the
# existing filter reaches them, plus the write gate's one case in
# `handlers::repo_protection::tests`, which needed its own filter arm — and L28
# the mission-lookup cases in two more siblings, `verdict_admission::lookup_tests`
# and `api::git::verdict_admission_scope::tests`).
# Project pack-source founder endorsement also runs here: its current-head
# admission proof needs Postgres and must not remain an unexecuted ignored test.
# A filter of `api::git::policy` alone reaches only 10 of them, and one naming
# only `verdict_admission::tests` misses every sibling module entirely — which
# is exactly how arm (B)'s ten sat unexecuted for one run. **Every new sibling
# module needs its own substring here**; that is the whole failure mode.
test-git-push-gate: _ensure-services
    #!/usr/bin/env bash
    set -euo pipefail
    db="buzz_push_gate_$$_$(date +%s)"
    pg() { docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q "$@"; }
    cleanup() { pg -d postgres -c "DROP DATABASE IF EXISTS ${db};" >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    cleanup
    pg -d postgres -c "CREATE DATABASE ${db};" >/dev/null
    scratch="postgres://buzz:buzz_dev@localhost:5432/${db}" # sadscan:disable np.postgres.1
    DATABASE_URL="${scratch}" cargo run -q -p buzz-admin -- migrate
    echo "==> git push-gate acceptance cases against ${db} (serial, isolated)"
    DATABASE_URL="${scratch}" BUZZ_TEST_DATABASE_URL="${scratch}" \
        cargo test -p buzz-relay --lib -- \
        api::git::policy::tests::gate api::git::verdict_admission::tests \
        api::git::verdict_admission::observed_tests \
        api::git::verdict_admission::verified_tests \
        handlers::repo_protection::tests \
        handlers::pack_source::tests \
        api::git::verdict_admission::lookup_tests \
        api::git::verdict_admission::hardening_tests \
        api::git::verdict_admission_scope::tests \
        --ignored --test-threads=1

# Genesis uniqueness proofs (kind 44226) and authority-chain proofs (kind
# 44228) against a throwaway database.
#
# These prove security properties — that two rival claims on one session
# reference cannot both be accepted (genesis), and that two rival authority
# transitions for one chain cannot both be accepted (authority transitions) —
# so they may not sit unexecuted. They are `#[ignore]`d because they need
# Postgres, and `run-tests.sh` deliberately runs `cargo test -p buzz-db`
# *without* `--ignored`, so nothing else in this repo ever runs them.
#
# The filter is two substrings — `genesis` and `authority_transition` — passed
# as separate libtest filter arguments, which libtest ORs together (this is
# NOT cargo's own single-TESTNAME positional; both go after `--`). Extend this
# list, not the DB name, the next time a Postgres-gated proof needs to join it.
#
# They cannot simply be pointed at the dev database. Each Postgres-backed
# buzz-db test drops and rebuilds the schema from the *invoking worktree's*
# migrations, so running them from a feature branch silently downgrades a shared
# dev database and deletes any schema newer than that branch. They also deadlock
# against each other under the default parallel harness. Hence: a database
# created for this run, serial execution, and a drop on the way out.
test-genesis: _ensure-services
    #!/usr/bin/env bash
    set -euo pipefail
    # Unique per run: a fixed name collides when two worktrees run the gate
    # at once (ruling R18), and both would drop each other's database.
    db="buzz_genesis_gate_$$_$(date +%s)"
    pg() { docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q "$@"; }
    cleanup() { pg -d postgres -c "DROP DATABASE IF EXISTS ${db};" >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    cleanup
    pg -d postgres -c "CREATE DATABASE ${db};" >/dev/null
    scratch="postgres://buzz:buzz_dev@localhost:5432/${db}" # sadscan:disable np.postgres.1
    # The event tests expect a migrated schema; they do not build one themselves.
    DATABASE_URL="${scratch}" cargo run -q -p buzz-admin -- migrate
    echo "==> genesis + authority-chain proofs against ${db} (serial, isolated)"
    DATABASE_URL="${scratch}" BUZZ_TEST_DATABASE_URL="${scratch}" \
        cargo test -p buzz-db --lib -- genesis authority_transition --ignored --test-threads=1

# CI callback composition and atomic result proofs require Postgres. Keep them
# in the integration entrypoint so their #[ignore] annotations cannot hide them.
# Each run owns its database; no developer or live app database is reset.
test-ci-completion: _ensure-services
    #!/usr/bin/env bash
    set -euo pipefail
    db="buzz_ci_completion_$$_$(date +%s)"
    pg() { docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -q "$@"; }
    cleanup() { pg -d postgres -c "DROP DATABASE IF EXISTS ${db};" >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    pg -d postgres -c "CREATE DATABASE ${db};" >/dev/null
    scratch="postgres://buzz:buzz_dev@localhost:5432/${db}" # sadscan:disable np.postgres.1
    DATABASE_URL="${scratch}" cargo run -q -p buzz-admin -- migrate
    DATABASE_URL="${scratch}" BUZZ_TEST_DATABASE_URL="${scratch}" \
        cargo test -p buzz-db --lib ci_result -- --ignored --test-threads=1
    cargo build -p buzz-cli --bin bee
    target_dir="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    DATABASE_URL="${scratch}" BUZZ_TEST_DATABASE_URL="${scratch}" BUZZ_TEST_BEE_BIN="${target_dir}/debug/bee" \
        cargo test -p buzz-relay --lib ci_result -- --ignored --test-threads=1

# CI-managed turn continuation (docs/CI_MANAGED_CONTINUATION_IMPL.md): composes
# the real buzz-relay binary (its own scratch database, dropped on exit; Redis
# logical DB 14, never DB 0 or the dev database), the built `bee` CLI, and the
# real buzz-session-provider binary (a fake ACP adapter script standing in for
# the model — no network model call) to prove registration, real-webhook-
# produced CI results, at-most-once turn admission, and the materialized-
# context contract end to end. See scripts/ci-continuation-acceptance.sh for
# what each step proves and what this does not cover (the private-project
# read path, §3f).
test-ci-continuation: _ensure-services
    cargo build -p buzz-cli --bin bee -p buzz-relay --bin buzz-relay -p buzz-session-provider --bin buzz-session-provider
    cargo test -p buzz-session-provider --test ci_continuation_composition -- --ignored --test-threads=1

# Absent-participant handover (docs/HANDOVER_IMPL.md §6): composes the real
# buzz-relay binary (its own scratch database, dropped on exit; Redis logical
# DB 14, never DB 0 or the dev database), the built `bee` CLI, and **two**
# real buzz-session-provider processes with distinct keys, state dirs and
# working directories (fake ACP adapter scripts standing in for the model — no
# network model call), against a relay-hosted git repository pushed to and
# fetched from over the relay's own smart-HTTP transport with NIP-98
# credentials. Proves checkpoint/claim/continue end to end: the fence on a
# returning provider and on its sibling executions, racing claims, a voided
# claim that a regrant does not restore, replay, a missing artifact, an
# interrupted continuation, retirement, and the native leg.
#
# `git-credential-nostr` is built too: without it the checkpoint's wip-ref
# push has no way to authenticate to the relay. See
# scripts/handover-acceptance.sh for what each step proves and what it does
# not (no real model, one host rather than two machines, no seated restore,
# no Windows).
#
# Absent-participant handover composition: two providers, one relay-hosted repo
test-handover: _ensure-services
    cargo build -p buzz-cli --bin bee -p buzz-relay --bin buzz-relay -p buzz-session-provider --bin buzz-session-provider -p git-credential-nostr --bin git-credential-nostr
    ./scripts/handover-acceptance.sh

# Composes a real `beekeeper-host`, a real `buzz-session-provider` as its
# child, a real control socket and a second host competing for the same state
# directory. Needs no relay, no Postgres and no Redis — the provider is pointed
# at an unreachable URL on purpose, which is what lets this observe supervision
# without a database.
#
# See scripts/host-acceptance.sh for what each step proves and, named at the
# top, what it does not: no transcript items reaching a relay, no desktop app
# quitting, no login-time start, no real model.
#
# Agent-host lifecycle composition: supervision, takeover refusal, the ladder
test-host:
    ./scripts/host-acceptance.sh

# Run unit tests only (no infra needed)
test-unit:
    #!/usr/bin/env bash
    set -euo pipefail
    # The whole workspace, not a hand-kept package list.
    #
    # This used to enumerate nine `-p` targets, and everything outside that
    # list — buzz-relay (1032 tests), buzz-acp (836), buzz-session-provider
    # (419), buzz-sdk (306), git-credential-nostr and a dozen more, ~3,375
    # tests — ran in no local gate at all. Only `.woodpecker/gate.yml` ever
    # executed them, so the only way to find a failure there was to push and
    # watch CI go red. That is exactly how both flakes fixed in the commit that
    # widened this recipe reached `main`. The list was also hand-mirrored into
    # `scripts/run-tests.sh` and free to drift from it.
    #
    # Every test that needs Postgres, Redis or MinIO is `#[ignore]`d with a
    # reason, so this stays infra-free: measured green with DATABASE_URL and
    # REDIS_URL both pointed at a dead port. Keep it that way — if you add a
    # test that needs a service, mark it, do not re-narrow this command.
    #
    # All targets rather than --lib: buzz-conformance's replay fixtures,
    # buzz-cli, buzz-push-gateway and buzz-backend-kubernetes carry infra-free
    # coverage in tests/ that --lib would drop.
    ./scripts/test-ensure-local-relay-key.sh
    if command -v cargo-nextest &>/dev/null; then
        cargo nextest run --workspace
        # buzz-auth NIP-FI verifier doctests. The sealed-authority
        # `compile_fail` doctests prove the default-feature public API alone
        # cannot forge the issuer→JWKS authority; nextest does not run
        # doctests, and the `--workspace` nextest run above therefore does not
        # cover them.
        cargo test -p buzz-auth --doc
    else
        ./scripts/run-tests.sh unit
    fi

# Run integration tests only (starts services if needed)
test-integration:
    ./scripts/run-tests.sh integration

# Regenerate the model-capability normative corpus from the production Rust
# resolver. The corpus is a golden snapshot, never hand-edited: this runs the
# `#[ignore]`d writer test in buzz-agent, which serializes `resolve()` over the
# inputs-only question table to scripts/normative-corpus.json. Run this after
# any model-capabilities.json edit, then commit the regenerated file. The
# `corpus_matches_generated_snapshot` gate fails CI if the committed file drifts.
regen-model-corpus:
    cargo test -p buzz-agent --lib model_capabilities::tests::regen_corpus_file -- --ignored --exact

# Buzz shared compute e2e: current desktop discovery/admission logic and
# Playwright UI coverage.
mesh-e2e:
    cargo test --manifest-path {{desktop_dir}}/src-tauri/Cargo.toml --features mesh-llm mesh_llm --lib
    cd {{desktop_dir}} && pnpm test:e2e:smoke -- mesh-compute.spec.ts

# Reset only development state, seed deterministic local channels, and launch
# the mesh-enabled desktop with the repository's public Tyler test identity.
# This is for local verification only; never point this identity at staging/prod.
[confirm("This will reset development data, preserve installed Buzz, then launch a seeded mesh dev app. Continue? (y/N)")]
mesh-dev-fresh:
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/dev-reset.sh --yes
    ./scripts/setup-desktop-test-data.sh
    export BUZZ_PRIVATE_KEY="3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03"
    export BUZZ_REQUIRE_RELAY_MEMBERSHIP=true
    export BUZZ_ALLOW_NIP_OA_AUTH=true
    export RELAY_OWNER_PUBKEY="e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34"
    export BUZZ_RELAY_PRIVATE_KEY="0000000000000000000000000000000000000000000000000000000000000001"
    export BUZZ_RECONCILE_CHANNELS=true
    export BUZZ_RESET_WEBVIEW_STATE=1
    exec just mesh=1 dev

# Real serve->client->inference on this machine (not CI).
mesh-e2e-hardware:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -p buzz-relay --example mesh_serve_client_smoke

# Three isolated node processes: trusted member joins and infers; stranger is rejected.
# Uses temp homes and explicit mesh owner keystores. Never reads the Buzz Keychain.
mesh-e2e-admission:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -p buzz-relay --example mesh_admission_smoke

# Full hardware confidence suite: routing, owner admission, and real agent inference.
mesh-e2e-confidence:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p buzz-agent -p buzz-dev-mcp
    cargo run -p buzz-relay --example mesh_serve_client_smoke
    cargo run -p buzz-relay --example mesh_admission_smoke
    cargo run -p buzz-relay --example mesh_agent_e2e

# Take desktop screenshots using the mock bridge
desktop-screenshot *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    pnpm -C {{desktop_dir}} build:e2e
    cd {{desktop_dir}}
    if ! curl -sf http://127.0.0.1:4173/ >/dev/null 2>&1; then
        python3 -m http.server 4173 -d dist >/dev/null 2>&1 &
        trap "kill $! 2>/dev/null || true" EXIT
        for i in $(seq 1 20); do curl -sf http://127.0.0.1:4173/ >/dev/null && break; sleep 0.5; done
    fi
    node tests/helpers/screenshot.mjs {{ARGS}}

# ─── Run ──────────────────────────────────────────────────────────────────────

# Start the relay server (auto-starts Docker services if needed)
relay: bootstrap _ensure-migrations
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    set -o allexport
    source .env
    set +o allexport
    cargo run -p buzz-relay

# Start the relay with the built web UI served from it
relay-web: bootstrap _ensure-migrations
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    set -o allexport
    source .env
    set +o allexport
    [[ -d node_modules ]] || pnpm install
    pnpm -C web build
    BUZZ_WEB_DIR=./web/dist cargo run -p buzz-relay

# Build and run the private read-only admin dashboard
admin: bootstrap _ensure-migrations
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    set -o allexport
    source .env
    set +o allexport
    [[ -d node_modules ]] || pnpm install
    pnpm -C admin-web build
    export BUZZ_ADMIN_HOST="${BUZZ_ADMIN_HOST:-admin.localhost:3000}"
    export BUZZ_ADMIN_WEB_DIR="${BUZZ_ADMIN_WEB_DIR:-{{justfile_directory()}}/admin-web/dist}"
    echo "Admin dashboard: http://${BUZZ_ADMIN_HOST}/reports"
    cargo run -p buzz-relay

# Seed deterministic reports and product feedback for local admin dashboard review
admin-seed: _ensure-migrations
    ./scripts/seed-admin-dashboard.sh

# Run focused relay and browser checks for the read-only admin dashboard
admin-check: fmt-check
    cargo check -p buzz-relay --all-targets
    cargo test -p buzz-relay api::admin
    cargo test -p buzz-relay router::tests
    pnpm -C admin-web check
    pnpm -C admin-web exec playwright test

# Start the relay server in release mode
relay-release: bootstrap _ensure-migrations
    #!/usr/bin/env bash
    set -euo pipefail
    set -o allexport
    source .env
    set +o allexport
    cargo run -p buzz-relay --release


# Run the desktop Tauri app in dev mode with a local relay (ports and identity derived from worktree)
dev *ARGS: bootstrap _ensure-sidecar-stubs _ensure-migrations
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    set -o allexport
    source .env
    set +o allexport
    bind_addr="${BUZZ_BIND_ADDR:-0.0.0.0:3000}"
    relay_port="${bind_addr##*:}"; [[ -n "$relay_port" ]] || relay_port=3000
    health_port="${BUZZ_HEALTH_PORT:-8080}"
    metrics_port="${BUZZ_METRICS_PORT:-9102}"
    if command -v lsof >/dev/null 2>&1; then
        for spec in "relay:$relay_port" "health:$health_port" "metrics:$metrics_port"; do
            name="${spec%%:*}"; port="${spec##*:}"
            if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
                echo "Error: $name port $port is already in use; refusing to launch desktop against a stale relay." >&2
                lsof -nP -iTCP:"$port" -sTCP:LISTEN >&2 || true
                echo "Stop the process above (often a stale buzz-relay) and rerun: just dev" >&2
                exit 1
            fi
        done
    fi
    cargo build -p buzz-acp -p buzz-agent -p buzz-backend-kubernetes -p buzz-dev-mcp -p buzz-cli -p git-credential-nostr -p buzz-shell-host -p beekeeper-host -p buzz-relay
    # Docker Desktop's forwarded MinIO port can stall under the deployment
    # probe's 32 concurrent writers. Keep the gate enabled in local dev, using
    # the bounded profile already used by the relay test launcher.
    export BUZZ_GIT_PROBE_WRITERS="${BUZZ_GIT_PROBE_WRITERS:-8}"
    export BUZZ_GIT_PROBE_ROUNDS="${BUZZ_GIT_PROBE_ROUNDS:-2}"
    ./target/debug/buzz-relay &
    RELAY_PID=$!
    cleanup() {
        [[ -n "${INSTANCE_ID:-}" ]] && ../scripts/cleanup-instance-agents.sh "$INSTANCE_ID" || true
        kill "$RELAY_PID" 2>/dev/null || true
    }
    trap cleanup EXIT
    relay_ready=false
    for _ in $(seq 1 120); do
        if ! kill -0 "$RELAY_PID" 2>/dev/null; then
            echo "Error: buzz-relay exited during startup; refusing to launch desktop." >&2
            wait "$RELAY_PID" || true
            exit 1
        fi
        if curl --silent --fail --max-time 1 "http://127.0.0.1:${health_port}/_readiness" >/dev/null; then
            relay_ready=true
            break
        fi
        sleep 0.5
    done
    if [[ "$relay_ready" != true ]]; then
        echo "Error: buzz-relay did not become healthy within 60 seconds; refusing to launch desktop." >&2
        exit 1
    fi
    cd {{desktop_dir}}
    [[ -d node_modules ]] || pnpm install
    source ../scripts/instance-env.sh
    INSTANCE_ID=$(node -e "console.log(JSON.parse(process.env.BUZZ_TAURI_CONFIG).identifier)")
    echo "Starting on Vite port ${BUZZ_VITE_PORT}, relay ${BUZZ_RELAY_URL}"
    FEATURES=(); [[ -n "{{mesh}}" ]] && FEATURES=(--features mesh-llm)
    pnpm exec tauri dev ${FEATURES[@]+"${FEATURES[@]}"} --config "$BUZZ_TAURI_CONFIG" {{ARGS}}

# Run only the desktop app. No relay, database, Docker, migrations, or .env are needed.
# The app opens normally and asks for a community before making a relay connection.
desktop-standalone *ARGS: _ensure-sidecar-stubs
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    cargo build -p buzz-acp -p buzz-agent -p buzz-backend-kubernetes -p buzz-dev-mcp -p buzz-cli -p git-credential-nostr -p buzz-shell-host -p beekeeper-host -p buzz-session-provider
    TARGET=$(rustc -vV | sed -n 's|host: ||p')
    TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | node -p "JSON.parse(require('fs').readFileSync(0, 'utf8')).target_directory")
    for bin in buzz-acp buzz-agent buzz-backend-kubernetes buzz-dev-mcp git-credential-nostr bee buzz-shell-host beekeeper-host; do
        cp "${TARGET_DIR}/debug/${bin}" "desktop/src-tauri/binaries/${bin}-${TARGET}"
        chmod +x "desktop/src-tauri/binaries/${bin}-${TARGET}"
    done
    cd {{desktop_dir}}
    [[ -d node_modules ]] || pnpm install
    unset BUZZ_PRIVATE_KEY BUZZ_SHARE_IDENTITY
    if [[ -n "{{fresh}}" ]]; then
        export BUZZ_RESET_WEBVIEW_STATE=1
    fi
    source ../scripts/instance-env.sh
    INSTANCE_ID=$(node -e "console.log(JSON.parse(process.env.BUZZ_TAURI_CONFIG).identifier)")
    # Worktrees get a scoped keyring service so concurrent instances do not
    # share an identity. The main checkout deliberately leaves this UNSET so
    # the Rust default applies: `beekeeper-desktop-dev` is where the existing
    # dev identity lives, and it is the only service the one-time agent-key
    # migration runs for (managed_agents/storage.rs `migrate_agent_keys_to_dev_service`
    # early-returns unless `keyring_service()` is exactly that). Naming the
    # main checkout `.main` would silently strand both.
    if [[ -n "${BUZZ_INSTANCE_SLUG:-}" ]]; then
        export BUZZ_DEV_KEYRING_SERVICE="beekeeper-desktop-dev.${BUZZ_INSTANCE_SLUG}"
    fi
    if [[ -n "{{fresh}}" ]]; then
        ../scripts/reset-desktop-standalone-state.sh "$INSTANCE_ID" "${BUZZ_DEV_KEYRING_SERVICE:-beekeeper-desktop-dev}"
    fi
    trap '../scripts/cleanup-instance-agents.sh "$INSTANCE_ID" || true' EXIT
    echo "Starting standalone desktop on Vite port ${BUZZ_VITE_PORT}; no relay services were started"
    TAURI_FLAGS=()
    if [[ -n "{{nokeyring}}" ]]; then
        echo "system-keyring OFF: secrets live in 0600 files under the app-data dir"
        # tauri-cli re-adds crate default features explicitly, so the feature
        # is stripped by a cargo runner wrapper rather than runner args.
        TAURI_FLAGS=(-r "{{justfile_directory()}}/scripts/cargo-strip-keyring.sh")
    fi
    pnpm exec tauri dev ${TAURI_FLAGS[@]+"${TAURI_FLAGS[@]}"} --config "$BUZZ_TAURI_CONFIG" {{ARGS}}

# Run the desktop app against the internal staging relay (installs deps + builds agent tools automatically)
staging *ARGS: bootstrap _ensure-sidecar-stubs
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    pnpm install  # unconditional: staging must always start with a clean dep tree
    cargo build --release -p buzz-acp -p buzz-agent -p buzz-backend-kubernetes -p buzz-dev-mcp -p buzz-cli -p git-credential-nostr -p buzz-shell-host -p beekeeper-host
    FEATURES=()
    if [[ -n "{{mesh}}" ]]; then
        FEATURES=(--features mesh-llm)
    fi
    # Replace 0-byte sidecar stubs with real binaries so tauri dev picks them up.
    # buzz: the CLI sidecar. buzz-backend-kubernetes: provider discovery scans the
    # exe dir for executable buzz-backend-* files, so the non-executable stub that
    # tauri dev copies next to the exe would hide the provider from "Run on".
    TARGET=$(rustc -vV | sed -n 's|host: ||p')
    TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | node -p "JSON.parse(require('fs').readFileSync(0, 'utf8')).target_directory")
    STAGING_SIDECARS=(bee)
    if [[ "$TARGET" != *windows* ]]; then
        STAGING_SIDECARS+=(buzz-backend-kubernetes)
    fi
    for bin in "${STAGING_SIDECARS[@]}"; do
        cp "${TARGET_DIR}/release/${bin}" "desktop/src-tauri/binaries/${bin}-${TARGET}"
        chmod +x "desktop/src-tauri/binaries/${bin}-${TARGET}"
    done
    cd {{desktop_dir}}
    export BUZZ_RELAY_URL="wss://sprout-oss.stage.blox.sqprod.co"
    source ../scripts/instance-env.sh
    # Ctrl+C kills the Tauri app before its in-process sweep finishes, leaking
    # agent workers. Reap this instance's agents on exit as a backstop.
    INSTANCE_ID=$(node -e "console.log(JSON.parse(process.env.BUZZ_TAURI_CONFIG).identifier)")
    trap '../scripts/cleanup-instance-agents.sh "$INSTANCE_ID" || true' EXIT
    echo "Starting staging on Vite port ${BUZZ_VITE_PORT}, relay ${BUZZ_RELAY_URL}"
    pnpm exec tauri dev ${FEATURES[@]+"${FEATURES[@]}"} --config "$BUZZ_TAURI_CONFIG" {{ARGS}}

# Run the desktop app against the production relay (installs deps + builds agent tools automatically)
production *ARGS: bootstrap _ensure-sidecar-stubs
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    pnpm install  # unconditional: production must always start with a clean dep tree
    cargo build --release -p buzz-acp -p buzz-agent -p buzz-backend-kubernetes -p buzz-dev-mcp -p buzz-cli -p git-credential-nostr -p buzz-shell-host -p beekeeper-host
    FEATURES=()
    if [[ -n "{{mesh}}" ]]; then
        FEATURES=(--features mesh-llm)
    fi
    # Replace 0-byte sidecar stubs with real binaries so tauri dev picks them up.
    # buzz: the CLI sidecar. buzz-backend-kubernetes: provider discovery scans the
    # exe dir for executable buzz-backend-* files, so the non-executable stub that
    # tauri dev copies next to the exe would hide the provider from "Run on".
    TARGET=$(rustc -vV | sed -n 's|host: ||p')
    TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | node -p "JSON.parse(require('fs').readFileSync(0, 'utf8')).target_directory")
    PRODUCTION_SIDECARS=(bee)
    if [[ "$TARGET" != *windows* ]]; then
        PRODUCTION_SIDECARS+=(buzz-backend-kubernetes)
    fi
    for bin in "${PRODUCTION_SIDECARS[@]}"; do
        cp "${TARGET_DIR}/release/${bin}" "desktop/src-tauri/binaries/${bin}-${TARGET}"
        chmod +x "desktop/src-tauri/binaries/${bin}-${TARGET}"
    done
    cd {{desktop_dir}}
    export BUZZ_RELAY_URL="wss://buzz.block.builderlab.xyz"
    source ../scripts/instance-env.sh
    # Ctrl+C kills the Tauri app before its in-process sweep finishes, leaking
    # agent workers. Reap this instance's agents on exit as a backstop.
    INSTANCE_ID=$(node -e "console.log(JSON.parse(process.env.BUZZ_TAURI_CONFIG).identifier)")
    trap '../scripts/cleanup-instance-agents.sh "$INSTANCE_ID" || true' EXIT
    echo "Starting production on Vite port ${BUZZ_VITE_PORT}, relay ${BUZZ_RELAY_URL}"
    pnpm exec tauri dev ${FEATURES[@]+"${FEATURES[@]}"} --config "$BUZZ_TAURI_CONFIG" {{ARGS}}

# Run the desktop frontend dev server (port derived from worktree)
desktop-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{desktop_dir}}
    [[ -d node_modules ]] || pnpm install
    source ../scripts/instance-env.sh
    echo "Starting frontend dev server on Vite port ${BUZZ_VITE_PORT}, relay ${BUZZ_RELAY_URL}"
    pnpm exec vite --port "${BUZZ_VITE_PORT}" --strictPort

# ─── Web ─────────────────────────────────────────────────────────────────────

# Run the web frontend dev server (port derived from worktree to avoid collisions)
web:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ -d node_modules ]] || pnpm install
    source scripts/instance-env.sh
    export VITE_PORT=$((BUZZ_VITE_PORT + 100))
    export VITE_RELAY_URL="${BUZZ_RELAY_URL}"
    echo "Starting web dev server on port ${VITE_PORT}, relay ${BUZZ_RELAY_URL}"
    cd {{web_dir}}
    pnpm exec vite --port "${VITE_PORT}" --strictPort

# Run web lint and format checks
web-check:
    cd {{web_dir}} && pnpm check

# Fix web lint and format issues
web-fix:
    cd {{web_dir}} && pnpm exec biome check --write .

# Run web TypeScript checks
web-typecheck:
    cd {{web_dir}} && pnpm typecheck

# Run the web unit tests
web-test:
    cd {{web_dir}} && pnpm test

# Build web frontend assets
web-build:
    cd {{web_dir}} && pnpm build

# Run web browser smoke tests
web-e2e-smoke:
    cd {{web_dir}} && pnpm test:e2e:smoke

# ─── Mobile ──────────────────────────────────────────────────────────────────

mobile_dir := "mobile"

# Install mobile Flutter dependencies
mobile-install:
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && flutter pub get

# Format all Dart code
mobile-fmt:
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && dart format .

# Fix mobile formatting and run analysis
mobile-fix:
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && dart format . && flutter analyze

# Run mobile lint and format checks
mobile-check:
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && dart format --output=none --set-exit-if-changed . && flutter analyze

# Run mobile tests
#
# `--reporter expanded`: Flutter's default reporter becomes "compact" with
# carriage-return redraws whenever a pseudo-terminal is attached, so a
# failing test's name scrolls away in agent/CI capture (item 208). Expanded
# is line-oriented and deterministic regardless of tty.
mobile-test:
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && flutter test --reporter expanded

# Regenerate the emoji dataset asset from desktop's emoji-mart install.
# Output is committed — rerun after bumping @emoji-mart/data.
mobile-emoji-data:
    node {{mobile_dir}}/scripts/generate-emoji-data.mjs

# Compile an unsigned Android debug APK (worktree-aware debug identity)
mobile-build-android:
    ./scripts/mobile-worktree-overrides.sh
    unset GIT_DIR GIT_WORK_TREE; cd {{mobile_dir}} && flutter build apk --debug --no-pub

# Run the mobile app on iOS simulator (worktree-aware debug identity)
mobile-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    # Xcode 27 replaced Simulator.app with Device Hub; either host will do.
    if ! pgrep -x Simulator &>/dev/null && ! pgrep -x DeviceHub &>/dev/null; then
        open -a Simulator 2>/dev/null || open -a DeviceHub
        sleep 3
    fi
    ./scripts/mobile-worktree-overrides.sh
    cd {{mobile_dir}}
    unset GIT_DIR GIT_WORK_TREE
    flutter run

# Uninstall stale worktree-suffixed Buzz debug installs (production apps kept)
mobile-clean:
    ./scripts/mobile-worktree-clean.sh

# ─── Database ─────────────────────────────────────────────────────────────────

# Apply database migrations
migrate: _ensure-migrations

# ─── Utilities ────────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean
    cargo clean --manifest-path desktop/src-tauri/Cargo.toml

# Check the Rust workspace compiles without producing binaries
check-compile:
    cargo check --workspace --all-targets

# ─── Release ─────────────────────────────────────────────────────────────────

# Read the current desktop version from package.json
get-current-version:
    @node -p "require('./desktop/package.json').version"

# Read the current relay version from its crate manifest
get-current-relay-version:
    @grep -m1 '^version = ' crates/buzz-relay/Cargo.toml | sed -E 's/version = "(.*)"/\1/'

# Compute next minor version (e.g., 0.3.0 → 0.4.0)
get-next-minor-version:
    @python3 -c "v='$(just get-current-version)'.split('.'); print(f'{v[0]}.{int(v[1])+1}.0')"

# Compute next patch version (e.g., 0.3.0 → 0.3.1)
get-next-patch-version:
    @python3 -c "v='$(just get-current-version)'.split('.'); print(f'{v[0]}.{v[1]}.{int(v[2])+1}')"

# Compute next relay patch version (e.g., 0.3.0 → 0.3.1)
get-next-relay-patch-version:
    @python3 -c "v='$(just get-current-relay-version)'.split('.'); print(f'{v[0]}.{v[1]}.{int(v[2])+1}')"

# Update version in desktop package manifests and regenerate lockfiles
bump-desktop-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    # desktop/package.json
    cd desktop && npm pkg set "version={{ version }}" && cd ..
    # desktop/src-tauri/tauri.conf.json
    node -e "
        const fs = require('fs');
        const p = 'desktop/src-tauri/tauri.conf.json';
        const c = JSON.parse(fs.readFileSync(p, 'utf8'));
        c.version = '{{ version }}';
        fs.writeFileSync(p, JSON.stringify(c, null, 2) + '\n');
    "
    # JSON.stringify expands arrays/objects in a way biome rejects; reformat to match.
    (cd desktop && pnpm exec biome format --write src-tauri/tauri.conf.json)
    # desktop/src-tauri/Cargo.toml — only first version line (under [package])
    node -e "
        const fs = require('fs');
        const p = 'desktop/src-tauri/Cargo.toml';
        let t = fs.readFileSync(p, 'utf8');
        t = t.replace(/^version = \".*\"/m, 'version = \"{{ version }}\"');
        fs.writeFileSync(p, t);
    "
    # Regenerate lockfiles
    pnpm install --lockfile-only
    cargo update -p beekeeper-desktop --manifest-path desktop/src-tauri/Cargo.toml
    echo "Bumped desktop manifests to {{ version }} and regenerated lockfiles"

# Bump the relay crate version and regenerate the lockfile
bump-relay-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    # buzz-relay carries its own `version =` (not version.workspace), so the
    # replace targets the package version line only.
    perl -i -pe 's/^version = ".*"/version = "{{ version }}"/' crates/buzz-relay/Cargo.toml
    cargo update -p buzz-relay
    echo "Bumped buzz-relay to {{ version }} and regenerated Cargo.lock"

# Open or update the desktop release PR from an immutable origin/main snapshot
release-desktop *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    ARG="{{ ARGS }}"
    if [[ -z "$ARG" || "$ARG" == "patch" ]]; then
        VERSION=$(just get-next-patch-version)
    else
        VERSION="$ARG"
    fi
    scripts/prepare-desktop-release.sh "$VERSION"

# Open or update the relay release PR (ghcr.io/block/buzz image)
release-relay *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    ARG="{{ ARGS }}"
    if [[ -z "$ARG" || "$ARG" == "patch" ]]; then
        VERSION=$(just get-next-relay-patch-version)
    else
        VERSION="$ARG"
    fi
    just _release-pr relay "$VERSION"

# Shared release-PR engine for desktop and relay. Mobile publishes immutable
# candidate tags directly from remote main instead of using metadata-only PRs.
_release-pr lane version:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION="{{ version }}"
    if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
        echo "Error: '$VERSION' is not valid semver (expected X.Y.Z)"
        exit 1
    fi
    # Lane-specific identifiers. The bump command runs after the branch switch.
    case "{{ lane }}" in
        desktop)
            BRANCH_PREFIX="version-bump"
            TAG_FETCH='v*'
            TAG_MATCH='v[0-9]*'
            TAG_EXCLUDE='*-*'
            TAG_PREFIX="v"
            CHANGELOG="CHANGELOG.md"
            ADD_FILES=(desktop/package.json desktop/src-tauri/tauri.conf.json desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock pnpm-lock.yaml CHANGELOG.md)
            # Every crate whose changes ship inside the desktop bundle. Five
            # bundled crates were already missing from this list; the two new
            # ones are added rather than joining them, because a host change
            # that never appears in the changelog is a change nobody reviewing
            # the release can see.
            LOG_PATHS=(desktop/ crates/buzz-core/ crates/buzz-persona/ crates/buzz-sdk/ crates/buzz-agent/ crates/beekeeper-host/ crates/beekeeper-host-core/)
            ARTIFACT="Beekeeper Desktop" ;;
        relay)
            BRANCH_PREFIX="relay-release"
            TAG_FETCH='relay-v*'
            TAG_MATCH='relay-v[0-9]*'
            TAG_EXCLUDE='relay-v*-*'
            TAG_PREFIX="relay-v"
            CHANGELOG="crates/buzz-relay/CHANGELOG.md"
            ADD_FILES=(crates/buzz-relay/Cargo.toml Cargo.lock crates/buzz-relay/CHANGELOG.md)
            LOG_PATHS=(crates/buzz-relay/ crates/buzz-core/ crates/buzz-db/ crates/buzz-auth/ crates/buzz-pubsub/ crates/buzz-search/ crates/buzz-audit/ crates/buzz-media/ crates/buzz-sdk/ crates/buzz-workflow/ crates/buzz-conformance/ migrations/)
            ARTIFACT="Buzz Relay" ;;
        *)
            echo "Error: unknown release lane '{{ lane }}'"
            exit 1 ;;
    esac
    echo "Preparing ${ARTIFACT} release v${VERSION}..."
    # Must run on main with a clean, up-to-date tree.
    CURRENT_BRANCH=$(git symbolic-ref --short HEAD)
    if [[ "$CURRENT_BRANCH" != "main" ]]; then
        echo "Error: must be on main branch (currently on '$CURRENT_BRANCH')"
        exit 1
    fi
    git fetch origin refs/heads/main:refs/remotes/origin/main --no-tags
    # Release tags are remote-owned state; sync only this lane's tags so stale
    # local tags from older histories do not make release preflight fail.
    git fetch origin "+refs/tags/${TAG_FETCH}:refs/tags/${TAG_FETCH}"
    if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
        echo "Error: local main is not up-to-date with origin/main. Run 'git pull' first."
        exit 1
    fi
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "Error: working tree is dirty. Commit or stash changes first."
        exit 1
    fi
    # Switch to the release branch (create, or reset to main if it exists).
    BRANCH="${BRANCH_PREFIX}/${VERSION}"
    if git rev-parse --verify "refs/heads/$BRANCH" >/dev/null 2>&1; then
        echo "Branch '$BRANCH' already exists — resetting to origin/main..."
        git switch "$BRANCH"
        git reset --hard origin/main
    elif git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
        echo "Branch '$BRANCH' exists on remote — checking out and resetting to origin/main..."
        git switch -c "$BRANCH" --track "origin/$BRANCH"
        git reset --hard origin/main
    else
        git switch -c "$BRANCH"
    fi
    # Lane-specific bump (the one diverging step).
    case "{{ lane }}" in
        desktop) just bump-desktop-version "$VERSION" ;;
        relay)   just bump-relay-version "$VERSION" ;;
    esac
    # Generate the changelog from commits since this lane's last release tag.
    LAST_TAG=$(git describe --tags --abbrev=0 --match "$TAG_MATCH" --exclude "$TAG_EXCLUDE" 2>/dev/null || echo "")
    REPO=$(git remote get-url origin | sed -E 's|.*github\.com[:/]||; s|\.git$||')
    format_log() {
        local range="$1"
        git log "$range" --format="%h %H %s" --no-merges -- "${LOG_PATHS[@]}" | while IFS=' ' read -r short full rest; do
            local pr subject
            pr=$(printf '%s' "$rest" | grep -oE '\(#[0-9]+\)$' | grep -oE '[0-9]+' || true)
            if [[ -n "$pr" ]]; then
                subject=$(printf '%s' "$rest" | sed -E 's/ \(#[0-9]+\)$//')
                printf -- '- %s ([#%s](https://github.com/%s/pull/%s)) ([`%s`](https://github.com/%s/commit/%s))\n' \
                    "$subject" "$pr" "$REPO" "$pr" "$short" "$REPO" "$full"
            else
                printf -- '- %s ([`%s`](https://github.com/%s/commit/%s))\n' \
                    "$rest" "$short" "$REPO" "$full"
            fi
        done
    }
    TMPFILE=$(mktemp)
    {
        echo "# Changelog"
        echo ""
        echo "## ${TAG_PREFIX}${VERSION}"
        echo ""
        if [[ -n "$LAST_TAG" ]]; then
            format_log "${LAST_TAG}..HEAD"
        else
            echo "- Initial release"
        fi
        echo ""
        if [[ -f "$CHANGELOG" ]]; then
            tail -n +2 "$CHANGELOG"
        fi
    } > "$TMPFILE"
    mkdir -p "$(dirname "$CHANGELOG")"
    mv "$TMPFILE" "$CHANGELOG"
    # Commit.
    git add "${ADD_FILES[@]}"
    RELEASE_MSG="chore(release): release ${ARTIFACT} version ${VERSION}"
    if [[ "$(git log -1 --format='%s' 2>/dev/null)" == "$RELEASE_MSG" ]]; then
        git commit --amend --no-edit
    else
        git commit -m "$RELEASE_MSG"
    fi
    # Push and open/update the PR.
    git push --force-with-lease -u origin "$BRANCH"
    PR_BODY="## ${ARTIFACT} release v${VERSION}"$'\n\n'
    if [[ -n "$LAST_TAG" ]]; then
        PR_BODY+="### Changes since ${LAST_TAG}:"$'\n\n'
        CHANGELOG_BODY=$(format_log "${LAST_TAG}..HEAD~1")
        MAX_LOG=62000
        if (( ${#CHANGELOG_BODY} > MAX_LOG )); then
            TRUNCATED=$(printf '%s' "$CHANGELOG_BODY" | awk -v max="$MAX_LOG" \
                'BEGIN{n=0} {line_len=length($0)+1; if(n+line_len>max) exit; n+=line_len; print}')
            SHOWN=$(printf '%s\n' "$TRUNCATED" | grep -c '^-' || true)
            TOTAL=$(printf '%s\n' "$CHANGELOG_BODY" | grep -c '^-' || true)
            SKIPPED=$(( TOTAL - SHOWN ))
            CHANGELOG_BODY="${TRUNCATED}"$'\n'"_… and ${SKIPPED} more commits — [compare ${LAST_TAG}…${TAG_PREFIX}${VERSION}](https://github.com/${REPO}/compare/${LAST_TAG}...${TAG_PREFIX}${VERSION})_"
        fi
        PR_BODY+="${CHANGELOG_BODY}"$'\n\n'
    else
        PR_BODY+="Initial release."$'\n\n'
    fi
    PR_BODY+="**To release:** merge this PR. The tag and build will happen automatically."
    PR_TITLE="chore(release): release ${ARTIFACT} version ${VERSION}"
    EXISTING_PR=$(gh pr list --head "$BRANCH" --json url --jq '.[0].url' 2>/dev/null || true)
    if [[ -n "$EXISTING_PR" ]]; then
        gh pr edit "$BRANCH" --title "$PR_TITLE" --body "$PR_BODY"
        PR_URL="$EXISTING_PR"
        echo ""
        echo "Updated existing release PR: ${PR_URL}"
    else
        PR_URL=$(gh pr create --title "$PR_TITLE" --body "$PR_BODY")
        echo ""
        echo "Release PR opened: ${PR_URL}"
    fi
    echo "Merge it to trigger the release build."

# ─── Agent Harness ────────────────────────────────────────────────────────────

# Run a goose agent connected to a Buzz relay (foreground)
goose relay="ws://localhost:3000" agents="1" heartbeat="0" prompt="" key="$BUZZ_PRIVATE_KEY":
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    source ./scripts/_goose-env.sh "{{relay}}" "{{key}}" "{{agents}}" "{{heartbeat}}" "{{prompt}}"
    exec env "${env_args[@]}" ./target/release/buzz-acp

# Run a goose agent in the background (screen session named 'goose-agent-N')
goose-bg relay="ws://localhost:3000" agents="1" heartbeat="0" prompt="" key="$BUZZ_PRIVATE_KEY":
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    source ./scripts/_goose-env.sh "{{relay}}" "{{key}}" "{{agents}}" "{{heartbeat}}" "{{prompt}}"
    screen -dmS goose-agent-{{agents}} bash -c "$(printf '%q ' env "${env_args[@]}") ./target/release/buzz-acp"
    echo "Agent running in screen session 'goose-agent-{{agents}}'. Attach with: screen -r goose-agent-{{agents}}"

# ─── Benchmarking ─────────────────────────────────────────────────────────────

# Run the Buzz orchestra benchmark — leaderboard-eligible by default (TB 2.1, k=5, Sonnet+Haiku). Stands up its own Docker stack; --gui opens a live spectator desktop app; other flags pass to benchmark.py (--dataset/--path, --include-task, --attempts, --manifest, --dry-run, ...)
benchmark *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    uv run --project benchmarks/harbor-buzz-orchestra/testbed \
        benchmarks/harbor-buzz-orchestra/scripts/benchmark.py {{ARGS}}

# Stop the benchmark Docker stack (state and channels are kept)
benchmark-down:
    docker compose --project-name buzz-benchmark down

# ─── Worktree lifecycle (L11) ─────────────────────────────────────────────────

# Seed a worktree you cut by hand, exactly as the Beekeeper launcher would.
#
# Reads the tree's own sandbox.yml — one declaration, one parser (`bee`). A
# second copy of it in bash is how the reclaim list came to disagree with itself
# in four places, so this recipe parses nothing.
#
# On APFS the heavy directories are cloned, so a 36 GB target/ costs seconds and
# almost no disk, and the cargo registry is shared through one pool per
# repository instead of re-downloaded per tree.
#
#   just sandbox-seed ../beekeeper-wt-mine                 print the plan only
#   just sandbox-seed ../beekeeper-wt-mine --confirm        seed it
#   just sandbox-seed ../beekeeper-wt-mine --run-recipes --confirm
#                                                          and run the project's
#                                                          own setup recipes
#
# Seed a hand-cut worktree's build state, as the Beekeeper launcher would
sandbox-seed TREE *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    # A fresh worktree has nothing built, so look for a usable `bee` rather than
    # assuming this checkout has one.
    BEE="${BUZZ_BEE:-$(command -v bee 2>/dev/null || true)}"
    for candidate in \
        "{{justfile_directory()}}/target/release/bee" \
        "{{justfile_directory()}}/target/debug/bee" \
        "/Applications/Beekeeper.app/Contents/MacOS/bee"; do
        [[ -n "$BEE" && -x "$BEE" ]] && break
        BEE="$candidate"
    done
    if [[ -z "$BEE" || ! -x "$BEE" ]]; then
        echo "no 'bee' found. Build it with 'cargo build -p buzz-cli', install the app," >&2
        echo "or set BUZZ_BEE to a binary." >&2
        exit 1
    fi
    "$BEE" sandbox seed --tree "{{TREE}}" --from "{{justfile_directory()}}" {{ARGS}}

# What this project declares as build state, and what reclaim would do to it
sandbox-plan *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    BEE="${BUZZ_BEE:-$(command -v bee 2>/dev/null || true)}"
    [[ -n "$BEE" && -x "$BEE" ]] || BEE="{{justfile_directory()}}/target/debug/bee"
    [[ -x "$BEE" ]] || { echo "no 'bee' found; run 'cargo build -p buzz-cli'" >&2; exit 1; }
    "$BEE" sandbox plan {{ARGS}}


# Remove the merged, clean lane worktrees; list the merged-but-dirty ones; refuse
# every unmerged tree, however old. Opt-in, never wired into `just check` or any
# hook: this removes directories, and nothing that removes a directory should run
# because somebody typed a different command. Pass --dry-run first — it prints the
# whole plan and removes nothing. --targets also deletes the build state every
# merged tree declares in its own sandbox.yml, held or not: no commit lives in a
# build directory. --targets needs `bee`, and refuses without it rather than
# guessing at `target/` alone.
#
# Remove the merged, clean lane worktrees and refuse everything else
worktrees-prune *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    bash "{{justfile_directory()}}/scripts/worktrees-prune.sh" {{ARGS}}

# ─── Bundle from a landed commit (P1) ─────────────────────────────────────────

# Assert the sidecar set is one set: what the bundle declares (externalBin +
# the coding-session provider), what `desktop-standalone` builds and copies,
# what `app-from` builds and copies, and what `_ensure-sidecar-stubs` stubs.
# A sidecar that drifts out of one list is otherwise invisible until someone
# dates the files beside the app by hand.
sidecar-parity-check:
    node desktop/scripts/check-sidecar-parity.mjs

# Build the founder's `Beekeeper Dev.app` from a commit that has LANDED on
# origin/main, install it to ~/Applications (keeping the previous as .prev) and
# relaunch. Debug profile, so the bundle keeps the dev keyring service and the
# existing dev identity; a throwaway detached worktree, so this never touches a
# checkout with an app running out of it.
#
#   just app-from <sha>                 build, install, relaunch
#   just app-from <sha> --no-install    build and verify only
#   just app-from <sha> --fresh         rebuild from cold
app-from *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="{{justfile_directory()}}/bin:$PATH"
    bash "{{justfile_directory()}}/scripts/app-from.sh" {{ARGS}}
