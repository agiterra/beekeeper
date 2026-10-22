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
