# Working in agiterra/beekeeper

This repo is **Beekeeper**. It began as a fork of
[block/buzz](https://github.com/block/buzz) — which is why the crates are still
named `buzz-*` — and is now its own product: nothing is merged in from that
repository and nothing is pushed to it. It is a single-branch repo, and `main`
is the product.

## Remotes

| Remote | Repo | What it is |
|---|---|---|
| `origin` | `hive.agiterra.org/git/<owner>/agiterra-beekeeper` | The relay's **own** git hosting — Beekeeper serving its own source. **`main` tracks `origin/main`**, and this is where you push. Needs Nostr credentials; run `just install-git-credentials`. |
| `upstream` | [agiterra/beekeeper](https://github.com/agiterra/beekeeper) | GitHub. Still what **Woodpecker watches**, so it is what CI and the relay deploy from — kept in step automatically by the forge's mirror bridge (see § The relay is canonical). Fetch-only from clones; never push here. |

Two remotes, both agiterra's. This is not the arrangement described before
2026-08-24, when `origin` was the GitHub repo and there was no `upstream`; both
names moved at once, so anything you remember about which is which is probably
stale — `git remote -v` is the only reliable answer. Beware the word in older
documents and in the ledger, too: there, "upstream" means `block/buzz`.

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

There is deliberately **no `block/buzz` remote and no vanilla mirror**. Both
were removed on 2026-10-03 along with the last of the fork process: `blockbuzz`
cost 1,167 remote-tracking refs and `vanilla` 2, for history nothing here reads
any more. Adding either back is one `git remote add`, but there is no workflow
that wants one — see the note on the fork at the top of this file.

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

#### Landing a batch: the push, step by step

Written 2026-09-01 after the third time a landing stalled on the same two
facts. Both are recorded elsewhere in this file and in the ledger; this is
the sequence, in order, so nobody re-derives it.

1. **Prove the SHA first.** `just ci` (and `just test` if relay/db/auth
   changed) on the exact commit you will push, in a worktree. Note the SHA.
2. **Confirm the remotes and the base.** `git remote -v`; `git fetch origin
   main`; your branch must sit on top of `origin/main` (rebase with
   `--signoff` if not — the pre-push branch-skew guard blocks a topic branch
   that is behind main and touches the same files).
3. **Push with `just push` (`scripts/push-with-floor.sh`), on that same SHA.**

   ```sh
   GIT_TERMINAL_PROMPT=0 just push origin <branch>:main
   ```

   Why not a plain `git push`: git mints the NIP-98 credential at ref
   discovery, *before* the pre-push hooks run, and reuses that one credential
   for the whole push — confirmed empirically against a throwaway HTTP git
   server (`plans/archive/2026-09-20-pre-push-floor-stamp.md`): the credential
   helper's `get` fires exactly once per `git push`, and the identical
   Authorization value is replayed on the retried GET and the receive-pack
   POST. The pre-push floor (clippy, typecheck, the changed crates' unit
   tests) can outlast the relay's ±900s token window on a crate-touching
   push, so the upload then arrives with an expired token and fails
   `HTTP 401` with every hook green (ledger 178(n)). `just push` runs the
   exact same floor *before* opening the connection and records a short-lived
   pass stamp keyed to the tip sha, so the pre-push hook that `git push`
   triggers finds a fresh stamp and returns immediately — the credential is
   seconds old when the pack uploads. A plain `git push` still runs the floor
   in full inside the hook and may still 401 on a long one; its summary names
   `just push` when that happens. Never fall back to `--no-verify` on a SHA
   `just ci` did not pass. `GIT_TERMINAL_PROMPT=0` makes a credential problem
   fail in a second instead of waiting on a prompt nobody can answer.
4. **Verify both heads.** `git fetch origin main && git rev-parse origin/main`
   must be your SHA; a minute later `git fetch upstream main` shows the
   GitHub mirror following.
5. **Expect the relay to redeploy.** The autodeploy timer on agincus builds
   the newest CI-green `main` and restarts the relay whenever the commit
   differs from the running image — it has no path filter, so any push that
   passes CI flips the relay, even one that touches no relay crate. Budget
   CI time plus a five-minute timer plus the build; do not start a live
   run that must survive a WebSocket drop inside that window. Docs-only
   pushes skip both the gate and the deploy.
6. **Then land the checkout you run from.** With the dev app stopped,
   `git merge --ff-only origin/main` in the main checkout, rebuild
   `bee` (`cargo build -p beekeeper-cli`), and relaunch with the keyring enabled
   (`env -u BUZZ_DESKTOP_NOKEYRING just desktop-standalone`). Never merge,
   rebase, or switch in that checkout while the app runs.

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
(`crates/beekeeper-relay/src/api/git/transport.rs`, the `GitAuth` extractor). The
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

Every branch here is rebased. There is no longer any exception: the one that
existed was upstream history, and this repo no longer tracks any.

This repo previously carried an upstreamable-feature-branch model
(`feature/*` rebased onto a `main` mirror, reassembled through
`integration/glue` into a force-pushed `integrated`). That is gone. Upstream
received far too many submissions for ours to land in useful time, so the
branches were being maintained for a merge that was never going to happen —
and two of them had already stopped building standalone. That was the first step
of the separation finished on 2026-10-03.

Those branches were retired locally on 2026-08-22, along with the pre-rebrand
`feature/*` lineage whose content is already on `main` under different SHAs.
Nothing was thrown away: each survives as a local-only `archive/<branch>` tag
(`git tag -l 'archive/*'`), which keeps the commits reachable without putting
them back in `git branch`. They are not pushed.

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
  `crates/beekeeper-sdk/src/builders.rs`; `**/*.md` is *not* excluded, because
  `crates/beekeeper-acp/src/base_prompt.md` is `include_str!`'d into `BASE_PROMPT`.
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
Postgres/Redis/RustFS as services, and migrates a **fresh** database — so a
local run can go green and still land red. Run the two extra steps by hand
before a push you intend to deploy, and remember that a fresh-database
`cargo run -p beekeeper-admin -- migrate` exercises migration ordering that an
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

- **`buzz-mirror-bridge`** (`crates/beekeeper-mirror-bridge`, installed to
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
- The forge's identities: the GitHub App `agiterra-hive-mirror` (Contents
  and Workflows read/write, installed on each hive-mirrored repo: this one
  and `tankloop`) for the GitHub push,
  and a Nostr key (`/home/git/.nostr/key`, member of the
  community and of the repo's bound channel) for the NIP-98 fetch — the git
  read gate 404s repos to non-members, and `git-credential-nostr` (git 2.46+
  required, PPA-installed on the forge) signs the fetches.
  `git-credential-github-app` (`scripts/forge/`) mints the app's installation
  token per push. It replaced a deploy key on 2026-10-05: GitHub names a
  deploy-key push's sender as whoever added the key, and Woodpecker labels
  each pipeline with that sender, so every bridged push — anyone's — read as
  `andy-agiterra`. Pipelines now read `agiterra-hive-mirror[bot]`; the
  pusher's own name never reaches Woodpecker, since GitHub has no way to know
  it.

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

## Seeding a sandbox — `sandbox.yml`

A fresh sandbox — a seat's worktree, an action step's detached tree, or one you
cut by hand — arrives with its tracked files and nothing else. The code takes
seconds; the build state does not. Measured on one developer's machine: `target/`
36 GB, a *second* cargo workspace under `desktop/src-tauri/target` 19 GB, a
1.3 GB `CARGO_HOME` at `.hermit/rust`, and `node_modules` in four places. Nothing
seeded any of it, so every sandbox re-downloaded the registry and rebuilt from
cold, and fourteen worktrees accumulated 331 GB of duplicates.

`sandbox.yml` at the repository root is how the project says what to seed and
how. One declaration, one parser (`beekeeper_core::sandbox_manifest`), three
consumers: the desktop's worktree creation, the session provider's action steps,
and `just sandbox-seed`. The same declaration also tells
`bee sessions worktree reclaim` what counts as build state, so the set cannot
differ between seeding and freeing.

```bash
just sandbox-plan                                  # what this project declares
just sandbox-seed ../beekeeper-wt-mine             # print the plan, write nothing
just sandbox-seed ../beekeeper-wt-mine --confirm   # seed it
just sandbox-seed ../beekeeper-wt-mine --run-recipes --confirm
```

Five entry kinds, chosen per entry because no single mechanism is right for all
of them:

| Kind | What it does | Used here for |
|------|--------------|---------------|
| `clone` | Copy-on-write clone (`cp -Rc`) | the two `target/` dirs, the four `node_modules` |
| `copy` | A real, independent copy | `.env` |
| `symlink` | A link into the source checkout | — |
| `share` | A link into one pool per project | `.hermit/rust`, i.e. `CARGO_HOME` |
| `run` | The project's own recipe or script | the sidecar stubs, the mobile overrides |

Measured on this repository, seeding a freshly cut worktree: **103 GB** of
logical build directories, **419 MiB** of real disk, 85 s wall clock, and
`git status --porcelain` empty afterwards. The first `cargo build` in it took
20 s against 1298 crates reached through the shared registry, with no network.

Five things that will mislead you:

- **`du` is not what a clone costs.** A cloned `target/` measures its full
  logical size while its blocks are shared with the source until one side
  writes. Every receipt says `up to N GB — shared with the source, so the
  exclusive share is unknown`, and the only measurement that proves clone-on-write
  is the free-space delta. A size nobody paid to measure reads `unknown`, never
  `0`.
- **An ignore rule written with a trailing slash hides a directory, not a
  link.** `.gitignore` says `node_modules/`, so a *symlink* there is staged and
  the sandbox's `git status` is never empty — which is what a seat's push gate
  reads. The seeder refuses such an entry and names `link: entries`, whose
  destination is a real directory. It does **not** write to `info/exclude`:
  that file is shared by the main checkout and every linked worktree.
- **A sandbox is seeded before these paths exist.** `git check-ignore target`
  does not match `/target/` for a path that is not there, because git evaluates
  the query as a non-directory. The question has to be asked as `target/`. The
  one implementation of this rule is
  `beekeeper_core::sandbox_seed_fs::ignored_from_patterns`.
- **`CARGO_HOME` cannot be redirected by an environment variable here.**
  Hermit's rustup package pins `CARGO_HOME="${HERMIT_ENV}/.hermit/rust"` in its
  own package environment and `hermit exec` applies that over the ambient value,
  so for every `cargo` reached through a `bin/` shim the exported value does
  nothing. What redirects it is the **path**: a sandbox's `.hermit/rust` is a
  link into `project_scope_pool_dir`, inside the directory the project's
  executions are already granted. That is why the entry is `share` and not an
  env var.
- **A shared `target/` serializes your lanes.** CI gets away with one shared
  target directory only because `WOODPECKER_MAX_WORKFLOWS=1` runs one workflow
  at a time (see the cache-mount table above). Local sandboxes build in parallel
  and would block each other on cargo's build lock, so `clone` is the mechanism
  for a target directory and `share` on one emits a warning.

And one rule that is not obvious: **`node_modules` is cloned and re-rooted,
never linked.** pnpm keys its workspace state by absolute path, so a tree
pointing at another checkout's `node_modules` reads as a stale install on every
script and purges the content-addressed store every concurrent sandbox resolves
through. The seeder rewrites `.pnpm-workspace-state-v1.json` onto the new tree,
and a rewrite that matches nothing is a **failure** that undoes the entry rather
than a silent no-op.

Seeding never fails the tree it was asked to seed. An unseeded sandbox is
*cold*, which is slow; a sandbox that is not the commit it claims is *wrong*.
And `sandbox.yml` is a tracked file an agent may edit, so letting it fail a hire
would hand whoever can edit it a lever to fail every hire. Every skipped,
refused or failed entry is instead named in the receipt, which the desktop
returns from the create, stores in `coding-session-workdirs.json`, and an action
step leaves at `<artifact dir>/seed.json`.

## Deploying

The relay deploys itself. **One deployer script,
`/usr/local/sbin/autodeploy`**, driven by a per-relay config:

| Unit | Config | Repo | Branch | Instance |
|---|---|---|---|---|
| `beekeeper-autodeploy.timer` | `/etc/default/beekeeper-autodeploy` | `agiterra/beekeeper` (Woodpecker `repo_id=2`) | `main` | `hive` → `/opt/beekeeper` |

A second relay used to deploy the vanilla mirror (`buzz-autodeploy`,
`agiterra/buzz` / `repo_id=1`, the `buzz` instance behind
lightyear.agiterra.org). It was retired on 2026-09-29 and the mirror itself on
2026-10-03.

`REPO_ID` is still the load-bearing line even so: Woodpecker's pipelines table
keeps repo 1's `main` history, so an unpinned query could still select one of
those builds, and deploying it would come up *healthy*.

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

## NIP-11

*Added 2026-09-03 (batch 3 lane L24, finding 32).*

Before finding 32, the relay's NIP-11 document advertised only `version:
0.2.1` — the crate version, which moves once a release, not the build. There
was no mechanism to answer "did that push actually redeploy hive", short of
running a command that needs the new code and watching whether it works (see
§ Deploying and `deploy/autodeploy/README.md` § "A trailing `BUZZ_IMAGE` is
now sometimes correct" for why a trailing image is not, on its own, evidence
of anything). Two additive NIP-11 fields fix that:

- **`software_commit`** — the full 40-hex git commit this binary was built
  from, or the literal `unknown`. `unknown` is a legal, disclosed value, not
  an error: a relay predating this field, or a build environment that could
  not determine its own commit, both answer honestly rather than lying with a
  guess.
- **`build_time`** — an RFC 3339 UTC timestamp taken when the binary was
  compiled, second precision (e.g. `2026-09-03T02:51:29Z`), or `unknown`.
- **`software_commit_count`** *(added 2026-09-04)* — `git rev-list --count`
  of that same `software_commit`: the number of commits reachable from it,
  inclusive. A JSON number, or `null` when the build could not determine one.
  `null` is the disclosed non-answer `unknown` is for the other two — always
  present in the document, never omitted, never guessed, and never `0`
  (`rev-list --count` of a real commit is at least 1, so a `0` could only
  come from a broken pipeline, and unlike `null` it would silently take part
  in a consumer's subtraction).

  **A count is a set size, not a position.** `count(B) − count(A)` is the
  number of commits B has that A does not *only when A is an ancestor of B*;
  on divergent branches it is the difference of two unrelated quantities. Two
  counts are comparable only when they come from the same linear history, and
  a consumer that renders a difference must disclose which method it used —
  see `bee git check`'s `EnforcementCheckMethod`
  (`crates/beekeeper-cli/src/commands/git_setup.rs`) for the shape that answer
  should take.

  **Shallow checkouts disclose `null`, never a number.** In a
  `fetch-depth: 1` clone `rev-list --count` returns the size of the graft
  rather than the ordinal, so a client subtracting it would announce a drift
  of the entire history. `build.rs` refuses to count a shallow checkout.

  The commit and its count are resolved **together** (`resolve_stamp` in
  `crates/beekeeper-relay/build.rs`), never independently: a count read from a
  local checkout while the SHA came from `BUZZ_SOURCE_SHA` would advertise a
  commit from one history and an ordinal from another, which no consumer
  could detect.

Resolution (`crates/beekeeper-relay/build.rs` / `src/build_provenance.rs`, and see
`src/build_info.rs` for the two compile-time env vars it produces):

1. `git rev-parse HEAD` in the crate's own checkout, when `.git` is present —
   a native `cargo build`.
2. `BUZZ_SOURCE_SHA`, the build-arg `Dockerfile` already declares (`ARG`
   default `unknown`, then `ENV`) and the image-build path threads through:
   `deploy/autodeploy/autodeploy` passes the full `$sha` it already selects
   from Woodpecker, for hive. (The inherited `.github/workflows/docker.yml`
   lane that published a relay image to Block's registry was removed on
   2026-10-06.) This is the
   case `git` cannot answer on its own: the relay's `.dockerignore` excludes
   `.git/`, and `deploy/autodeploy/autodeploy` builds from a `git archive`
   export, which never had one — without the build-arg, every relay built
   this way would silently disclose `unknown` forever, which is exactly what
   was happening before this lane added the `--build-arg` to the deployer.
3. `unknown`.

`build_time` is always the compiling machine's own clock at `cargo build`
time — it cannot fail the way a git lookup can, so it is only ever `unknown`
in the pathological case of a clock read failing entirely.

**`GET /health`**'s body carries the same disclosure in the plain-text
liveness check: `ok <sha8>`, where `<sha8>` is the first 8 hex characters of
`software_commit` (or `unknown`, unchanged in length by truncation since it's
already 7 characters). `ok` stays the first token so any existing probe that
matches/prefixes on it keeps working — this repo's own compose healthcheck
greps `"200 OK"` on `/_readiness` instead (see § Deploying), but a third party
could reasonably have grepped `/health`'s body directly. `/_status` (a
separate, pre-existing endpoint) continues to carry the fuller
`source_sha`/`id`/`url` build object as JSON.

**`GET /health/system`** is the one health path that is not a probe: the
relay's own machine — CPU, memory and disk — for the people who run it. It
is NIP-98 authenticated like `POST /query`, bound to the request host's
community, and answered only for an `owner` or `admin` on the roster (or
anyone, on an open relay with no steward yet — the kind:9033 rule); everyone
else gets 403. The body is the latest sample from a ten-second sampler plus
`age_seconds`, so a reader can tell a live figure from a stale one; before
the first sample it answers 503 rather than a guess. The figures are what
the relay *process* can see: in a container the CPU and memory totals are
the host's unless a cgroup limit applies (then a `memory.container` block
names the limit), the load average is always the host's, and `disks` are
`statvfs` of the git data path and `/` — a database volume in another
container is not measured. The desktop's Dashboard "Relay machine" card is
the consumer (`crates/beekeeper-relay/src/system_health.rs`,
`desktop/src/features/dashboard/lib/relaySystemHealth.ts`).

**Reading it from the CLI:**

```bash
bee --format compact sessions whoami         # relay_commit beside relay_url
bee repos protect list --id agiterra-beekeeper   # relay_commit in the listing
bee repos protect set  --id agiterra-beekeeper --ref refs/heads/main --require-verdict  # relay_commit in the write response
```

`bee git check --ref <ref>` goes one step further: it answers whether the
relay actually serving a repository enforces `require-verdict`
(`42dd921d831c483e6e16111491b39947b4cf1f86` — see § Verdict-gated refs below),
not just whether *this build* would. It tries two methods, in order, and
always names which one answered:

1. **Ancestry** — `git merge-base --is-ancestor` against this checkout, when
   it holds both the require-verdict commit and the relay's disclosed
   `software_commit`. Exact, when available.
2. **Date** — the relay's disclosed `build_time` compared against the
   require-verdict commit's own committer time
   (`2026-09-03T02:51:29Z`), used only when ancestry could not answer — most
   commonly because the checkout is a shallow clone (the gate clones at
   `depth: 1`; see § CI) that does not hold the require-verdict commit even
   though the relay's own commit is newer.

A relay that predates finding 32 entirely (`software_commit` absent, not just
`unknown`) makes both methods answer "unknown" — never a guessed yes/no.

**Desktop:** the "Edit Community" dialog shows "Relay build: `<8-hex>`" (or
`unknown`) beneath the Relay URL field, read via the `get_relay_build_commit`
Tauri command — for the community being edited, not necessarily the active
one, so editing an inactive workspace still shows the truth about its own
relay rather than the currently-connected one's.

### `limitation.rate_limits`

*Added 2026-09-07 (relay admission split).*

The document also advertises the admission limits the relay enforces, so a
client can pace itself instead of learning the numbers from `rate-limited:`
refusals. The object is built from the live configuration and the relay's
per-kind constants (`RelayRateLimits::from_config`,
`crates/beekeeper-relay/src/nip11.rs`) — never retyped — so it cannot drift from
enforcement. Absent only from relays that predate the field.

```bash
curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/ \
  | jq .limitation.rate_limits
```

```json
{
  "window_secs": 5,
  "reads_per_connection": 150,
  "messages_per_connection": 50,
  "ephemeral_per_connection": 500,
  "messages_per_key_per_min": 60,
  "agent_messages_per_key_per_min": 120,
  "api_calls_per_key_per_min": 300,
  "max_connections_per_key": 8,
  "presence_per_key_per_sec": 5,
  "typing_per_key_per_sec": 5,
  "ephemeral_kind_per_key_per_sec": 10
}
```

- The three `*_per_connection` figures are **per WebSocket connection, per
  `window_secs`**, kept in relay memory: a REQ or COUNT costs one read
  however many filters it carries (up to NIP-11 `max_filters`); a stored
  EVENT costs one message; an ephemeral EVENT (kinds 20000–29999) costs one
  ephemeral. Two devices on one key each get their own set, and reads never
  touch Redis.
- `messages_per_key_per_min` is **shared across every connection a
  (community, pubkey) holds**, in Redis, and is charged by durable *and*
  ephemeral EVENTs — a streaming terminal spends the same pool as chat.
  `api_calls_per_key_per_min` is a separate pool for `POST /events`,
  `/query` and `/count`, which is why one-shot reads bundled into a single
  `/query` leave the WebSocket budget alone.
- `max_connections_per_key` is enforced after NIP-42 auth: the socket over
  the cap receives `NOTICE rate-limited: too many connections for this key`
  and is closed.
- The `*_per_key_per_sec` ceilings are one-second per-kind windows on
  generic ephemeral EVENTs, applied in the event handler — that is, *after*
  admission has already charged the frame to the per-connection ephemeral
  burst and to the shared per-key message quota. A refused sixth presence
  update in a second has still spent one of the key's 60 per minute.

A refusal always reads `rate-limited: {read|message|ephemeral|api} quota
exceeded; retry in {n}s` — on `CLOSED` for REQ/COUNT, on `OK false` for
EVENT, as HTTP 429 for the bridge — and the word names the pool that
tripped. When Redis cannot be reached only EVENTs are refused, with
`rate-limited: shared admission unavailable` (HTTP 503). The relay logs one
`admission quota exceeded` warn line per window per connection, and counts
every refusal in `buzz_admission_rejections_total{transport,reason,budget,scope}`.

## Verdict-gated refs

*Added 2026-09-02 (batch 3 lane L6).*

A repository can require that a mission ruled on a commit before it reaches a
branch:

```bash
bee repos protect set --id agiterra-beekeeper --ref refs/heads/main --require-verdict
bee repos protect list --id agiterra-beekeeper
```

With the rule set, the relay's pre-receive hook admits an update to that ref
only when a **founder-signed, canonical** kind:44244 `disposition` approves a
report whose `headSha` is the exact commit being pushed **and whose `branch` is
the branch being pushed**, in a mission founded by **a founder of the
repository** on the channel the repository is bound to — and only when **a
founder** is the one pushing. The reservation to the founders is the relay's
rule, not the mission's: no session policy is read. The relaxation that lets a seat land exists in the
code and is switched off until a verdict-gated push has been exercised live
once.

**Who founds a repository** (2026-09-03, finding 33): the announcement's
signer, every pubkey in its NIP-34 `["maintainers", …]` tag, and every Owner on
the project roster its `["project", …]` back-reference names. Before this the
gate keyed both questions to the signer alone, which is right for a
single-founder repository and wrong for `agiterra-beekeeper`: it is announced
by Andy and co-owned by Brian, so Brian's missions would have ruled on nothing
and Brian's landing pushes would have been refused.

```bash
# Add a co-founder to a repository you announced.
bee repos update --id agiterra-beekeeper --maintainer <hex64>
# Read the set back, with the sentence that says who may rewrite the rules.
bee repos get --id agiterra-beekeeper --owner <hex64>   # .founders, .founders_note
bee sessions explain founder
```

**Rules are still signer-only in v1.** The `buzz-protect` tags live on the
announcement, which is addressable at its author, so a co-founder's
`bee repos protect set` would create *their own* repository rather than edit
yours. Every surface that prints the founder set says this. Changing it needs a
founder-signed rule event the gate reads alongside the announcement — not
built.

**Nothing is governed until someone sets the rule.** It is opt-in and is
currently set on no repository, so today this section describes a capability,
not a running control.

What this changes for a person landing work:

- `git push` prints the relay's reason, one line per ref, e.g.
  `remote: refs/heads/main: require-verdict is set and no mission verdict names
  this commit: no approved report names <sha>. Searched 2 mission(s) — the 2
  newest mission(s) that seat 3d3b7169, the key this push authenticated as, of
  the newest 4 seats it holds — over one shared page of the newest 512 team
  transactions on their channels. An older ruling can fall outside both.`
- **The refusal names the lookup that ran** (finding 56, 2026-09-03). A seat's
  push is judged by the mission that seated it, not by the channel the
  repository is bound to — every coding session lives in its own channel, so
  the old bound-channel-only search could never find a real mission and said
  "Searched 0 mission(s)" for one that was sitting there, green. A pusher
  holding no seat falls back to the project's session channels, and a
  repository in no project falls back to its bound channel; the sentence says
  which of the three it was, and says "no seat **in the newest 512 authority
  transitions this relay could read**" rather than claiming a seat does not
  exist.
- A report that names only a branch does not admit anything — ask for a report
  carrying `headSha`.
- An approval is scoped to the branch its report named. A commit approved for
  `whoami/cli` does not land on `main`, and an approved-but-superseded commit
  cannot be pushed back over a branch later.
- Deleting a gated ref is refused for everyone, founders included: no report
  names the zero oid.
- The rule is enforced by the relay serving the repository. A relay that
  predates it ignores the token, so a repository is only as protected as the
  relay it lives on. `bee repos protect list` and `bee git check --ref` both
  answer from the *local* build and say so, and print what the serving relay
  reports being.

To ask what the hook would say *before* pushing:

```bash
bee git check --push --ref refs/heads/main            # about HEAD
bee git check --push --ref refs/heads/main --sha <oid>
```

The answer is printed under `Prediction, not a promise.` — the CLI runs the
relay's own admission rule over the fold it can read, but the hook decides at
push time on the commit actually sent, and the CLI reads the authority chain
off the wire rather than from the relay's accepted projection. It runs the same
three-step lookup and prints the sentence naming which step found the missions
(`prediction.lookup` in `--format compact`); its project step reads the
relay-signed kind:39000 channel metadata where the relay reads the `channels`
table, so it can miss a channel and predict a refusal that does not come — the
safe direction for a prediction.

### Seats inherit at most Member on a guarded ref

Also since 2026-09-02: a key that holds repository authority **only** by
attestation to someone else (a hired seat pushing on its operator's grant) is
capped at `member` **on a guarded ref**. Two refs are guarded:
`refs/heads/main`, and any ref an operator wrote a `buzz-protect` rule for.
There, an inherited grant can no longer force-push, delete a ref, or overwrite
a tag; it still creates and fast-forwards branches.

**Everywhere else the inherited grant is unchanged.** A seat rebases and
force-pushes its own `lane/*` or `wip/*` branch, and deletes it afterwards,
exactly as this document's § Landing a batch, `CLAUDE.md` and
`plans/archive/CREW_SESSIONS_PLAN.md` (agents repository) tell it to. An earlier draft
capped every ref and silently broke all three workflows.

A seat's *own* roster row is unaffected anywhere — a seat added to a channel as
`admin` is admin by that grant, not by inheritance. The cap is not a
per-repository opt-in: an opt-in would leave every repository that never set
one with an inherited **Owner** grant over its own trunk, which is how a hired
lead came to hold owner authority over this repository in live run 3.

**What the cap does not close.** Run 3's push was a *fast-forward*
(`1dd98e876..07c470be0`), and a fast-forward needs only `member` — which a
capped seat still holds. The cap closes the inherited-Owner escalation
(force-push, delete, tag overwrite). The rule that refuses run 3's push is
`require-verdict`, and it is opt-in and set on nothing yet.

Every ref update the hook decides is now logged by the relay with the
repository, the ref, both object ids, the authenticated pusher and the
decision. Before this, "who pushed `main`?" could only be answered from the
derived kind:30618 event.
