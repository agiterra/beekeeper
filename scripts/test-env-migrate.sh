#!/usr/bin/env bash
# Offline test for scripts/env-migrate.sh (`just env-migrate`).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MIGRATE="${SCRIPT_DIR}/env-migrate.sh"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/beekeeper-env-migrate-test.XXXXXX")"
trap 'rm -rf "${TEST_DIR}"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
has() { grep -qxF -- "$2" "$1" || fail "$1 should contain: $2"; }
lacks() { ! grep -qE -- "$2" "$1" || fail "$1 should not match: $2"; }

# 1. A local .env: keys renamed, values and `export` kept, Compose names kept,
#    a stale twin dropped, comments renamed, log targets rewritten, one .bak.
ENV="${TEST_DIR}/.env"
cat > "${ENV}" <<'EOF'
# local dev
BUZZ_RELAY_URL=ws://localhost:3000
export BUZZ_PRIVATE_KEY=abc=def
BUZZ_AUTH_TAG=stale
BEEKEEPER_AUTH_TAG=current
# BUZZ_REQUIRE_AUTH_TOKEN=true
BUZZ_IMAGE=beekeeper-relay:local
BUZZ_HTTP_PORT=3000
BUZZ_S3_BUCKET=buzz-media
RUST_LOG=buzz_relay=debug,tower=info
DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz
EOF
chmod 600 "${ENV}"
cp "${ENV}" "${TEST_DIR}/original"

out="$("${MIGRATE}" "${ENV}")"
has "${ENV}" "BEEKEEPER_RELAY_URL=ws://localhost:3000"
has "${ENV}" "export BEEKEEPER_PRIVATE_KEY=abc=def"
has "${ENV}" "BEEKEEPER_AUTH_TAG=current"
lacks "${ENV}" '^BUZZ_AUTH_TAG='
has "${ENV}" "# BEEKEEPER_REQUIRE_AUTH_TOKEN=true"
has "${ENV}" "BUZZ_IMAGE=beekeeper-relay:local"
has "${ENV}" "BUZZ_HTTP_PORT=3000"
# No compose file beside it interpolates BUZZ_S3_BUCKET, so it is renamed.
has "${ENV}" "BEEKEEPER_S3_BUCKET=buzz-media"
has "${ENV}" "RUST_LOG=beekeeper_relay=debug,tower=info"
has "${ENV}" "DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz"
cmp -s "${ENV}.bak" "${TEST_DIR}/original" || fail ".env.bak is not the original"
[[ "$(stat -f '%Lp' "${ENV}" 2>/dev/null || stat -c '%a' "${ENV}")" == 600 ]] ||
  fail "the migrated .env lost its 0600 mode"
grep -q 'abc=def\|stale\|current' <<<"${out}" && fail "the report printed a value"
grep -q 'kept for Compose: BUZZ_IMAGE' <<<"${out}" || fail "report should name the kept Compose names: ${out}"

# 2. Idempotent: a second run changes nothing and keeps the first .bak.
cp "${ENV}" "${TEST_DIR}/after-first"
out="$("${MIGRATE}" "${ENV}")"
grep -q 'already current' <<<"${out}" || fail "second run was not a no-op: ${out}"
cmp -s "${ENV}" "${TEST_DIR}/after-first" || fail "second run changed the file"
cmp -s "${ENV}.bak" "${TEST_DIR}/original" || fail "second run replaced .env.bak"

# 3. A Compose deployment .env: whatever the sibling compose file interpolates
#    as ${BUZZ_*} keeps its name; the rest is renamed.
DEPLOY="${TEST_DIR}/deploy"
mkdir -p "${DEPLOY}"
cat > "${DEPLOY}/compose.yml" <<'EOF'
services:
  relay:
    environment:
      BEEKEEPER_S3_BUCKET: ${BUZZ_S3_BUCKET:-buzz-media}
      BEEKEEPER_AUTO_MIGRATE: ${BUZZ_AUTO_MIGRATE:-false}
EOF
cat > "${DEPLOY}/.env" <<'EOF'
BUZZ_S3_BUCKET=buzz-media
BUZZ_AUTO_MIGRATE=true
BUZZ_DOMAIN=relay.example
BUZZ_RELAY_PRIVATE_KEY=0000000000000000000000000000000000000000000000000000000000000001
EOF
# Without the override a deployment .env is refused and left byte for byte.
cp "${DEPLOY}/.env" "${TEST_DIR}/deploy-original"
out="$("${MIGRATE}" "${DEPLOY}/.env")"
grep -q "deployment .env" <<<"$out" || fail "a deployment .env must be refused without ENV_MIGRATE_DEPLOY=1"
cmp -s "${DEPLOY}/.env" "${TEST_DIR}/deploy-original" || fail "a refused deployment .env was changed"
[[ ! -e "${DEPLOY}/.env.bak" ]] || fail "a refused deployment .env left a .env.bak"
ENV_MIGRATE_DEPLOY=1 "${MIGRATE}" "${DEPLOY}/.env" >/dev/null
has "${DEPLOY}/.env" "BUZZ_S3_BUCKET=buzz-media"
has "${DEPLOY}/.env" "BUZZ_AUTO_MIGRATE=true"
has "${DEPLOY}/.env" "BUZZ_DOMAIN=relay.example"
has "${DEPLOY}/.env" "BEEKEEPER_RELAY_PRIVATE_KEY=0000000000000000000000000000000000000000000000000000000000000001"

# 4. A missing file is not an error.
"${MIGRATE}" "${TEST_DIR}/absent.env" | grep -q 'nothing to do' || fail "missing file"

echo "PASS: env-migrate renames BUZZ_* keys, keeps Compose names, is idempotent"
