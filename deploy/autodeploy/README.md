# Autodeploy

Host-side deployers that watch Woodpecker and ship green builds to the relays
on agincus. They run on the **host**, not inside an instance, because they
drive `incus exec` against both `forge` (Woodpecker's sqlite, the git mirrors)
and the target instance.

**One script, configured per relay.** Everything relay-specific arrives through
the environment, from `/etc/default/<unit>` via the unit's `EnvironmentFile=`:

| | Woodpecker repo | Instance | Relay | Image |
| --- | --- | --- | --- | --- |
| `beekeeper-autodeploy` | id **2**, `agiterra/beekeeper` | `hive` | hive.agiterra.org | `beekeeper-relay:<short9>` |

It tracks branch `main`. Container names come from the compose project
(`buzz-prod-relay-1`, `buzz-prod-postgres-1`).

**Retired 2026-09-29:** `buzz-autodeploy`, which deployed vanilla
`agiterra/buzz` (Woodpecker repo 1) to the `buzz` instance behind
lightyear.agiterra.org. The instance, its Caddy site, its forge mirror and its
units are gone. Woodpecker's `pipelines` table still holds repo 1's history on
branch `main`, which is why `REPO_ID` stays mandatory (see the hazards below).

This started as two near-identical scripts, one of them untracked and
root-owned on the host. That invisibility is the direct cause of every hazard
below: each was found by reading the tracked copy, and each still existed in
the copy nobody could see.

## Four hazards, all found the hard way

**`REPO_ID` is not optional.** Woodpecker served two repos that both use branch
`main`, and its `pipelines` table still holds both. A query filtered on branch alone returns whichever repo pushed most
recently. An unpinned deployer will build Beekeeper and deploy it onto the
vanilla relay — and the relay comes up **healthy**, because a relay is a relay.
Wrong product, wrong schema, no alarm. Caught live on 2026-08-22 with about
five minutes to spare: run side by side, the pinned query returned the vanilla
commit while the unpinned one returned a Beekeeper commit mid-build.

**Deleting a branch disarms nothing.** The `pipelines` table keeps history, so
a deployer pointed at a long-dead branch keeps selecting that branch's last
build indefinitely. It stays silent only while that happens to match what is
deployed; change the image and it wakes up and deploys the ghost. This is
exactly what happened to lightyear during the vanilla rebuild — the health
check and rollback were the only things that caught it.

**A mirror root cannot read is not a mirror that is behind.** `incus exec` runs
as root; mirrors are owned by `git`. Git refuses with "detected dubious
ownership" unless the repo is in root's `safe.directory`, and on agincus that
list held exactly one entry — `/srv/git/buzz.git`, added by hand when the first
deployer was built. The failure is nasty because `git cat-file -e` exits
non-zero for an unreadable repo and for a genuinely missing commit alike, so
the deployer logged *"commit not in mirror yet"* and exited 0, forever. Fixed
two ways: `mirror_git` carries `-c safe.directory=$MIRROR` so it does not
depend on host config, and the script proves the mirror is *readable* before
asking what is in it, treating unreadability as fatal.

**Retention is a credential concern before a disk one.** Every run leaves a
`.env.bak-pre-<sha>`, each a full copy of the relay private key. By 2026-08-22
one host had 34 of those, plus 38 SQL dumps and 51 images. Pruning now runs
after a *successful* deploy only — a failed run leaves everything in place for
diagnosis — and never removes the image the relay is currently running.
It is a disk concern as well: removing images frees none of the BuildKit
cache, which had grown to ~94 GB on hive and ~197 GB on the buzz instance by
2026-09-29 and filled the agincus pool to 90%. Retention now also caps the
cache with `docker builder prune --keep-storage $KEEP_BUILD_CACHE` (default
`10GB`, so the next build stays incremental).

## It edits the live `.env` in two places

Besides flipping `BUZZ_IMAGE`, every deploy rewrites pre-rename tracing
targets in `RUST_LOG` and `BUZZ_OTEL_FILTER`: `buzz_relay` becomes
`beekeeper_relay`, and so on for every crate renamed in ledger 354. Without it,
an old `.env` filters for crate names that no longer exist, and the relay logs
only `tower_http`. Both edits happen in the one command that first copies
`.env` to `.env.bak-pre-<sha>`, so a rollback restores them together. Only the
two filter variables are touched; every `BUZZ_*` name, and every other line,
is left byte for byte.

This runs only from the **installed** copy, so reinstall the script (below)
before the first deploy of the renamed crates. Until then nothing is lost:
the binaries themselves read old targets as new ones and say so on stderr
(`beekeeper_core::log_targets`). The file fix just stops that warning.

## Build-arg names after the `BUZZ_*` → `BEEKEEPER_*` rename

The repository copy passes `--build-arg BEEKEEPER_SOURCE_SHA` and
`BEEKEEPER_SOURCE_COMMIT_COUNT`. Reinstalling the installed copy for this is
**optional**: the relay `Dockerfile` declares both spellings and uses
`BEEKEEPER_<X>` when it is set, `BUZZ_<X>` otherwise, so a deployer installed
before the rename still builds an image that discloses its commit. Reinstall
when convenient, so the host stops depending on that fallback.

What does not change: `BUZZ_IMAGE` (and the other names Compose interpolates,
listed in `deploy/compose/README.md`) keeps its name in `.env`, and the
`RELAY_CONTAINER` / `PG_CONTAINER` defaults stay `buzz-prod-*`, because they
come from `name: buzz-prod` in compose.yml. The relay itself reads the
`BEEKEEPER_*` names that compose.yml now sets, and still reads any `BUZZ_*`
name in `.env` whose new twin is unset.

## Configuration

`/etc/default/<unit>` is parsed by **systemd, not a shell**: plain `KEY=value`,
no `export`, no command substitution, no variable expansion. The unit uses
`EnvironmentFile=` with **no `-` prefix**, so a missing config file fails the
unit rather than letting the deployer start unconfigured and choose its own
target.

The script validates every required value before the first `incus` call, and
rejects a `REPO_ID` that is empty or non-numeric. That case matters more than
it looks: a set-but-empty value slips past `set -u`, and the resulting SQL
`where repo_id =  and ...` is a syntax error that yields an empty row — which
the "no usable pipeline row" branch would treat as a perfectly normal quiet
exit.

## Tests

```bash
just autodeploy-test
```

They stub `incus`, `flock` and `sleep` on `PATH`, so they need no host and run
in about a second. `flock` is util-linux and absent on macOS, which is why it
is stubbed rather than assumed. Wired into `.woodpecker/gate.yml` as the
`deploy-scripts` step and into `just check`.

Covered: invalid config is fatal before anything reaches the host; a
still-building pipeline does not deploy; already-current is a silent no-op; an
unreadable mirror is FATAL and does not masquerade as a sync delay; the query
is pinned to the configured repo; the scratch sqlite path is per-target; and a
successful deploy caps the build cache while a failed one leaves it alone.

Each case was verified to fail when its property is removed from the script —
a test that cannot fail is worse than no test, because it reads as coverage.

## Install

Requires root on agincus. `sudo install` of an scp'd file clears the permission
check where `sudo sed -i` and piping into `sudo python3` do not; copy up, then
install.

```bash
scp deploy/autodeploy/autodeploy agincus:/tmp/
scp deploy/autodeploy/etc-default/beekeeper-autodeploy agincus:/tmp/
scp deploy/autodeploy/beekeeper-autodeploy.{service,timer} agincus:/tmp/

ssh agincus '
  sudo install -m 0755 /tmp/autodeploy /usr/local/sbin/autodeploy &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy /etc/default/beekeeper-autodeploy &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.service /etc/systemd/system/ &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.timer   /etc/systemd/system/ &&
  sudo systemctl daemon-reload &&
  sudo systemctl enable --now beekeeper-autodeploy.timer
'
```

## Verify — positively, not by absence

**A silent no-op is weak evidence.** It is exactly what the `safe.directory`
bug produced: nothing happened, and nothing was wrong-looking. "Nothing
happened" cannot distinguish *correctly current* from *broken into permanent
silence*. Check the mechanism directly instead.

**Step 0 — read NIP-11 `software_commit` before anything else.** The steps
below (repo=, mirror readability, what Woodpecker selected, `BUZZ_IMAGE`) all
reason about *shas*; this is the one step that asks the running relay what it
actually is, over the wire, the same way any client would (finding 32,
`docs/INTEGRATION.md` § NIP-11). It's also the cheapest: no `incus exec`, no
host access, just an HTTP GET.

```bash
curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/   | jq '{software_commit, software_commit_count, build_time}'
# or, for the plain-text liveness check (`ok <sha8>`):
curl -s https://hive.agiterra.org/health
```

An `unknown` `software_commit` on hive (as opposed to a third-party
build of this image, where it is legitimate) means the deployed image predates
this lane's `--build-arg BEEKEEPER_SOURCE_SHA=$sha` (`BUZZ_SOURCE_SHA` in a
deployer installed before the rename) in step 3 of `autodeploy` below
— check that the build-arg is actually present in the deployer script running
on the host, not just in this checkout.

