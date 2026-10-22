#!/usr/bin/env bash
# Format or lint every plugin workspace under plugins/ (the list is
# scripts/lib/plugins.sh). The root `just fmt` / `fmt-check` / `clippy`
# recipes call this, because `--all` and `--workspace` stop at the root
# workspace and no plugin is a member of it.
#
# Usage: scripts/plugins-lint.sh fmt|fmt-check|clippy
#   clippy runs twice per plugin, -D warnings each time: for
#   wasm32-unknown-unknown, the target it ships as, and natively, where its
#   tests run. Both use <target>/plugin-build, the directory build-plugins.sh
#   already compiles plugins into.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/plugins.sh
source "$root/scripts/lib/plugins.sh"
target="${CARGO_TARGET_DIR:-$root/target}"
mode="${1:-}"

status=0
for spec in "${BEEKEEPER_PLUGINS[@]}"; do
  IFS=: read -r name dir _ <<<"$spec"
  manifest="$root/$dir/Cargo.toml"
  case "$mode" in
    fmt)
      cargo fmt --manifest-path "$manifest" --all || status=1
      ;;
    fmt-check)
      echo "==> plugin $name: fmt --check"
      cargo fmt --manifest-path "$manifest" --all -- --check || status=1
      ;;
    clippy)
      echo "==> plugin $name: clippy (wasm32-unknown-unknown)"
      CARGO_TARGET_DIR="$target/plugin-build" cargo clippy --locked --manifest-path "$manifest" \
        --all-targets --target wasm32-unknown-unknown -- -D warnings || status=1
      echo "==> plugin $name: clippy (native)"
      CARGO_TARGET_DIR="$target/plugin-build" cargo clippy --locked --manifest-path "$manifest" \
        --all-targets -- -D warnings || status=1
      ;;
    *)
      echo "usage: $0 fmt|fmt-check|clippy" >&2
      exit 2
      ;;
  esac
done
exit "$status"
