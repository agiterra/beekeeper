# Testing

## Automated Tests

```bash
just test-unit          # unit tests — no infrastructure needed
just test               # unit + integration (starts Docker if needed)
```

`just test` runs unit tests plus integration tests against Postgres and Redis
(started automatically if not already running). Neither task runs the E2E suites in
`beekeeper-test-client` — those are marked `#[ignore]` and require a running relay:

```bash
# Start a relay first (see below), then:
cargo test -p beekeeper-test-client -- --ignored
```

### CI completion integration checks

`just test-ci-completion` runs the CI-result atomic storage and authenticated
webhook composition tests against a newly migrated throwaway database. It is
included in `just test`; the Woodpecker Rust job also runs them with its
existing Postgres/Redis services and explicitly built CLI. The default unit
harness skips these Postgres cases;
core contracts and CLI replay/reconnect tests run without infrastructure.

### The desktop Playwright smoke suite is not in `just ci`

```bash
just smoke              # pnpm build:e2e + the whole desktop smoke project
```

`just ci` (justfile:379) runs `desktop-test` — the Vitest/node unit tests — and
the desktop and web *builds*. It does **not** run the Playwright smoke project,
and neither does any pre-commit or pre-push hook. That is on purpose: the smoke
project ran 1,319 browser cases in about **1.5 hours** on this M-series
laptop on 2026-09-12 (see the steering experience report, `plans/archive/2026-09-12-steering-experience.md` in the agents repository).
That is an observation, not a duration guarantee; fixtures and machine load
change it. Wiring that run into every commit would make the local gate costly.

The cost of that choice is that the suite rots silently — in August 2026 it sat
at 75 failing tests that no green `just ci` ever mentioned. So run `just smoke`
deliberately: before landing a change that touches desktop UI, and before a
desktop release. Do not add it to `just ci`.

### Per-round E2E: `just e2e-affected`

```bash
just e2e-affected                  # changes since the nearest main
just e2e-affected main~3           # or any other base
node desktop/scripts/e2e-affected.mjs --explain   # the selection alone, with reasons
node desktop/scripts/e2e-affected.mjs port        # this worktree's preview port
```

Between rounds, run only the smoke specs a change can reach. The selector
(`desktop/scripts/e2e-affected.mjs`) lists the files changed since
`merge-base(base, HEAD)`, untracked files included. The default base is the
nearest main: of `main` and every `<remote>/main`, the one whose merge-base
with HEAD is newest, so neither a remote name nor a stale local `main` is
baked in. For each changed source file it collects the `data-testid` values
the file renders — for a hook or model that renders none, the values of the
nearest components importing it, up to two levels up — and the user-visible
string literals the file itself holds (a spec that clicks "In 30 minutes" by
text). It selects every smoke spec naming one of them, every changed spec,
every spec importing a changed test helper (through other helpers too), and a
fixed core set: app boot (`boot-splash.spec.ts`, the shell test in
`smoke.spec.ts`), sending a message (`messaging.spec.ts`), and opening and
prompting a coding session (two tests in `coding-sessions.spec.ts`).

A change to a global file prints `FULL` and runs the whole project: Tailwind,
PostCSS and Vite config, `index.html`, `desktop/package.json`, the lockfile,
`desktop/public/`, `src/shared/styles/`, the shared markdown renderer,
`main.tsx`, `App.tsx`, `AppShell*`, the router, everything under
`src/testing/` (the whole mock bridge), `tests/helpers/bridge.ts` and
`previewOrigin.ts`, `tests/fixtures/`, the preview server, and the Playwright
config other than spec registration. On stderr the selector names a source
file no spec reaches, a non-source file under `src/` (JSON, images — not
traced), and a file reached only through its importers' ids, since that path
stops at the first component that renders ids and can miss a spec. Read those
lines; the selection is a heuristic.

The recipe builds the e2e bundle once and runs the selection in two passes
(`smoke`, then `smoke-serial`).