A `null` `software_commit_count` beside a *known* `software_commit` is the
same signal one level down: the deployed image predates
`--build-arg BEEKEEPER_SOURCE_COMMIT_COUNT=$count` (or its `BUZZ_` spelling),
or the mirror could not answer
`rev-list --count` for that commit. It is never a fault in the relay, and it
is never guessed — the pair is resolved together in `build.rs`, so a missing
count is disclosed rather than filled in from whatever history happened to be
lying around. Clients comparing builds simply lose the distance and fall back
to comparing the commits themselves.

```bash
# 1. the unit resolves its own config and is pinned to repo 2.
#
# Run the service once and read the line it logs. Do NOT reach for
# `systemctl show -p Environment` — that property reflects only `Environment=`
# directives, and comes back EMPTY for values supplied by EnvironmentFile,
# which systemd resolves at exec time. It looks like a missing config when
# nothing is wrong.
ssh agincus 'sudo systemctl start beekeeper-autodeploy.service
             journalctl -u beekeeper-autodeploy -n 2 --no-pager'
# expect, on a current relay:
#   [beekeeper-autodeploy] repo=2 branch=main selected=<sha>(success) deployed=<sha> — up to date
# The repo= field is the assertion. If the relay is behind, this starts a real
# ~15 min build instead — check which case you are in first.

# 2. the mirror is readable by root the way the script reads it
ssh agincus 'm=/srv/git/beekeeper.git
  incus exec forge -- git -c safe.directory=$m -C $m rev-parse --git-dir >/dev/null && echo "$m readable"'

# 3. what the deployer would select
ssh agincus "incus exec forge -- sh -c \"docker cp woodpecker-server-1:/var/lib/woodpecker/woodpecker.sqlite /tmp/c.sqlite >/dev/null && sqlite3 /tmp/c.sqlite \\\"select status, substr(commit,1,9) from pipelines where repo_id = 2 and branch = 'main' and event = 'push' order by id desc limit 1;\\\"; rm -f /tmp/c.sqlite\""

# 4. what the relay is actually running
ssh agincus 'incus exec hive -- grep -m1 ^BUZZ_IMAGE= /opt/beekeeper/compose/.env'
```

