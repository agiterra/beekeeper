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
- `feature/coding-sessions` — coding sessions with a Claude Code provider (NIP-CSC/CSL/CST kinds 44220–44225, `buzz-session-provider` crate, workspace UI, transcript export, `buzz sessions` CLI); project-shelf coupling lives in glue

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
dev machines; both are upstream-issue candidates):
- `buzz-relay` `api::mesh_demo::…round_trips_echo` — loopback QUIC cannot
  complete inside the docker-in-incus runner (10s ECHO_TIMEOUT expires; the
  test self-skips on redis-less dev machines anyway).
- `buzz-agent` `fake_llm::cancelled_turn_with_usage_emits_notification_before_response`
  — the test releases its gated LLM round right after sending cancel, assuming
  stdin wins the race; on loaded cores round 2 completes first and the turn
  legitimately ends `end_turn`, not `cancelled`. Test-design race.
Re-check both if the runner topology changes.
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
