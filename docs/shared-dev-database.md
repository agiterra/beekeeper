# The shared dev database and the worktree migration skew

**Status:** operational note for this fork. Written 2026-08-15 after the
hazards below destroyed a local dev database during the sessions authority
phase.

One Postgres instance (`buzz-postgres`, via `docker compose`) serves every
worktree in this fork. Branches carry **different migration sets** — far fewer
of them now that the fork is a single `main` plus short-lived topic branches,
but a topic branch that adds a migration still skews the database against every
other worktree. The database can only match one branch at a time, and several
tools will silently or loudly disagree with it. All four hazards below are real
and were hit in one afternoon.

## 1. The `#[ignore]`d buzz-db tests rebuild the schema from the invoking worktree

`crates/buzz-db` has ~120 Postgres-backed tests marked
`#[ignore = "requires Postgres"]`. Many are destructive: they `DROP SCHEMA
public CASCADE` and re-migrate **using the migrations of whichever worktree
invoked them**.

Run them from a feature branch against the shared database and it is silently
downgraded — every migration newer than that branch is gone, along with the
tables they created. During this phase, running them from
`feature/coding-sessions` (31 migrations) took the shared database from v34 to
v31 and deleted `project_acl` and the `channels.project_ref` column, both of
which come from `feature/project-access`.

Nothing warns you. The tests pass.

**Rule: never point a Postgres-backed buzz-db test at the default database.**
Create a throwaway one:

```bash
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -d postgres \
  -c "DROP DATABASE IF EXISTS buzz_scratch; CREATE DATABASE buzz_scratch;"
scratch="postgres://buzz:buzz_dev@localhost:5432/buzz_scratch"
DATABASE_URL="$scratch" cargo run -q -p buzz-admin -- migrate
DATABASE_URL="$scratch" BUZZ_TEST_DATABASE_URL="$scratch" \
  cargo test -p buzz-db --lib <filter> -- --ignored --test-threads=1
```

`just test-genesis` is a worked example of this pattern.

## 2. They deadlock without `--test-threads=1`

The default parallel harness runs them concurrently against one database, where
each test's `DROP SCHEMA` blocks on the open transactions of its neighbours. The
run wedges indefinitely — observed as seven backends `idle in transaction`
behind blocked `DROP SCHEMA` statements, hung for 37 minutes until killed.

Always pass `--test-threads=1`. Diagnose a suspected hang with:

```bash
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -d buzz \
  -c "SELECT pid, state, wait_event_type, wait_event, left(query,60)
      FROM pg_stat_activity WHERE backend_type='client backend';"
```

A related trap: cross-process contention produces failures that look exactly
like code regressions. `deletion::postgres_tests` fails with
`community deletion catalog drift (unknown=late_altered_scoped, …)` when
`migration.rs`'s tests hold their fixture tables concurrently — nothing to do
with the code under test. **Before believing a buzz-db failure, confirm no
other cargo/just process is running.**

## 3. Nothing in the sanctioned gates runs these tests

`scripts/run-tests.sh` runs `cargo test -p buzz-db` **without** `--ignored`, and
says so in its own comment. `just test-unit` runs `--lib` only. So the ignored
set is executed by nothing, and drifts: 6 of its tests already failed at
baseline before this phase touched anything.

A security property whose test never runs is not proven. Where one exists, give
it a targeted gate against an isolated database — see `just test-genesis`.

## 4. `just test` cannot run from a feature worktree against a newer database

`_ensure-migrations` runs `buzz-admin migrate`, which **fails closed** when the
database holds a migration the branch does not resolve:

```
error: migration error: migration 32 was previously applied but is missing in
the resolved migrations
```

This is correct behaviour — it refuses rather than downgrading, and is exactly
the protection hazard 1 bypasses by dropping the schema outright. But it means
integration tests are runnable from whichever branch the database was last
migrated for, and not from one behind it.

Practical consequence: **merge to `main` and re-migrate before expecting
`just test` or a live-relay test to work across worktrees.** The alternative —
pinning integration runs to a branch-matched scratch database — is parallel
infrastructure for a problem one `just migrate` solves.

## Restoring a damaged database

Volumes only; this leaves desktop state and keychain identities alone:

```bash
cd <your clone>                      # a worktree on `main` — matters
docker compose down -v --remove-orphans
docker compose up -d
just migrate                         # applies the full set, reseeds local hosts
```

Do **not** reach for `scripts/dev-reset.sh` for a database problem. Despite the
name it also runs `reset-desktop-dev-state.sh`, which deletes every
`xyz.block.buzz.app.dev*` application-support directory and the
`buzz-desktop-dev` keychain item — i.e. provider state, session history, and dev
agent keys. During this phase those directories held the only local copy of the
orphaned Hallway session records the phase used as a test case.

Verify a restore with:

```bash
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -d buzz \
  -tAc "SELECT max(version) FROM _sqlx_migrations;"          # expect the branch's max
docker exec -e PGPASSWORD=buzz_dev buzz-postgres psql -U buzz -d buzz \
  -tAc "SELECT count(*) FROM pg_tables WHERE tablename='project_acl';"  # expect 1
```

## Unrelated but adjacent: `buzz sessions export` filters kinds

`buzz sessions export` writes provider-authored kinds (44223/44224/44225) and
**omits operator-signed 44221 creates**. A channel whose sessions have full
create history will look like it has none. Query the relay directly
(`POST /query` with `kinds:[44221]`) when auditing session authorship.