Step 3 is the one that caught the cross-deploy hazard. If the selected sha is
not a commit you recognise as belonging to `agiterra/beekeeper`, stop the timer
before it finishes.

### A trailing `BUZZ_IMAGE` is now sometimes correct

Since 2026-08-23, documentation-only pushes to `main` skip the Woodpecker gate
(`.woodpecker/gate.yml` `path.exclude`), so no green pipeline exists for them
and nothing is deployed. **The deployed image therefore trails `origin/main`
by design whenever every commit since it was markdown.** Before this, a
trailing tag meant something was wrong; now it usually does not, which is
exactly the kind of ambiguity that hides a real fault.

Tell the two apart by asking what actually changed, rather than by comparing
shas:

```bash
# What is deployed, and what is main?
ssh agincus 'incus exec hive -- grep -m1 ^BUZZ_IMAGE= /opt/beekeeper/compose/.env'
git fetch origin && git log --oneline -1 origin/main

# Anything between them that is NOT markdown? Empty output = correctly trailing.
git diff --name-only <deployed-sha>..origin/main -- . ':(exclude)*.md' ':(exclude)docs/**/*.md'
```

Non-empty output with the relay still on the old image is a real fault — read
the deployer's journal next, not the sha comparison.

After any deploy, confirm the relay's NIP-11 `self` still matches its recorded
baseline (see `plans/SESSION_STATE.md` § 1). `BUZZ_RELAY_PRIVATE_KEY`
auto-generates when unset, so a relay can silently adopt a new identity on
restart and evict every client's cache.

## Paper trail

- `journalctl -u beekeeper-autodeploy`
- `<BASE>/deploy.log` in the target instance
- `<BASE>/build-<short9>.log`
- `<BASE>/autodeploy-failed-<short9>` — left after a failed attempt so the
  timer does not loop. Remove it to retry.