**Judging the run.** `desktop/tests/e2e/known-failures.json` lists the tests
that fail on `main`, each with the commit it was verified on and its ledger
item. The recipe sorts every failure against it and fails on a new one, so a
gate does not rebuild `main` to learn its baseline. It also fails a run it
cannot trust, even with no failing test: a top-level Playwright error (a spec
or helper that does not load, a server that does not start), no test results
in either pass, a non-zero Playwright exit that no failing test explains, or a
core-set test that did not run. A known failure that passes is reported so it
can be re-verified and removed; add an entry only after running the test on an
untouched `main`.

An entry marked `intermittent` is a blind spot: a real regression in that test
reads as known. `channels.spec.ts:2474` (forum unread count) is listed that
way since SV-48; when a change reaches it, run it alone and read the failures
yourself. The `project-team-setup.spec.ts` entries were pruned under SV-47
after three clean file-alone runs at `623b2c301`. The list came
from three four-worker full runs at `dad4726ba` on 2026-10-04; each of the
second and third found entries the earlier ones missed, so a fourth may too.

**Ports.** The preview port is `E2E_PORT` if set (`BEEKEEPER_E2E_PORT` is the older
spelling), 4173 under `CI`, and otherwise derived from the checkout's path
(4300–4999), so two worktrees never share a server by default and one worktree
reuses its own. `tests/helpers/previewOrigin.ts` gives the port and origin to
`playwright.config.ts`, `playwright.perf.config.ts`, and every spec that needs
the absolute origin; `just desktop-screenshot` uses it too.

**Workers.** The smoke project runs `E2E_WORKERS` files at once (default 4
locally, 1 under `CI`; each file stays on one worker, in order; any other value
than a positive integer is an error). The whole project took 27 minutes at four
workers on 2026-10-04 (1,369 tests, `dad4726ba`) against 1.6 hours at one
(ledger 311(s)). The integration project stays at one. A file that fails only
in parallel goes in the `smoke-serial` project, which `pnpm test:e2e`,
`pnpm test:e2e:smoke` and `just e2e-affected` run as a second pass with nothing
else running (`desktop/scripts/e2e-passes.sh`); a bare `playwright test` runs it
beside the others. Do not lower the global count.
`just e2e-affected` is the per-round check; `just smoke` is still the run
before landing.

---

## Live Local Relay

The fastest way to exercise the relay end-to-end is to build the release
binaries once, run `beekeeper-relay`, and drive it with the `bee` CLI. The
CLI signs every request with NIP-98, so you don't need `nak` or hand-rolled
`curl`.

### 1. Setup

```bash
. ./bin/activate-hermit          # activate pinned toolchain
just bootstrap                   # create .env and its stable relay key once
just setup                       # start Docker services, run migrations
```

> **Already running Beekeeper Desktop?** Desktop uses the same Docker container
> names (`buzz-postgres`, `buzz-redis`) and the same
> default ports (`:5432`, `:6379`). `just setup` will reuse those
> services, so **your test relay writes into Desktop's database**. That's
> fine for read/write smoke tests, but: `just reset` wipes Desktop's data
> along with yours. If you need isolation, stop Desktop first or run the
> dev stack on a different Compose project
> (`COMPOSE_PROJECT_NAME=buzz-dev docker compose …`).

`just reset` wipes all local data and starts over — **including Beekeeper
Desktop's data** if its services are sharing your dev stack (see callout
above).

> **Heads up — scrub stale env first.** If your shell inherits any of
> `BEEKEEPER_AUTH_TAG`, `BEEKEEPER_RELAY_URL`, or `BEEKEEPER_PRIVATE_KEY` from a
> prior session (or a staging config), `unset` them before continuing.
> A stale `BEEKEEPER_AUTH_TAG` fails the **local dev relay** with
> `auth_error: signature verification failed` on the first CLI write —
> it is *not* tolerated.
> ```bash
> unset BEEKEEPER_AUTH_TAG BEEKEEPER_RELAY_URL BEEKEEPER_PRIVATE_KEY
> ```

### 2. Build the binaries

```bash
cargo build --release -p beekeeper-relay -p beekeeper-cli -p beekeeper-admin
export PATH="$PWD/target/release:$PATH"
```

Rebuild after any code change — the steps below use the release binaries.

### 3. Start the relay

In a separate terminal (it runs in the foreground):

