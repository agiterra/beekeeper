# shellcheck shell=bash
# Every plugin under plugins/, one list: scripts/build-plugins.sh builds and
# registers them, scripts/plugins-lint.sh formats and lints them. Each is its
# own Cargo workspace, so no root-workspace gate (`cargo fmt --all`, `cargo
# clippy --workspace`, `cargo test --workspace`) sees any of them. A plugin
# missing here is built, linted and registered by nothing.
#
# name:dir:first-party (yes / no / unregistered)
# shellcheck disable=SC2034  # read by the scripts that source this
BEEKEEPER_PLUGINS=(
  "planning:plugins/planning:yes"
  "planning-fixture:plugins/test-planning-fixture:no"
  "test-faults:plugins/test-faults:no"
  "test-webhook:plugins/test-webhook:unregistered"
)

# The relay's built-in default: the first-party registry build-plugins.sh
# writes (Planning). Every `just` recipe that starts a relay calls this after
# loading .env, so each one runs the same plugins a relay image does. An
# operator's own BEEKEEPER_PLUGINS_FILE, from the environment or .env,
# replaces it wholesale — never merged.
# $1 = repository root.
beekeeper_default_plugins_file() {
  local target="${CARGO_TARGET_DIR:-$1/target}"
  export BEEKEEPER_PLUGINS_FILE="${BEEKEEPER_PLUGINS_FILE:-${target}/plugins/registry.toml}"
  echo "plugins: ${BEEKEEPER_PLUGINS_FILE}" >&2
}
