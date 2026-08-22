# Autodeploy

Host-side deployers that watch Woodpecker and ship green builds to the relays
on agincus. They run on the **host**, not inside an instance, because they
drive `incus exec` against both `forge` (Woodpecker's sqlite, the git mirrors)
and the target instance.

| Deployer | Woodpecker repo | Branch | Instance | Relay | Image |
| --- | --- | --- | --- | --- | --- |
| `beekeeper-autodeploy` (here) | id **2**, `agiterra/beekeeper` | `main` | `hive` | hive.agiterra.org | `beekeeper-relay:<short9>` |
| `buzz-autodeploy` (**untracked**) | id **1**, `agiterra/buzz` | `main` | `buzz` | lightyear.agiterra.org | `buzz-relay:<short9>` |

`buzz-autodeploy` still lives only at `/usr/local/sbin/buzz-autodeploy` on
agincus with no copy in any repo. That is why the `repo_id` bug below survived
unreviewed for as long as it did. It should be brought here too.

## Two hazards, both found the hard way

**`repo_id` is not optional.** Woodpecker serves both repos and both use
branch `main`. A query filtered on branch alone returns whichever repo pushed
most recently. An unpinned deployer will build Bee Keeper and deploy it onto
the vanilla relay — and the relay comes up **healthy**, because a relay is a
relay. Wrong product, wrong schema, no alarm. Caught live on 2026-08-22 with
roughly five minutes to spare: run side by side, the pinned query returned the
vanilla commit while the unpinned one returned a Bee Keeper commit mid-build.

**Deleting a branch disarms nothing.** The `pipelines` table keeps history, so
a deployer pointed at a long-dead branch keeps selecting that branch's last
build indefinitely. It stays silent only while that happens to match what is
deployed; change the image and it wakes up and deploys the ghost. This is
exactly what happened to lightyear during the vanilla rebuild — the health
check and rollback were the only things that caught it.

## Retention

`buzz-autodeploy` has none. By 2026-08-22 one host had accumulated 38 SQL
dumps, 34 `.env` backups and 51 relay images. **Each `.env.bak-pre-<sha>` is a
full copy of the relay private key**, so this is a credential problem before
it is a disk problem. The deployer here prunes builds, backups, `.env` backups,
build logs and images after a *successful* deploy only — a failed run leaves
everything in place for diagnosis.

Image pruning explicitly excludes whatever the relay is currently running,
regardless of what the keep-count would otherwise say.

## Install

Requires root on agincus. From a checkout of this repo:

```bash
scp deploy/autodeploy/beekeeper-autodeploy agincus:/tmp/
scp deploy/autodeploy/beekeeper-autodeploy.{service,timer} agincus:/tmp/

ssh agincus '
  sudo install -m 0755 /tmp/beekeeper-autodeploy /usr/local/sbin/beekeeper-autodeploy &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.service /etc/systemd/system/ &&
  sudo install -m 0644 /tmp/beekeeper-autodeploy.timer   /etc/systemd/system/ &&
  sudo systemctl daemon-reload &&
  sudo systemctl enable --now beekeeper-autodeploy.timer &&
  systemctl list-timers beekeeper-autodeploy.timer --no-pager
'
```

## Verify

Know which of the two first runs you are expecting **before** you enable the
timer, because they look nothing alike and each is alarming if you expected the
other.

- **Relay already current** → a silent no-op: started and deactivated within a
  second or two, no log lines. That is the `current == short` early exit.
- **Relay behind** → a real build and deploy, ~15 min, with a pre-deploy
  backup and a health-gated restart. This is the expected case for hive's
  first run: it has never had a deployer, so it is many commits behind
  whatever `main` is green at.

Check which one you are in first — compare the selected commit against the
deployed image:

```bash
# what the deployer will select
ssh agincus "incus exec forge -- sh -c \"docker cp woodpecker-server-1:/var/lib/woodpecker/woodpecker.sqlite /tmp/c.sqlite >/dev/null && sqlite3 /tmp/c.sqlite \\\"select status, substr(commit,1,9) from pipelines where repo_id = 2 and branch = 'main' and event = 'push' order by id desc limit 1;\\\"; rm -f /tmp/c.sqlite\""

# what hive is running
ssh agincus "incus exec hive -- grep -m1 ^BUZZ_IMAGE= /opt/beekeeper/compose/.env"

journalctl -u beekeeper-autodeploy -n 20 --no-pager
```

The thing that should actually alarm you is a build whose commit you do not
recognise as belonging to **this** repo. Cross-check the selected sha against
`git log` on `agiterra/beekeeper` before letting a first run finish; a sha that
turns out to be a `agiterra/buzz` commit means the `repo_id` filter did not
take, and the deploy must be stopped.

## Paper trail

- `journalctl -u beekeeper-autodeploy`
- `/opt/beekeeper/deploy.log` (in the `hive` instance)
- `/opt/beekeeper/build-<short9>.log`
- `/opt/beekeeper/autodeploy-failed-<short9>` — left after a failed attempt so
  the timer does not loop. Remove it to retry.