```bash
set -o allexport
source .env                    # includes the key generated by just bootstrap
set +o allexport
beekeeper-relay                     # release binary from step 2, serves ws://localhost:3000
# alternatives:
# cargo run --release -p beekeeper-relay     # rebuild + run in release
# just relay                            # DEBUG build — fast to launch on a hot cache,
#                                       # but mismatched if step 2 left you on release.
#                                       # Use `just relay-release` if you want the recipe.
```

Verify it's up (back in your working terminal):

```bash
curl -s http://localhost:3000/health           # → ok
curl -s http://localhost:8080/_readiness        # → {"status":"ready"}
```

> Health/readiness/liveness live on a **separate port** (default `8080`,
> `BEEKEEPER_HEALTH_PORT`) so K8s probes bypass auth middleware. The main app
> port also exposes `/health` for convenience.

The relay starts in dev mode (`BEEKEEPER_REQUIRE_AUTH_TOKEN=false`) with the stable
relay identity generated in `.env`. See the env vars table at the bottom if
you need to lock it down.

> **Already running Beekeeper Desktop (or another relay) on `:3000` / `:8080` /
> `:9102`?** Beekeeper binds three ports — main, health, metrics — and any of
> them can collide. Use a separate terminal per role and export the right
> vars in each:
>
> **In the relay terminal** (before launching `beekeeper-relay`):
> ```bash
> export BEEKEEPER_BIND_ADDR=0.0.0.0:3030
> export BEEKEEPER_HEALTH_PORT=8088
> export BEEKEEPER_METRICS_PORT=9202
> export RELAY_URL=ws://localhost:3030     # advertised in NIP-42 challenges
> beekeeper-relay
> ```
>
> **In your working / CLI terminal** (for steps 4+ and the ACP harness):
> ```bash
> export BEEKEEPER_RELAY_URL=http://localhost:3030    # CLI target
> # verify the relay on the overridden ports:
> curl -s http://localhost:3030/health             # → ok
> curl -s http://localhost:8088/_readiness         # → {"status":"ready"}
> ```
>
> Every snippet later in this doc shows the defaults. When you see
> `localhost:3000` / `:8080` in a code block, mentally substitute your
> overrides — or the CLI will end up talking to Beekeeper Desktop's relay.

> **Ignore `just setup`'s "Next steps" banner.** It still prints
> `just relay` (a debug build). Use `beekeeper-relay` from step 2 here —
> step 2 already built the release binary.

When you're done, stop the relay (Ctrl-C in its terminal). If it's
backgrounded or you lost the terminal: `pkill -f beekeeper-relay`. Leaving
it running will collide with the next reviewer who follows this doc on
the same machine.

### 4. Smoke test the CLI against the relay

End-to-end: generate an identity, create a channel, post a message, read it
back. This is the minimum sequence an agent needs to verify a local relay.

```bash
# Generate a keypair
GEN=$(beekeeper-admin generate-key)
export BEEKEEPER_PRIVATE_KEY=$(echo "$GEN" | awk '/Secret key:/ {print $3}')
PUBKEY=$(echo "$GEN"           | awk '/Public key:/ {print $3}')
echo "pubkey: $PUBKEY"

# Create a channel — the UUID is returned in the response
CHANNEL=$(bee channels create --name "smoke-$$" --type stream --visibility open | jq -r '.channel_id')
echo "channel: $CHANNEL"

# Send a message and read it back
SEND=$(bee messages send --channel "$CHANNEL" --content "hello from smoke test")
EVENT_ID=$(echo "$SEND" | jq -r '.event_id')
bee messages get --channel "$CHANNEL" --limit 5 | jq .

# Fetch the reply chain for a specific message (empty array on a leaf — that's fine)
bee messages thread --channel "$CHANNEL" --event "$EVENT_ID" | jq .
```

A successful run prints `{"event_id":"…","accepted":true,"message":""}` for
the send, and the message body in the `get` output. `thread` returns `[]`
for a leaf message — populated only after a reply comes in (see §6).

### 5. Verify a roster beyond 1,000 members

Use the focused live-relay script when changing channel membership, discovery,
or reconciliation. It proves the three boundaries that DB-only tests cannot:
the relay-served kind 39002 includes a member at roster position 1,501, that
identity can publish a channel message, and targeted reconciliation preserves
its discoverability.

