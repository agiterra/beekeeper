# Beekeeper Docker Compose deployment

This is the single-node/VPS deployment bundle. It is intentionally separate from
the root `docker-compose.yml`, which remains local development infrastructure.

## Quick start

```bash
cd deploy/compose
cp .env.example .env
$EDITOR .env       # replace every CHANGE_ME value
./run.sh start
```

For a public VPS with automatic Let's Encrypt certificates:

```bash
cd deploy/compose
BUZZ_COMPOSE_TLS=true ./run.sh start
```

The bootstrap script should eventually replace manual `.env` editing for normal
users. It is responsible for generating stable secrets and, optionally, an owner
keypair.

## Production notes

- Requires Docker Compose v2.24.4 or newer; the TLS override uses Compose's
  `!reset` tag to remove the direct relay port when Caddy terminates HTTPS.
- `BUZZ_IMAGE` has **no published default any more**: it defaults to
  `beekeeper-relay:latest`, which is a local build, because Beekeeper does not
  publish a relay image. Build one with `docker build -t beekeeper-relay:latest
  .` from the repository root, or set `BUZZ_IMAGE` to whatever registry you
  publish to. The hive relay never relies on this default — its deployer sets
  `BUZZ_IMAGE` to the image it just built on the host.
- **Two spellings of environment names.** The relay reads `BEEKEEPER_*`
  names; they were `BUZZ_*` before the rename, and the relay still reads a
  `BUZZ_*` name whose `BEEKEEPER_*` twin is unset (and says so in one startup
  line, names only). The names Compose itself interpolates — `BUZZ_IMAGE`,
  `BUZZ_DOMAIN`, `BUZZ_HTTP_PORT`, `BUZZ_S3_ACCESS_KEY`, `BUZZ_S3_SECRET_KEY`,
  `BUZZ_S3_BUCKET`, `BUZZ_AUTO_MIGRATE`, `BUZZ_GIT_CONFORMANCE_PROBE` — and
  run.sh's `BUZZ_COMPOSE_TLS` / `BUZZ_COMPOSE_DEV` keep their old names for now:
  existing `.env` files are hand-managed, so renaming those is a coordinated
  step of its own. `compose.yml` maps each of them onto the relay's
  `BEEKEEPER_*` name, and a name it pins there wins over a legacy `BUZZ_*`
  copy arriving through `env_file`.
- Keep `BEEKEEPER_RELAY_PRIVATE_KEY`, `BEEKEEPER_GIT_HOOK_HMAC_SECRET` (or their
  `BUZZ_*` spellings in an older `.env`), database/Redis, and S3 secrets stable
  across restarts.
- `RELAY_OWNER_PUBKEY` is intentionally not prefixed with `BEEKEEPER_`; it must
  be a 64-character hex Nostr pubkey when closed relay mode is enabled.
- `BUZZ_AUTO_MIGRATE` is opt-in. Set `BUZZ_AUTO_MIGRATE=true` or run
  `beekeeper-admin migrate` before starting the relay when bootstrapping a fresh
  database. Auto-migration requires an image that includes embedded SQLx
  migrations.
- The stack uses Postgres, Redis, RustFS, and a git data volume because
  those are real Beekeeper dependencies today. Minimal mode can simplify this later.
- The bundled Compose stack fixes the relay endpoint to `http://rustfs:9000` and
  `BEEKEEPER_S3_ADDRESSING_STYLE=path`: Docker DNS resolves `rustfs`, not
  `<bucket>.rustfs`. It is not configurable for an external S3 provider through
  `.env`; use a custom Compose configuration for providers
  such as new Railway Storage Buckets that require `virtual` addressing.

- The relay image's binaries are `beekeeper-relay` and `beekeeper-admin`, with
  `buzz-relay` and `buzz-admin` links to them for scripts written before the
  rename. `./run.sh add-member` and friends use `beekeeper-admin` and fall back
  to `buzz-admin` on an older image.

Run `./run.sh backup-hint` for the backup checklist.

## Validation

Before sharing an install link publicly, verify a fresh install with:

```bash
cd deploy/compose
cp .env.example .env
$EDITOR .env
./run.sh config
./run.sh start
curl -fsS "http://127.0.0.1:$(grep -E '^BUZZ_HTTP_PORT=' .env | cut -d= -f2-)/_liveness"
./run.sh status
```
