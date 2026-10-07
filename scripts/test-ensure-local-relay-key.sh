#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/beekeeper-relay-key-test.XXXXXX")"
trap 'rm -rf "${TEST_DIR}"' EXIT

ENV_FILE="${TEST_DIR}/.env"
cp "${REPO_ROOT}/.env.example" "${ENV_FILE}"

"${SCRIPT_DIR}/ensure-local-relay-key.sh" "${ENV_FILE}" >/dev/null
first_key="$(sed -n 's/^BEEKEEPER_RELAY_PRIVATE_KEY=//p' "${ENV_FILE}")"

if [[ ! "${first_key}" =~ ^[0-9a-f]{64}$ ]]; then
  echo "FAIL: bootstrap did not generate a valid 32-byte hex relay key" >&2
  exit 1
fi

"${SCRIPT_DIR}/ensure-local-relay-key.sh" "${ENV_FILE}" >/dev/null
second_key="$(sed -n 's/^BEEKEEPER_RELAY_PRIVATE_KEY=//p' "${ENV_FILE}")"

if [[ "${first_key}" != "${second_key}" ]]; then
  echo "FAIL: bootstrap replaced the existing relay key" >&2
  exit 1
fi

if [[ "$(grep -c '^BEEKEEPER_RELAY_PRIVATE_KEY=' "${ENV_FILE}")" -ne 1 ]]; then
  echo "FAIL: bootstrap wrote more than one relay key" >&2
  exit 1
fi

# A pre-rename .env names the key BUZZ_RELAY_PRIVATE_KEY. It must be reused,
# not shadowed by a fresh BEEKEEPER_RELAY_PRIVATE_KEY (which would win).
LEGACY_FILE="${TEST_DIR}/legacy.env"
legacy_key="$(printf '%064d' 7)"
printf 'BUZZ_RELAY_PRIVATE_KEY=%s\n' "${legacy_key}" > "${LEGACY_FILE}"
"${SCRIPT_DIR}/ensure-local-relay-key.sh" "${LEGACY_FILE}" >/dev/null
if grep -q '^BEEKEEPER_RELAY_PRIVATE_KEY=' "${LEGACY_FILE}"; then
  echo "FAIL: bootstrap shadowed a legacy BUZZ_RELAY_PRIVATE_KEY with a new key" >&2
  exit 1
fi
if [[ "$(sed -n 's/^BUZZ_RELAY_PRIVATE_KEY=//p' "${LEGACY_FILE}")" != "${legacy_key}" ]]; then
  echo "FAIL: bootstrap changed a legacy BUZZ_RELAY_PRIVATE_KEY" >&2
  exit 1
fi

echo "PASS: bootstrap generates one relay key and reuses it (legacy name too)"
