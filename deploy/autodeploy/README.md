# Autodeploy

Host-side deployers that watch Woodpecker and ship green builds to the relays
on agincus. They run on the **host**, not inside an instance, because they
drive `incus exec` against both `forge` (Woodpecker's sqlite, the git mirrors)
and the target instance.

**One script, two relays.** Everything that differs arrives through the
environment, from `/etc/default/<unit>` via each unit's `EnvironmentFile=`:

| | Woodpecker repo | Instance | Relay | Image |
| --- | --- | --- | --- | --- |
| `buzz-autodeploy` | id **1**, `agiterra/buzz` | `buzz` | lightyear.agiterra.org | `buzz-relay:<short9>` |
| `beekeeper-autodeploy` | id **2**, `agiterra/beekeeper` | `hive` | hive.agiterra.org | `beekeeper-relay:<short9>` |

Both track branch `main` and share container names (`buzz-prod-relay-1`,
`buzz-prod-postgres-1` — the compose project is `buzz-prod` in both instances,
which never collide because the instances are separate).

This started as two near-identical scripts, one of them untracked and
root-owned on the host. That invisibility is the direct cause of every hazard
below: each was found by reading the tracked copy, and each still existed in
the copy nobody could see.

## Four hazards, all found the hard way

**`REPO_ID` is not optional.** Woodpecker serves both repos and both use branch
`main`. A query filtered on branch alone returns whichever repo pushed most
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
is pinned to the configured repo; and the scratch sqlite path is per-target.

Each case was verified to fail when its property is removed from the script —
a test that cannot fail is worse than no test, because it reads as coverage.

## Install

Requires root on agincus. `sudo install` of an scp'd file clears the permission
check where `sudo sed -i` and piping into `sudo python3` do not; copy up, then
install.

```bash
scp deploy/autodeploy/autodeploy agincus:/tmp/
scp deploy/autodeploy/etc-default/{buzz,beekeeper}-autodeploy agincus:/tmp/
scp deploy/autodeploy/{buzz,beekeeper}-autodeploy.{service,timer} agincus:/tmp/

ssh agincus '
  sudo install -m 0755 /tmp/autodeploy /usr/local/sbin/autodeploy &&
  sudo install -m 0644 /tmp/buzz-autodeploy      /etc/default/buzz-autodeploy &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy /etc/default/beekeeper-autodeploy &&
  sudo install -m 0644 /tmp/buzz-autodeploy.service      /etc/systemd/system/ &&
  sudo install -m 0644 /tmp/buzz-autodeploy.timer        /etc/systemd/system/ &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.service /etc/systemd/system/ &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.timer   /etc/systemd/system/ &&
  sudo systemctl daemon-reload &&
  sudo systemctl enable --now buzz-autodeploy.timer beekeeper-autodeploy.timer
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
curl -s -H 'Accept: application/nostr+json' https://hive.agiterra.org/   | jq '{software_commit, build_time}'
# or, for the plain-text liveness check (`ok <sha8>`):
curl -s https://hive.agiterra.org/health
```

An `unknown` `software_commit` on hive/lightyear (as opposed to a third-party
build of this image, where it is legitimate) means the deployed image predates
this lane's `--build-arg BUZZ_SOURCE_SHA=$sha` in step 3 of `autodeploy` below
— check that the build-arg is actually present in the deployer script running
on the host, not just in this checkout.

```bash
# 1. each unit resolves its own config, and the two REPO_IDs differ.
#
# Run each service once and read the line it logs. Do NOT reach for
# `systemctl show -p Environment` — that property reflects only `Environment=`
# directives, and comes back EMPTY for values supplied by EnvironmentFile,
# which systemd resolves at exec time. It looks like a missing config when
# nothing is wrong.
ssh agincus 'sudo systemctl start buzz-autodeploy.service beekeeper-autodeploy.service
             journalctl -u buzz-autodeploy -u beekeeper-autodeploy -n 4 --no-pager'
# expect, on a current relay:
#   [buzz-autodeploy]      repo=1 branch=main selected=<sha>(success) deployed=<sha> — up to date
#   [beekeeper-autodeploy] repo=2 branch=main selected=<sha>(success) deployed=<sha> — up to date
# The repo= field is the assertion. If a relay is behind, this starts a real
# ~15 min build instead — check which case you are in first.

# 2. both mirrors are readable by root the way the script reads them
ssh agincus 'for m in /srv/git/buzz.git /srv/git/beekeeper.git; do
  incus exec forge -- git -c safe.directory=$m -C $m rev-parse --git-dir >/dev/null && echo "$m readable"
done'

# 3. what each deployer would select, side by side
ssh agincus "incus exec forge -- sh -c \"docker cp woodpecker-server-1:/var/lib/woodpecker/woodpecker.sqlite /tmp/c.sqlite >/dev/null && for r in 1 2; do echo -n \\\"repo \\\$r: \\\"; sqlite3 /tmp/c.sqlite \\\"select status, substr(commit,1,9) from pipelines where repo_id = \\\$r and branch = 'main' and event = 'push' order by id desc limit 1;\\\"; done; rm -f /tmp/c.sqlite\""

# 4. what each relay is actually running
ssh agincus 'incus exec buzz -- grep -m1 ^BUZZ_IMAGE= /opt/buzz/compose/.env
             incus exec hive -- grep -m1 ^BUZZ_IMAGE= /opt/beekeeper/compose/.env'
```

Step 3 is the one that caught the cross-deploy hazard. If a repo's selected sha
is not a commit you recognise as belonging to *that* repo, stop the timer
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
baseline (see `docs/SESSION_STATE.md` § 1). `BUZZ_RELAY_PRIVATE_KEY`
auto-generates when unset, so a relay can silently adopt a new identity on
restart and evict every client's cache.

## Paper trail

- `journalctl -u {buzz,beekeeper}-autodeploy`
- `<BASE>/deploy.log` in the target instance
- `<BASE>/build-<short9>.log`
- `<BASE>/autodeploy-failed-<short9>` — left after a failed attempt so the
  timer does not loop. Remove it to retry.
