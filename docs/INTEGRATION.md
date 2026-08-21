# Integration workflow — agiterra/buzz

How this fork tracks upstream `block/buzz` while keeping each feature separately
maintainable and upstreamable. Quick version: [CONTRIBUTING-FORK.md](../CONTRIBUTING-FORK.md).

## Branch roles

| Branch | Meaning | History |
|---|---|---|
| `main` | Pure mirror of `block/buzz` main. **Never** carries local commits. | ff-only |
| `feature/<name>` | One upstreamable feature. Based on `main`, or stacked on another feature. No CI files, no agiterra-only bits. | rebased per sync |
| `integration/glue` | Cross-feature adaptation patches + `scripts/integrate.sh` + this doc + `.woodpecker/` CI. A patch series **rebased onto the feature assembly** each build (so glue commits may edit files that only exist on feature branches); its current base is recorded in `integration/glue-base`. Force-pushed like `integrated`. | rebased per build |
| `integrated` | The assembled product: `main` + every feature + glue. **Deploys and daily work use this.** | rebuilt + force-pushed; pin via `build/*` tags |

`integrated` is rewritten on every rebuild (like linux-next). Consumers re-fetch
rather than pull; anything that must not move pins a `build/YYYY-MM-DD[.n]` tag.
Relay images are tagged with the build tag they were built from.

Current stack:
- `feature/project-containers` — projects as containers (channels/forums/repos/workflows under projects; sidebar + management UI)
- `feature/project-access` (stacked on containers) — visibility levels (`buzz-access`), project ACL, private-repo gating
- `feature/builtin-shell` (stacked on access) — built-in shell terminals (`buzz-shell-host` sidecar, session broker + agent consent, `buzz session` CLI) and NIP-ST shared terminals (kinds 30623/24310/24311: project members observe sessions read-only)
- `feature/project-pulse` (stacked on builtin-shell) — Project Pulse: kind 44240 entries, the project-membership read/write gate, `buzz pulse`, ACP context injection, and the desktop Pulse screen; the project-child wiring (sidebar row, home card, route registration) lives in glue. Known impurity: `crates/buzz-core/src/pulse.rs`, `crates/buzz-cli/src/commands/pulse.rs`, `desktop/src/features/project-pulse/lib/{pulseFormat,pulseQueries}.ts` reference `feature/coding-sessions` symbols, so the branch does not build standalone — Pulse's whole thesis is explicit claims *beside* observed coding-session state
- `feature/coding-sessions` — coding sessions with a Claude Code provider (NIP-CSC/CSL/CST kinds 44220–44225, `buzz-session-provider` crate, workspace UI, transcript export, `buzz sessions` CLI); project-shelf coupling lives in glue. Known impurity: `agent_fence::tests::the_fenced_briefing_never_tells_a_session_to_write_the_pulse` asserts that `buzz_acp::BASE_PROMPT` keeps the Pulse write instruction its *unfenced* audience needs — a string that only `feature/project-pulse` introduces — so that one test is red on this branch standalone and green on the assembly. The pair is the point: the rule it pins is "never instruct an agent to do something its own environment forbids", and it is only checkable from both sides at once

## The sync loop

```
scripts/integrate.sh              # full: sync main, rebase stack, rebuild, gate, tag, push
scripts/integrate.sh --no-push    # dry run locally
scripts/integrate.sh --skip-gate  # when you've just run the gate manually
```

`git rerere` is enabled by the script: conflict resolutions are recorded and
replayed automatically on the next rebuild, so recurring conflicts are
resolved once. A **new** conflict stops the script — resolve it, `git rebase
--continue`, and rerun.

## Developing a change (prototype first)

Do **not** develop new work directly on the feature branches. Keeping the
branches separate during iteration costs a cross-branch dance per edit; the
separation is only actually needed at push time. Instead:

1. **Prototype on the assembly.** Cut a scratch branch from the current
   build: `git checkout -b wip/<topic> integrated-build`. Commit freely
   there (still `git commit -s`); feature code and cross-feature wiring land
   together.
2. **Test with the user at each step.** Build and run locally after each
   feature addition and wait for the user to confirm the behavior before
   moving on. The scratch tree is byte-for-byte what would ship, so this is
   the real test — not an approximation of it.
3. **Split when the user confirms it's ready to push.** Distribute the work
   to the branches that own each file, base-most first for stacked branches:
   on each `feature/<name>`, `git checkout wip/<topic> -- <paths it owns>`
   and commit with a real message. A file belongs to the branch that
   introduced it (`git ls-tree feature/<name> -- <path>` to check); changes
   to shared upstream files go to the feature they serve. Whatever remains —
   cross-feature wiring, files only the assembly has — becomes an
   `integration/glue` commit (added *after* the ceremony rebuild, per
   "Adding a feature" / integrate.sh).
