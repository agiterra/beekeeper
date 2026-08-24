# Working in agiterra/beekeeper

This repo is **Bee Keeper**, agiterra's fork of
[block/buzz](https://github.com/block/buzz). It is a single-branch repo: `main`
is the product, and upstream is merged in occasionally.

## Remotes

| Remote | Repo | What it is |
|---|---|---|
| `origin` | `hive.agiterra.org/git/<owner>/agiterra-beekeeper` | The relay's **own** git hosting — Bee Keeper serving its own source. Needs Nostr credentials (`git-credential-nostr`). |
| `upstream` | [agiterra/beekeeper](https://github.com/agiterra/beekeeper) | **Where `main` lives.** `main` tracks `upstream/main`; Woodpecker watches this repo, so this is what CI and the relay deploy from. |
| `vanilla` | [agiterra/buzz](https://github.com/agiterra/buzz) | The block/buzz mirror, plus the one CI patch that runs it on ci.agiterra.org. Upstream work is merged or cherry-picked from here. |

This is not the arrangement described before 2026-08-24, when `origin` was the
GitHub repo and there was no `upstream`. Both names moved at once, so anything
you remember about which is which is probably stale — `git remote -v` is the
only reliable answer.

There is deliberately **no `block/buzz` remote**. `vanilla` already carries
that history, so a second path to the same commits earned nothing and cost 827
remote-tracking refs. If the mirror ever stops being synced, `git remote add
block https://github.com/block/buzz.git` puts the old path back in one command
— note the name, since `upstream` is taken now.

**Do not hard-code `origin` in tooling.** Two pre-push guards did, and both
broke the day the names moved — silently, in different ways:

- `scripts/check-file-sizes-core.mjs` resolved the ratchet base from
  `origin/main`, which stopped existing. Every branch failed the gate at once,
  and since pre-push runs it, every push was blocked.
- `scripts/check-branch-skew.sh` ran `git fetch origin main`, which then meant
  the relay, which wants credentials a hook cannot supply. It hung two pushes
  for 17 minutes each with no output. Its `|| true` caught a fetch that
  *fails*, not one that never returns.

Both now resolve from what `main` tracks (`main@{upstream}`, then
`origin/main`, then `upstream/main`), and anything touching the network in a
hook sets `GIT_TERMINAL_PROMPT=0` so it fails fast rather than blocking. Follow
that pattern rather than adding a third remote-name assumption.

## Branches

| Branch | Meaning |
|---|---|
| `main` | The product. Default branch; deploys and daily work use it. |
| topic branches | Ordinary short-lived branches, **rebased** onto `main` and landed fast-forward. See § Landing a topic branch. |
| `build/YYYY-MM-DD[.n]` (tags) | Immutable pins of a deployed build. Relay images are tagged with the build tag they came from. |

Commit with `git commit -s` — the **DCO Check** fails any PR with a commit
missing a `Signed-off-by` trailer.

## Landing a topic branch

Topic branches are **rebased**, not merged. They are short-lived and
single-author, so rewriting them costs nothing, and `main` stays linear —
which keeps `git log main` a readable list of what shipped rather than a
braid of two-commit merges.

```sh
git fetch origin
git rebase --signoff origin/main   # --signoff: see below
just check                         # re-run the gate — the base moved
git push --force-with-lease        # expected; the branch was rewritten
```

Then land it on `main` as a fast-forward.

Two things to get right:

- **`--signoff`, not a plain `git rebase`.** A rebase preserves the trailers on
  commits it replays untouched, but any commit it *recreates* — a conflict
  resolution, a squash — loses its `Signed-off-by` and fails the DCO gate. The
  `commit-msg` hook does not fire during a rebase. Same applies to
  `git cherry-pick --signoff`.
- **`--force-with-lease`, not `--force`.** It refuses the push if the remote
  moved since your last fetch, which is the only thing standing between a
  rewritten branch and someone else's work on it.

Re-run the gate *after* rebasing, not before. A branch that was green against
an older `main` proves nothing about the base it will actually land on.

**This does not apply to `vanilla/main`** — see § Merging upstream, which
explains why upstream history is merged instead.

This repo previously carried an upstreamable-feature-branch model
(`feature/*` rebased onto a `main` mirror, reassembled through
`integration/glue` into a force-pushed `integrated`). That is gone. Upstream
receives far too many submissions for ours to land in useful time, so the
branches were being maintained for a merge that was never going to happen —
and two of them had already stopped building standalone. The vanilla mirror,
and the one CI patch that runs on ci.agiterra.org, now live in
[agiterra/buzz](https://github.com/agiterra/buzz).

Those branches were retired locally on 2026-08-22, along with the pre-rebrand
`feature/*` lineage whose content is already on `main` under different SHAs.
Nothing was thrown away: each survives as a local-only `archive/<branch>` tag
(`git tag -l 'archive/*'`), which keeps the commits reachable without putting
them back in `git branch`. They are not pushed.

## Merging upstream

```sh
git fetch vanilla
git merge vanilla/main         # merge, never rebase — see below
```

Upstream is the one place this repo does **not** rebase, and the reason is not
style. Those commits already exist in `agiterra/buzz` and in `block/buzz`;
rebasing them would mint new SHAs for history other repos share, so every later
merge would conflict against its own phantom copies. Topic branches carry no
such obligation — nobody else has them — which is why they are rebased.

Three things to check before starting one:

- **`vanilla/main` is not pristine block/buzz.** It carries this fork's CI
  patch on top (`12201c49b`, BIP-340 validation of `oa[0]` in
  `git-sign-nostr`), so a merge brings that along. Wanted today; slated for
  revert once upstream takes the fix.

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

### Documentation-only pushes skip the gate — and therefore the deploy

`gate.yml`'s push rule carries `path.exclude` for `*.md`, `docs/*.md` and
`docs/**/*.md`. A push to `main` that touches nothing else runs no pipeline,
so `beekeeper-autodeploy` — which deploys the newest **green pipeline** — has
nothing new to select and the relay is not rebuilt. Prose used to cost about
fifteen minutes of relay build; on 2026-08-23, two of four pushes to `main`
were pure documentation.

Two things make this safe, and both were verified rather than assumed:

- **A code commit cannot hide behind a docs commit at the tip.** Woodpecker's
  own documentation says push path filters consider only the most recent
  commit, which would be disqualifying here — multi-commit pushes are normal.
  This instance does not behave that way: pipeline 324 recorded `changed_files`
  spanning all six commits of a push whose tip was documentation only.
- **Only markdown is inert, and only outside `crates/`.** `docs/**` is *not*
  excluded, because `docs/nips/NIP-MP.fixtures.json` is `include_str!`'d by
  `crates/buzz-sdk/src/builders.rs`; `**/*.md` is *not* excluded, because
  `crates/buzz-acp/src/base_prompt.md` is `include_str!`'d into `BASE_PROMPT`.
  Either would skip a rebuild for a change to compiled output.

`scripts/test-woodpecker-path-filter.sh` enforces this: it expands the
exclusion list over the tracked tree, resolves every `include_str!` /
`include_bytes!` argument in `crates/` against it, and rejects any excluded
path that is a relay build input. It runs in the `deploy-scripts` step and in
`just check`. An exclusion pattern whose shape it cannot expand is fatal, not
ignored — an unexamined pattern is how a rebuild goes missing quietly.

The consequence to expect when reading deployment state: **`BUZZ_IMAGE` may
legitimately trail `origin/main`.** See `deploy/autodeploy/README.md` § Verify.
Pull requests are not filtered.

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