Run this only against an isolated local database. The script inserts fixture
members directly, then drives discovery and messaging through the release CLI
and relay. Keep the release relay from step 3 running and use its configured
relay key for authoritative replacement:

```bash
export PATH="$PWD/target/release:$PATH"
export DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/buzz_roster_e2e"
export BEEKEEPER_RELAY_URL="http://localhost:3030"  # match the relay from step 3
export RELAY_URL="ws://localhost:3030"
export BEEKEEPER_RELAY_PRIVATE_KEY="<same key used by beekeeper-relay>"

scripts/e2e-large-channel-roster.sh
```

Success is directly observable as four `PASS` lines. The first and fourth
include a member count greater than 1,000 and the same late-member pubkey; the
second includes the accepted kind 9 event ID, and the third proves targeted
repair left kind 39000/39001 IDs and tags unchanged:

```text
PASS discovery-before-republish channel=<uuid> members=1502 late_pubkey=<hex>
PASS late-member-action event_id=<hex>
PASS targeted-repair-preserves-metadata-and-admin-events channel=<uuid>
PASS discovery-after-republish channel=<uuid> members=1502 late_pubkey=<hex>
```

The script refuses debug binaries and refuses a `bee` or `beekeeper-admin` resolved
outside this checkout's `target/release`. It also requires the targeted admin
operation to use `BEEKEEPER_RELAY_PRIVATE_KEY`; never substitute an ephemeral signer
for an authoritative replacement.

### 6. Going deeper

For full coverage of every CLI command (54 subcommands across 12 groups),
follow [`crates/beekeeper-cli/TESTING.md`](crates/beekeeper-cli/TESTING.md).

The relay's HTTP bridge accepts three endpoints — useful if you're testing
a client other than `beekeeper-cli`:

| Endpoint        | Purpose                            |
|-----------------|------------------------------------|
| `POST /events`  | Submit a signed Nostr event        |
| `POST /query`   | NIP-01 filter query (returns events) |
| `POST /count`   | NIP-45 count query                 |

All three accept NIP-98 auth (recommended) or, in dev mode, an `X-Pubkey`
header fallback. There is no REST API for fetching message threads — use
`POST /query` with an `#e` filter, or `bee messages thread`.

---

## ACP Harness (optional, end-to-end with a real agent)

`beekeeper-acp` connects an ACP-speaking agent (goose, codex, claude code,
beekeeper-agent) to the relay. The harness listens for events, drives the
agent over stdio, and the agent replies through MCP tools.

Minimum recipe — assumes the relay from step 3 is running and the channel
`$CHANNEL` from step 4 still exists. The agent identity must be **different**
from the sender identity (`BEEKEEPER_ACP_RESPOND_TO=anyone` still skips events
the agent signed itself).

```bash
cargo build --release -p beekeeper-acp
export PATH="$PWD/target/release:$PATH"

# 1. Save your sender identity from step 4 — you'll need it to @mention the agent
SENDER_SK="$BEEKEEPER_PRIVATE_KEY"

# 2. Mint a fresh agent identity and capture its pubkey
AGENT_GEN=$(beekeeper-admin generate-key)
AGENT_SK=$(echo "$AGENT_GEN" | awk '/Secret key:/ {print $3}')
AGENT_PUBKEY=$(echo "$AGENT_GEN" | awk '/Public key:/ {print $3}')

# 3. Add the agent as a member of $CHANNEL — still using the sender identity.
#    Skip this and the agent boots to "discovered 0 channel(s) → agent will
#    sit idle" and silently ignores every mention.
bee channels add-member --channel "$CHANNEL" --pubkey "$AGENT_PUBKEY" --role member

# 4. Switch to the agent identity and start it.
#    beekeeper-acp wants ws:// (not http://). If you set BEEKEEPER_RELAY_URL to an
#    http:// URL in step 3, set the ws:// equivalent here — same host/port.
export BEEKEEPER_PRIVATE_KEY="$AGENT_SK"
export BEEKEEPER_RELAY_URL=ws://localhost:3000   # match step 3 (e.g. ws://localhost:3030 if overridden)
export BEEKEEPER_ACP_RESPOND_TO=anyone           # default is owner-only; opens the gate for testing
# NIP-AE core-memory prompt injection is on by default; set BEEKEEPER_ACP_NO_MEMORY=true to opt out.
export GOOSE_MODE=auto                        # must be 'auto' or goose hangs on prompts

beekeeper-acp                                    # foreground; logs to stdout (run in a separate terminal)

# Optional: turn on per-turn tracing if the default log is too quiet.
# RUST_LOG=beekeeper_acp=debug beekeeper-acp
```

