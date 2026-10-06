#!/usr/bin/env bash
# =============================================================================
# run-tests.sh — Run Beekeeper test suite
# =============================================================================
# Usage:
#   ./scripts/run-tests.sh              # run all tests (default)
#   ./scripts/run-tests.sh unit         # unit tests only (no infra needed)
#   ./scripts/run-tests.sh integration  # integration tests only
#   ./scripts/run-tests.sh all          # explicit all
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MODE="${1:-all}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log()    { echo -e "${BLUE}[run-tests]${NC} $*"; }
success(){ echo -e "${GREEN}[run-tests]${NC} $*"; }
warn()   { echo -e "${YELLOW}[run-tests]${NC} $*"; }
error()  { echo -e "${RED}[run-tests]${NC} $*" >&2; }
section(){ echo -e "\n${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"; echo -e "${CYAN}  $*${NC}"; echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"; }

cd "${REPO_ROOT}"

# ---- Load .env if present ---------------------------------------------------

if [[ -f ".env" ]]; then
  log "Loading .env..."
  set -o allexport
  # shellcheck disable=SC1091
  source .env
  set +o allexport
else
  # Use defaults matching docker-compose.yml
  export DATABASE_URL="postgres://buzz:buzz_dev@localhost:5432/buzz" # sadscan:disable np.postgres.1
  export PGHOST=localhost
  export PGPORT=5432
  export PGUSER=buzz
  export PGPASSWORD=buzz_dev
  export PGDATABASE=buzz
  export REDIS_URL="redis://localhost:6379"
fi

# ---- Track results ----------------------------------------------------------

declare -a PASSED=()
declare -a FAILED=()

run_test_step() {
  local name="$1"
  shift
  log "Running: ${name}"
  if "$@"; then
    success "${name} passed"
    PASSED+=("${name}")
  else
    error "${name} FAILED"
    FAILED+=("${name}")
  fi
}

# ---- Check / start infra (for integration tests) ----------------------------

ensure_infra() {
  "${REPO_ROOT}/bin/just" _ensure-migrations
}

# ---- Unit tests (no infra needed) -------------------------------------------

run_unit_tests() {
  section "Unit Tests (no infra required)"

  # The whole workspace, not a hand-kept package list. This function used to
  # enumerate nine packages and had to be kept in step by hand with the nextest
  # path in `just test-unit`; everything outside both lists (beekeeper-relay,
  # beekeeper-acp, beekeeper-session-provider, beekeeper-sdk, git-credential-nostr and more —
  # ~3,375 tests) ran in no local gate at all, so a failure there could only be
  # found by pushing. Tests that need Postgres, Redis or MinIO are `#[ignore]`d
  # with a reason, which is what keeps this infra-free; mark new ones rather
  # than re-narrowing this command.
  run_test_step "workspace unit tests" \
    cargo test --workspace -- --nocapture
}

# ---- DB / integration tests (infra required) --------------------------------

run_integration_tests() {
  section "Integration Tests (requires running services)"

  ensure_infra

  run_test_step "beekeeper-db tests" \
    cargo test -p beekeeper-db -- --nocapture

  if find crates/beekeeper-auth/tests -maxdepth 1 -name '*.rs' -print -quit 2>/dev/null | grep -q .; then
    run_test_step "beekeeper-auth integration tests" \
      cargo test -p beekeeper-auth --test '*' -- --nocapture
  else
    run_test_step "beekeeper-auth (no integration tests found)" true
  fi

  run_test_step "workspace integration tests" \
    cargo test --test '*' -- --nocapture 2>/dev/null || \
    run_test_step "workspace integration tests (none found)" true
}

# ---- Main -------------------------------------------------------------------

START_TIME=$(date +%s)

case "${MODE}" in
  unit)
    run_unit_tests
    ;;
  integration)
    run_integration_tests
    ;;
  all|*)
    run_unit_tests
    run_integration_tests
    ;;
esac

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

# ---- Summary ----------------------------------------------------------------

section "Test Summary"
echo ""
echo -e "  Duration: ${ELAPSED}s"
echo ""

if [[ ${#PASSED[@]} -gt 0 ]]; then
  echo -e "  ${GREEN}Passed (${#PASSED[@]}):${NC}"
  for t in "${PASSED[@]}"; do
    echo -e "    ${GREEN}pass${NC} ${t}"
  done
fi

if [[ ${#FAILED[@]} -gt 0 ]]; then
  echo ""
  echo -e "  ${RED}Failed (${#FAILED[@]}):${NC}"
  for t in "${FAILED[@]}"; do
    echo -e "    ${RED}fail${NC} ${t}"
  done
  echo ""
  exit 1
fi

echo ""
success "All tests passed!"
exit 0
