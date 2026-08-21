# Working in agiterra/beekeeper

This repo is **Bee Keeper**, agiterra's fork of
[block/buzz](https://github.com/block/buzz). It is a single-branch repo: `main`
is the product, and upstream is merged in occasionally.

## Branches

| Branch | Meaning |
|---|---|
| `main` | The product. Default branch; deploys and daily work use it. |
| topic branches | Ordinary short-lived branches, merged back via PR or fast-forward. |
| `build/YYYY-MM-DD[.n]` (tags) | Immutable pins of a deployed build. Relay images are tagged with the build tag they came from. |

Commit with `git commit -s` — the **DCO Check** fails any PR with a commit
missing a `Signed-off-by` trailer.

This repo previously carried an upstreamable-feature-branch model
(`feature/*` rebased onto a `main` mirror, reassembled through
`integration/glue` into a force-pushed `integrated`). That is gone. Upstream
receives far too many submissions for ours to land in useful time, so the
branches were being maintained for a merge that was never going to happen —
and two of them had already stopped building standalone. The vanilla mirror,
and the one CI patch that runs on ci.agiterra.org, now live in
[agiterra/buzz](https://github.com/agiterra/buzz).

## Merging upstream

```sh
git fetch upstream
git merge upstream/main        # merge, never rebase — this is shared history
```

Two things to check before starting one:

- **Migration numbering.** This fork owns `migrations/0032`–`0040`. If upstream
  has added migrations past `0031`, the numbers collide and the renumbering has
  to be resolved deliberately — a migration that changes number after it has run
  anywhere is a data-loss hazard, not a merge conflict.
- **Feature collision.** Upstream has independently shipped its own `projects`
  and `pulse`. The protocol layer is compatible (`KIND_PROJECT = 30621` is the
  same number on both sides), but the desktop screens are two independent
  redesigns of one surface. Picking hunks there produces a mixed, broken UI;
  decide whether to retire ours or keep it before touching the files.

## CI

`.woodpecker/` pipelines run the gate on pushes to `main` and on PRs, at
[ci.agiterra.org](https://ci.agiterra.org).

Known runner-environment limitations (excluded from the gate, still run on
dev machines; an upstream-issue candidate):
- `buzz-relay` `api::mesh_demo::…round_trips_echo` — loopback QUIC cannot
  complete inside the docker-in-incus runner (10s ECHO_TIMEOUT expires; the
  test self-skips on redis-less dev machines anyway).
Re-check if the runner topology changes.

The `buzz-agent` `fake_llm` steer/cancel tests were excluded here for a while
and are **no longer** — they were test-design races, not runner limits, and
are fixed. Both had to act on a turn that was still in flight while the fake
provider answered in ~1ms: `steer_folds_into_active_turn_without_cancelling`
lost when the runner got *fast* (the turn finished before the steer arrived —
3 of 4 attempts once the 2026-08 CI caching made the step quick), and
`cancelled_turn_with_usage_emits_notification_before_response` lost when it got
*slow* (it opened its gate as soon as cancel was written to stdin, not once the
agent had applied it). The fake provider now takes an `LlmGate`: it captures
the chosen round's request and holds the response until the test releases it,
so the steer/cancel provably lands mid-turn. Measured before the fix, at
`--test-threads 8`: 2/20 failures idle and 3/15 under full CPU saturation;
after, 0/65 across both conditions.
- `buzz-pair-relay` `integration::test_120s_timeout` /
  `test_cancellation_immediate` — 2 s close-window assertions that flake
  under a fully loaded gate (observed 2026-08-15 during a local ceremony);
  51/51 pass in isolation. Load-sensitivity, not a regression.

`just ci` is **narrower than the gate**. The gate additionally runs
`just conformance-check` and `just export-viewer-manifest-test`, starts
Postgres/Redis/MinIO as services, and migrates a **fresh** database — so a
local run can go green and still land red. Run the two extra steps by hand
before a push you intend to deploy, and remember that a fresh-database
`cargo run -p buzz-admin -- migrate` exercises migration ordering that an
already-migrated local database cannot.

Without a Woodpecker login you can still read pipeline state: the badge and
CCTray feeds are public — `https://ci.agiterra.org/api/badges/1/status.svg`
(add `?branch=main`) and `.../api/badges/1/cc.xml`, the latter carrying
the pipeline number and timestamp. Everything under `/api/repos/...` needs
auth, so logs are login-only. Whether a deploy actually landed is observable
from outside: publish a probe event of a kind the new build introduced and read
the relay's verdict — an older relay answers `restricted: unknown event kind`.

Mirrors + CI run on the `forge` incus container on agincus (bare mirrors at
`/srv/git`, Woodpecker at `ci.agiterra.org`). GitHub remains the canonical
host; the forge is additive infrastructure.

### CI caching

The gate/nightly rust and desktop steps run on the prebaked **`buzz-ci:N`**
image (apt deps + mold only — `scripts/ci-image/Dockerfile`; it cannot live
under `.woodpecker/`, whose config fetcher rejects subdirectories), built
directly on the forge docker daemon so Woodpecker uses it without a registry:

```sh
ssh agincus "incus exec forge -- docker build -t buzz-ci:1 -" \
  < scripts/ci-image/Dockerfile
```

No Rust toolchain is baked in — toolchains live in the mounted caches, so
`rust-toolchain.toml`/hermit bumps need no rebuild. Rebuild **only when the
apt dependency set changes**: bump the tag (`buzz-ci:2`), rebuild, and update
`image:` in both `.woodpecker/*.yml` in the same change.

Host cache mounts under `/srv/ci-cache` (require the repo's Trusted→Volumes
flag in Woodpecker):

| Host dir | Mounted at | Holds |
|----------|-----------|-------|
| `hermit-rust` | `/woodpecker/repo/.hermit/rust` | CARGO_HOME: registry, git checkouts, cargo/rustup bins |
| `rustup` | `/root/.rustup` | installed toolchains |
| `hermit` | `/root/.cache/hermit` | hermit package downloads |
| `hermit-state` | `/root/.local/state/hermit` | hermit unpacked packages |
| `cargo-target` | `/ci-target` (`CARGO_TARGET_DIR`) | shared persistent target dir |
| `pnpm-store` | `/woodpecker/.pnpm-store` | pnpm content-addressed store (pnpm stores on the project's filesystem, not `$HOME`) |

The `hermit-rust` mount targets a **workspace-relative** path because hermit's
rustup package hard-pins `CARGO_HOME=${HERMIT_ENV}/.hermit/rust` at shim-exec
time (ambient env overrides do not stick) — hence the pinned
`workspace: {base: /woodpecker, path: repo}` in both pipelines. Sharing
`cargo-target` across pipelines is safe: `WOODPECKER_MAX_WORKFLOWS=1` runs one
workflow at a time, and cargo's build lock serializes any future overlap.
Wiping any cache dir just costs one cold build. Size is capped by a weekly
root cron on forge that clears `cargo-target` past 60 GB and `hermit-rust`
past 20 GB.

The gate clones with `depth: 1` — nothing in it walks git history. The
file-size ratchet (`just file-size-check`) does **not** run in the gate
(desktop `pnpm check` is biome + px-text + pubkey-truncation); it runs on
pre-push instead, where it resolves its base from `origin/main` directly. If it
is ever added to the gate, raise the clone depth so that merge-base is
reachable.

## Deploying

The relay deploys itself: `buzz-autodeploy.timer` on the agincus host polls
Woodpecker every 5 minutes, and when the newest `main` push pipeline is green it
exports the source from the forge git mirror at that commit, builds
`buzz-relay:<short-sha>` inside the relay instance, takes a `pg_dump` backup
(`/opt/buzz/backup-pre-*.sql.gz`), flips `BUZZ_IMAGE` in
`/opt/buzz/compose/.env`, and restarts with compose health-wait. An unhealthy
relay rolls back to the previous image automatically. Red pipelines never
deploy; a failed attempt leaves `/opt/buzz/autodeploy-failed-<short-sha>` in
the instance so it will not rebuild in a loop (remove the marker to retry).

Paper trail: `journalctl -u buzz-autodeploy` on the host,
`/opt/buzz/deploy.log` and `/opt/buzz/build-<short-sha>.log` in the instance.
The deployer itself lives at `/usr/local/sbin/buzz-autodeploy` on the host —
CI has no credentials for (or access to) the prod instance; the deployer only
pulls from Woodpecker's status DB and the read-only mirror.

**The deployer still watches the branch name `integrated`.** It and the forge
mirror both have to be repointed at `main` and at `agiterra/beekeeper`, or
pushes stop deploying silently. That is host-side work, not a repo change.

`build/*` tags remain the pins for reproducing or manually rolling to a known
build. Desktop dev runs `just desktop-standalone`.

Running a daily-driver Bee Keeper.app and a dev instance side by side on macOS
(distinct icons, no repeated keychain prompts):
[local-desktop-instances.md](local-desktop-instances.md).