> **Using a different ACP agent?** The default recipe assumes `goose` is on
> `$PATH` and configured (`goose --version` should print). For codex / claude
> code / beekeeper-agent, set `BEEKEEPER_ACP_AGENT_COMMAND` and `BEEKEEPER_ACP_AGENT_ARGS`
> accordingly — see `crates/beekeeper-acp/README.md`. Without these, beekeeper-acp
> will fail to spawn the agent subprocess on startup.

If you started the agent before adding it to the channel, just run the
`add-member` afterwards — it picks up the membership notification live and
subscribes without restart (`membership notification: subscribing to new channel …`).

The justfile also ships `just goose key="$AGENT_NSEC"` (foreground) and
`just goose-bg key="$AGENT_NSEC"` (background screen session) which set the
same env. See `crates/beekeeper-acp/README.md` for parallel agents, heartbeats,
respond-to gates, and forum subscriptions.

To exercise deferred ACP startup, add `BEEKEEPER_ACP_LAZY_POOL=true` before launching
`beekeeper-acp`. The harness should connect, authenticate, subscribe, and publish
online presence without starting the configured ACP child. The first accepted,
flushable mention should start exactly one child and then dispatch the queued
message. Automated coverage in `pool_lifecycle_state` pins single-wake,
retry/backoff, and stale-result behavior; it does not replace this real
relay/process smoke test.

Send the agent a task — switch your shell back to the **sender** identity
from step 4 and @mention the agent:

```bash
export BEEKEEPER_PRIVATE_KEY=$SENDER_SK          # the key from step 4
bee messages send --channel "$CHANNEL" \
  --content "Hey agent, reply PONG only."

# Wait 10–90s, then read the channel — the agent's reply is a kind:9 from
# AGENT_PUBKEY. The current ACP build is quiet on stdout during a turn, so
# `bee messages get` is how you confirm it ran.
bee messages get --channel "$CHANNEL" --limit 5 | jq '.[] | {pubkey, content}'
```

Replies are kind:9 in the same channel; `bee messages thread --channel <id>
--event <event_id>` fetches the reply chain for a specific mention.

---

## Configuration reference

The relay reads all configuration from environment variables. Defaults work
out of the box with `just setup` or `just relay`. Common overrides:

| Variable                          | Default                     | Notes |
|-----------------------------------|-----------------------------|-------|
| `BEEKEEPER_BIND_ADDR`                | `0.0.0.0:3000`              | Main app port |
| `BEEKEEPER_HEALTH_PORT`              | `8080`                      | `/_liveness`, `/_readiness` |
| `BEEKEEPER_METRICS_PORT`             | `9102`                      | Prometheus `/metrics` |
| `RELAY_URL`                       | `ws://localhost:3000`       | Advertised in NIP-11 / NIP-42 challenges. **Note: no `BUZZ_` prefix.** |
| `DATABASE_URL`                    | `postgres://buzz:buzz_dev@localhost:5432/buzz` | |
| `REDIS_URL`                       | `redis://localhost:6379`    | |
| `BEEKEEPER_REQUIRE_AUTH_TOKEN`       | `false`                     | When true, REST requires NIP-98 (no `X-Pubkey` fallback) |
| `BEEKEEPER_REQUIRE_RELAY_MEMBERSHIP` | `false`                     | When true, only pubkeys in `relay_members` can connect |
| `BEEKEEPER_DRAIN_JITTER_MS`          | `0` (off)                   | Per-connection upper bound, in ms, for the random delay before each live WebSocket gets its `1012 Service Restart` close on graceful shutdown. `0` closes every socket at once (the previous behavior). A positive value spreads closes uniformly over `[1, value]` ms to avoid a reconnect thundering herd on rolling deploys. Values above `20000` are capped to `20000` (`MAX_DRAIN_JITTER_MS`) to leave close-frame delivery headroom under the relay's 30s hard-drain timeout. Empty or whitespace-only is treated as unset (off); a non-integer fails startup loudly. |
| `BEEKEEPER_AUDIT_ENABLED`            | `true`                      | Tamper-evident event/media audit log. Set `false`/`0`/`off` to skip its DB pool and writes. Does not disable the separate moderation audit trail. |
| `BEEKEEPER_AUTO_MIGRATE`             | `false`                     | Opt in with `true`/`1`/`yes`/`on` to run embedded SQLx migrations on relay startup |
| `RELAY_OWNER_PUBKEY`              | unset                       | Bootstrapped as `owner` in `relay_members` at first start |
| `BEEKEEPER_ALLOW_NIP_OA_AUTH`        | `false`                     | Enable NIP-OA owner attestation for membership |
| `BEEKEEPER_WEB_DIR`                  | unset (source), `/srv/buzz/web` (container) | Directory containing the invite landing bundle; the production container enables it so `/invite/{code}` always works |
| `BEEKEEPER_SERVE_GIT_WEB_GUI`        | `false`                     | Set to `true` or `1` to expose the bundled Git repository browser at `/` and `/repos/...`; invite routes do not depend on this flag |

