# Working in agiterra/beekeeper

This repo is **Beekeeper**, agiterra's fork of
[block/buzz](https://github.com/block/buzz). It is a single-branch repo: `main`
is the product, and upstream is merged in occasionally.

## Remotes

| Remote | Repo | What it is |
|---|---|---|
| `origin` | `hive.agiterra.org/git/<owner>/agiterra-beekeeper` | The relay's **own** git hosting — Beekeeper serving its own source. **`main` tracks `origin/main`**, and this is where you push. Needs Nostr credentials; run `just install-git-credentials`. |
| `upstream` | [agiterra/beekeeper](https://github.com/agiterra/beekeeper) | GitHub. Still what **Woodpecker watches**, so it is what CI and the relay deploy from — kept in step automatically by the forge's mirror bridge (see § The relay is canonical). Fetch-only from clones; never push here. |
| `vanilla` | [agiterra/buzz](https://github.com/agiterra/buzz) | The block/buzz mirror, plus the one CI patch that runs it on ci.agiterra.org. Upstream work is merged or cherry-picked from here. |

This is not the arrangement described before 2026-08-24, when `origin` was the
GitHub repo and there was no `upstream`. Both names moved at once, so anything
you remember about which is which is probably stale — `git remote -v` is the
only reliable answer.

### One push, one destination (since 2026-08-26)

Push to `origin` (the relay) only. The forge mirrors every ref to GitHub
within seconds — `hive-mirror-bridge.service` subscribes to the relay's
kind:30618 ref-state events and runs the sync on each one, with the hourly
`git-mirror.timer` as reconcile fallback. GitHub then triggers Woodpecker as
before. Setup per clone:

```sh
git branch -u origin/main main
git remote add upstream https://github.com/agiterra/beekeeper.git   # fetch-only
```

A clone still carrying the pre-bridge dual push URLs should drop them —
pushing to GitHub directly now only creates races against the bridge:

```sh
git config --unset-all remote.origin.pushurl
```

Divergence check (fetch first, or you are comparing frozen refs):
`git fetch upstream && git rev-parse origin/main upstream/main`. The bridge
logs on the forge: `journalctl -u hive-mirror-bridge`.

Why the relay is the fetch side: it is measurably faster from here (~190 ms
against GitHub's ~400 ms), and the pre-push guards resolve their base from
`main@{upstream}`, so they run on every push. Both were verified against the
relay base after the switch.

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

### Pushing to the relay

The relay authenticates git with **NIP-98** — a signed kind:27235 event
delivered through git's `authtype` credential capability, which needs git
≥ 2.46. An unauthenticated request gets:

```
HTTP/2 401
www-authenticate: Nostr realm="buzz", method="GET"
```

`git-credential-nostr` answers that challenge. It is bundled inside the desktop
app as a Tauri sidecar, which is why the app can push and a terminal cannot —
the app configures it per-subprocess with `GIT_CONFIG_GLOBAL=/dev/null` and the
key in the child's environment, so nothing persists. One command sets a
terminal up:

```sh
just install-git-credentials
```

That installs the helper to `$HOME/.local/bin` (**not** a bare `cargo install`
— hermit pins `CARGO_HOME` inside the repo, so the binary would land in
`.hermit/rust/bin`, off `PATH`, and a hermit clean would delete it) and writes
three git config entries, **scoped to the relay's `/git` path** so `osxkeychain`
keeps serving GitHub:

```
credential.https://hive.agiterra.org/git.helper       ""      <- resets the list
credential.https://hive.agiterra.org/git.helper       nostr
credential.https://hive.agiterra.org/git.useHttpPath  true
nostr.keyfile                                         ~/.nostr/key
```

The empty first value is load-bearing. A scoped helper is **appended** to
whatever the system and global configs already named, and on macOS
`/opt/homebrew/etc/gitconfig` sets `credential.helper = osxkeychain`. Without
the reset, both helpers run: nostr answers correctly, then osxkeychain tries to
`store` an ephemeral credential and every *successful* relay operation prints
`fatal: failed to store: -1`. A "fatal" over a request that worked sends you
hunting a failure that never happened.

It writes **no key material**. The key file is yours to create, at mode 0600,
holding the nsec of the identity the relay knows — for
`agiterra-beekeeper` that is the kind:30617 author, i.e. the **release**
desktop identity. The debug build uses a different keyring service
(`beekeeper-desktop-dev`) and therefore a different key, which is not the repo
owner and would be denied.

Two commands report on it, and the split is deliberate:

- **`bee git status`** — local only. Says whether the three pieces are in
  place, and distinguishes *configured* from *configured and the helper still
  exists*: a config naming a helper that has been moved or deleted reads as set
  up and fails only at push time. It reports `configured`, never `ready` —
  nothing local can know whether the relay accepts the key.
- **`bee git check`** — asks the relay, over the transport git uses. It makes a
  real `GET <repo>/info/refs?service=git-upload-pack` (add `--push` for
  `git-receive-pack`, the request `git push` makes first), signed by
  `git-credential-nostr`'s own key resolution, attestation reader and signing
  function (`crates/git-credential-nostr/src/lib.rs`: `resolve_key`,
  `resolve_auth_tag`, `repo_root_url`, `authorization_header`), so it cannot
  drift from what git sends. It reports the key it used and where it came from,
  the attestation state (present / absent / invalid), and the transport verdict:
  **accepted**, or **denied** with the relay's own non-explanation. Exit 0 when
  the transport accepts, 3 when it denies — the code matches what git will do.

Which repository it probes: a remote in the current checkout that points at the
relay, when there is one — that is the repository git would actually contact.
Otherwise it probes a repo path that **cannot exist**, which isolates the
authorization gate: the relay checks NIP-98 and membership in the request
extractor, before resolving the repository, so 403 means the key was refused at
the gate and 404 means it got through and only the repo was missing. On a *real*
repository a 404 is not acceptance — the relay answers 404 for both a missing
repo and a denied read, on purpose, so membership is not probeable — and the
check says so instead of choosing one.

Two gates, not one. The relay's HTTP membership path (`POST /query` and the rest
of the JSON surface, `api/mod.rs` `enforce_relay_membership`) is a different code
path from the git transport's, and they can disagree: on 2026-08-29 a seat's
`bee git check` failed with `relay_membership_required` while `git push` from the
same key succeeded seconds later. The HTTP answer is now printed as a secondary
line, labelled `relay HTTP membership:`, and never decides the exit code.

**No remedy ever tells you to unset `BUZZ_AUTH_TAG`.** That was the old advice on
a 403, and following it would have made the seat drop the owner attestation its
push depends on. A denial with an attestation present says to ask the operator to
confirm the seat's *owner* is a relay member, and to keep the attestation.

#### Seats push on their owner's grant

A hired seat signs git as **itself**, never as its operator — the ACP harness
injects `NOSTR_PRIVATE_KEY` into the managed subprocess, and
`git-credential-nostr` prefers it over `nostr.keyfile`
(`crates/git-credential-nostr/src/lib.rs`, `resolve_key`/`choose_key`, which
`bee git status` and `bee git check` call rather than reimplement). That fence is the point:
a seat must never be able to commit or push as the human. But a seat's own key
holds no membership and no roster row, so on its own it can read nothing.

What carries it is the **NIP-OA owner attestation** the harness also injects as
`BUZZ_AUTH_TAG` — `["auth", <owner>, <conditions>, <sig>]`, signed by the owner
over the agent key. Git's credential protocol can return an `Authorization`
value but cannot add a separate header, so the helper attaches the tag to the
**signed NIP-98 event itself**, where the relay reads it
(`crates/buzz-relay/src/api/git/transport.rs`, the `GitAuth` extractor). The
relay then admits the seat exactly as the rest of its HTTP surface does:

- the read gate resolves a grant for the signing key **and** for the verified
  owner, taking either (`authorize_git_read`);
- the pre-receive policy endpoint resolves the pusher's role for both
  principals and takes the more permissive, and a seat of the repo owner
  carries owner authority (`api/git/policy.rs`).

This is inheritance, not a bypass. An owner with no grant admits nobody,
removing the owner revokes every seat attested to them in the same request, and
every denial is the same generic 404 as a nonexistent repo, so nothing about
membership becomes probeable.

Two identities in one shell is the trap. `bee git status` and `bee git check`
both report the key git **will** present, resolved by the helper's own
precedence, and name where it came from (`key_source`:
`NOSTR_PRIVATE_KEY` or the key file path). When both sources hold keys and they
are different identities, both commands print one sentence saying which one git
signs with and which one is not used — reporting only the key file named the
operator's identity in a shell where git signs as the seat.

When git rejects a credential it calls the helper with `erase`; the helper
answers with one line on stderr naming the refused key and pointing at
`bee git check`. It never states a reason: the relay answers every git denial
identically, so a reason printed there would be invented. A denial that never
re-invokes the helper — a read-gate 404, which git reports as a missing
repository — still prints nothing, so `bee git check` remains the way to ask.

**A pasted public key is the failure mode to watch for.** `Keys::parse` accepts
any 64 hex characters as a *secret* key, so pasting a pubkey hex into the key
file yields a valid-looking, entirely different identity, and every local check
reports success. The bech32 `npub1…` form is rejected outright; the hex form is
not decidable locally, which is the reason `bee git check` exists.

Inside the desktop app the same setup is offered after an import or link wires
up a Beekeeper remote. It is an offer, never automatic: accepting writes the
identity key to disk, and the prompt names the file first.

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
`/srv/git`, Woodpecker at `ci.agiterra.org`).

**The relay is canonical (since 2026-08-26).** For beekeeper the old
GitHub-pull mirror is inverted: `/srv/git/beekeeper.git` fetches from the
relay and pushes to GitHub, which exists as CI trigger and backup. The moving
parts, all deployed by `scripts/forge/setup-hive-mirror.sh` (idempotent,
re-run it after changing any of them):

- **`buzz-mirror-bridge`** (`crates/buzz-mirror-bridge`, installed to
  `/usr/local/bin`, run as `hive-mirror-bridge.service`) subscribes to the
  relay's relay-signed **kind:30618** NIP-34 ref-state events — published on
  every ref-changing push, replaceable, so the bridge simply resyncs per event
  and on every reconnect — and runs the mirror sync within seconds of a push.
- **`git-mirror-update`** (`scripts/forge/git-mirror-update`) does the sync.
  Direction is per-repo git config, never a hard-coded remote name:
  `mirror.fetchRemote` (the relay) and `mirror.pushRemote` (GitHub) flip a
  repo to canonical→local→backup; repos without them (buzz, cairn, portage…)
  keep the old GitHub-pull behavior. The hourly `git-mirror.timer` runs the
  same script as the reconcile fallback, so a dead bridge costs latency, not
  commits. Pushes use forced refspecs but **no prune** — GitHub-only refs are
  left alone.
- The forge's identities: an SSH deploy key with **write** access for the
  GitHub push, and a Nostr key (`/home/git/.nostr/key`, member of the
  community and of the repo's bound channel) for the NIP-98 fetch — the git
  read gate 404s repos to non-members, and `git-credential-nostr` (git 2.46+
  required, PPA-installed on the forge) signs the fetches.

Woodpecker still watches GitHub, and that constraint is unchanged: it needs a
*forge* — OAuth login, repo/branch API, webhook delivery, commit statuses —
and the relay's git hosting is a bespoke NIP-98-authed smart-HTTP transport
whose only routes are `info/refs`, `git-upload-pack` and `git-receive-pack`.
The bridge is what reconciles "the relay is canonical" with "CI clones from
GitHub".

Everything *downstream* of the forge is already GitHub-free and stays that way:
`autodeploy` reads Woodpecker's sqlite and `git archive`s the local bare mirror,
so it never talks to GitHub at all.

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

The relay deploys itself. **One deployer script,
`/usr/local/sbin/autodeploy`, serves two relays** — the config is what differs,
so read the unit name before trusting any path below:

| Unit | Config | Repo | Branch | Instance |
|---|---|---|---|---|
| `beekeeper-autodeploy.timer` | `/etc/default/beekeeper-autodeploy` | `agiterra/beekeeper` (Woodpecker `repo_id=2`) | `main` | `hive` → `/opt/beekeeper` |
| `buzz-autodeploy.timer` | `/etc/default/buzz-autodeploy` | `agiterra/buzz` (`repo_id=1`) | `integrated` | `lightyear` → `/opt/buzz` |

`REPO_ID` is the load-bearing line in each: both repos push `main`, so an
unpinned query returns whichever pushed most recently, and deploying the wrong
one would come up *healthy*.

Every 5 minutes it polls Woodpecker's sqlite, and when the newest push pipeline
for its repo and branch is green it exports the source from the forge git
mirror at that commit (`git archive`, never a network clone), builds
`<image>:<short-sha>` inside the relay instance, takes a `pg_dump` backup
(`$BASE/backup-pre-*.sql.gz`), flips `BUZZ_IMAGE` in `$BASE/compose/.env`, and
restarts with compose health-wait. An unhealthy relay rolls back to the
previous image automatically. Red pipelines never deploy; a failed attempt
leaves `$BASE/autodeploy-failed-<short-sha>` in the instance so it will not
rebuild in a loop (remove the marker to retry).

Paper trail: `journalctl -u beekeeper-autodeploy` on the host, `$BASE/deploy.log`
and `$BASE/build-<short-sha>.log` in the instance. CI has no credentials for (or
access to) the prod instance; the deployer only reads Woodpecker's status DB and
the read-only mirror.

Because the mirror is fetched from GitHub on an hourly timer, and the deployer
selects on a *Woodpecker pipeline*, a commit that reaches only the relay is
invisible to both. That is the practical reason a relay push must be
accompanied by a GitHub push until a relay→mirror bridge exists.

`build/*` tags remain the pins for reproducing or manually rolling to a known
build. Desktop dev runs `just desktop-standalone`.

Running a daily-driver Beekeeper.app and a dev instance side by side on macOS
(distinct icons, no repeated keychain prompts):
[local-desktop-instances.md](local-desktop-instances.md).