4. **Reassemble and verify equivalence.** Run the ceremony, add the glue
   commit(s) on the rebased glue, then confirm the shipped tree is the
   tested tree: `git diff wip/<topic> integrated-build` must be empty (or
   every remaining hunk explained). Then gate, push, and delete
   `wip/<topic>`.

If the ceremony refs move under you while a `wip/*` branch is in flight
(someone else pushed a rebuild), rebase the wip branch onto the new
`integrated-build` before splitting — patch-ids, not ahead/behind counts,
tell you what's actually yours.

## Adding a feature

1. `git checkout -b feature/<name> main` (or stack on another feature if it
   genuinely depends on it — prefer independence).
2. Keep it upstream-clean: no `.woodpecker/`, no deploy tooling, no references
   to other features. Cross-feature adaptation goes in `integration/glue`.
3. Add the branch to `FEATURES` in `scripts/integrate.sh` (on `integration/glue`),
   in merge order (a stacked branch after its base).
4. Run `scripts/integrate.sh`.

## Retiring a feature (accepted upstream)

When upstream merges a feature, at the next sync the rebase collapses to
nothing (or to a small residual diff). Delete the branch, remove it from
`FEATURES`, keep any residual as a glue patch until upstream releases it.

## Upstreaming a feature

1. Create (once) a **public** GitHub fork of `block/buzz` — private repos
   cannot open PRs against public upstream. Suggested: fork under the GitHub
   user proposing the PR.
2. `git push <public-fork> feature/<name>` (freshly rebased on `main`).
3. Open the PR from the public fork. Review feedback lands on the same branch;
   the private repo keeps consuming it via the normal sync loop.

## CI

`.woodpecker/` pipelines (on `integration/glue`, therefore present on
`integrated`) run the gate on pushes to `integrated` and PRs. Feature branches
are validated locally by `integrate.sh`'s gate — they intentionally carry no CI
files so they stay upstream-clean.

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

The ceremony gate is **narrower than CI**. `integrate.sh` runs
`cargo test --workspace`, `just desktop-check`, `just desktop-test`, and
`pnpm typecheck`; `.woodpecker/gate.yml` additionally runs
`just conformance-check` and `just export-viewer-manifest-test`, starts
Postgres/Redis/MinIO as services, and migrates a **fresh** database. A
ceremony can therefore go green locally and still land red on CI. Run the two
extra steps by hand before a ceremony you intend to deploy, and remember a
fresh-database `cargo run -p buzz-admin -- migrate` exercises migration
ordering that an already-migrated local database cannot.

Without a Woodpecker login you can still read pipeline state: the badge and
CCTray feeds are public — `https://ci.agiterra.org/api/badges/1/status.svg`
(add `?branch=integrated`) and `.../api/badges/1/cc.xml`, the latter carrying
the pipeline number and timestamp. Everything under `/api/repos/...` needs
auth, so logs are login-only. Woodpecker fires on pushes to `integrated`, so
**re-running a suspected-flaky pipeline without UI access means producing a
new build** (a fresh ceremony) rather than restarting the old one. Whether a
deploy actually landed is observable from outside: publish a probe event of a
kind the new build introduced and read the relay's verdict — an older relay
answers `restricted: unknown event kind`.

Local-ceremony environment notes (Brian's post-migration machine): run the
script as `LEFTHOOK=0 CHECK_FILE_SIZES_BASE=$(git rev-parse upstream/main)
scripts/integrate.sh` — there is no `origin/main` in this clone (origin is
the relay), so the desktop file-size hook needs the explicit base, and the
hook-driven `cargo fmt --all` otherwise dirties the tree mid-run. The
feature stack and `integration/glue-base` must exist as local branches
(created from `upstream/*`), and the rerere cache was rebuilt 2026-08-15 —
the recurring cross-feature union resolutions replay automatically again.

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
`image:` in both `.woodpecker/*.yml` in the same glue change.

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
(desktop `pnpm check` is biome + px-text + pubkey-truncation); if it is ever
added, raise the clone depth so the stamped `.ci/base-ref` commit is present.

## Deploying

The relay deploys itself: `buzz-autodeploy.timer` on the agincus host polls
Woodpecker every 5 minutes, and when the newest `integrated` push pipeline is
green it exports the source from the forge git mirror at that commit, builds
`buzz-relay:<short-sha>` inside the `buzz` instance, takes a `pg_dump` backup
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

`build/*` tags remain the pins for reproducing or manually rolling to a known
build. Desktop dev runs `just desktop-standalone` from `integrated`.