CLI-side, only two matter for testing:

| Variable                | Default                  | Notes |
|-------------------------|--------------------------|-------|
| `BEEKEEPER_RELAY_URL`      | `http://localhost:3000`  | CLI relay base; accepts `ws(s)://` and normalises |
| `BEEKEEPER_PRIVATE_KEY`    | — (**required**)         | `nsec1…` or 64-char hex |
| `BEEKEEPER_AUTH_TAG`       | unset                    | Optional NIP-OA owner attestation JSON |

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `relay error 500` or `400: restricted: not a channel member` after a code change | Stale binary | Rebuild and re-export `PATH`; or `cargo run` directly |
| `Address already in use` on relay start (os error 48 on macOS, 98 on Linux) | Another relay (or stale process) holding `:3000` / `:8080` / `:9102` (or your override ports) | The panic line names the failing port — read it first. Then `lsof -iTCP:3000,8080,9102 -sTCP:LISTEN` (or your override equivalents). Kill the offender (`pkill -f beekeeper-relay`) or use the port-override block in step 3. If you already overrode and *still* collide, a prior reviewer left a relay running on the same alt ports — kill it or pick fresh ports |
| `auth_error: BEEKEEPER_PRIVATE_KEY is required` | Env not exported into the CLI's shell | `export BEEKEEPER_PRIVATE_KEY=...` (or pass `--private-key`) |
| `auth_error: BEEKEEPER_AUTH_TAG verification failed … signature verification failed` | A stale `BEEKEEPER_AUTH_TAG` inherited from a parent shell. The local dev relay rejects it. | `unset BEEKEEPER_AUTH_TAG` (see the scrub block in step 1) |
| `auth-required: verification failed` on a closed relay | NIP-OA attestation needed | Set `BEEKEEPER_AUTH_TAG` to the owner-issued JSON, or relax `BEEKEEPER_REQUIRE_RELAY_MEMBERSHIP` |
| `channels list` empty after `channels create` | The CLI doesn't echo the channel UUID; use the filter shown in step 4 | Or `POST /query` with `{"kinds":[39002]}` |
| ACP agent ignores all events | `BEEKEEPER_ACP_RESPOND_TO=owner-only` (default) with no owner configured | Set `BEEKEEPER_ACP_RESPOND_TO=anyone` for testing |
| ACP logs `discovered 0 channel(s)` / `no channel subscriptions resolved` | Agent identity isn't a member of any channel | `bee channels add-member --channel "$CHANNEL" --pubkey "$AGENT_PUBKEY" --role member` from another identity |
| `GOOSE_MODE` warning, agent hangs | Not set | `export GOOSE_MODE=auto` |
| Tests pass locally but CI fails | Forgot to run `just ci` | `just ci` runs the gate (fmt, clippy, unit tests, desktop/web builds) |
