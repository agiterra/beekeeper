#!/usr/bin/env bash
# Run the same fixed Planning inputs through an old host and this checkout,
# and say whether every output matches.
#
# For each side it builds the plugins from that side's own sources
# (scripts/build-plugins.sh), prints each component's sha256, then runs the
# capture block in crates/beekeeper-plugin-host/tests/runtime.rs (between the
# BEGIN/END planning-fixed-capture markers). The base revision does not have
# that block, so this script appends the checkout's copy to the base's
# tests/runtime.rs: both hosts run identical capture code, and only the host,
# the WIT and the plugins differ. The capture covers result, drafts, people,
# logs and error text; fuel_used is left out because it measures the build.
#
# Usage: scripts/compare-planning-outputs.sh [base-rev]
#   base-rev  default e8f2231b6 (the host before the webhook world split)
#
# The checkout must be clean, so "new" is exactly HEAD. Each side builds in its
# own target dir under a temp dir, which is removed on exit with the base
# worktree. Exit 0 when both outputs and the pinned
# testdata/plugins/planning-fixed-outputs.json are equal; 1 when any differs
# (a diff is printed); 2 on a setup failure.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
base_rev="${1:-e8f2231b6}"
tests="crates/beekeeper-plugin-host/tests/runtime.rs"
pinned="$root/testdata/plugins/planning-fixed-outputs.json"

cd "$root"
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "compare-planning-outputs: the checkout has uncommitted changes; commit them so 'new' is HEAD" >&2
  exit 2
fi
base="$(git rev-parse --verify "$base_rev^{commit}")" || {
  echo "compare-planning-outputs: no commit '$base_rev'" >&2
  exit 2
}
head="$(git rev-parse HEAD)"

work="$(mktemp -d "${TMPDIR:-/tmp}/compare-planning.XXXXXX")"
# shellcheck disable=SC2329 # run by the EXIT trap
cleanup() {
  git -C "$root" worktree remove --force "$work/base" 2>/dev/null || true
  rm -r "$work"
}
trap cleanup EXIT

block="$work/capture-block.rs"
sed -n '/^\/\/ BEGIN planning-fixed-capture/,/^\/\/ END planning-fixed-capture/p' "$tests" >"$block"
if ! grep -q '^// END planning-fixed-capture' "$block"; then
  echo "compare-planning-outputs: no BEGIN/END planning-fixed-capture block in $tests" >&2
  exit 2
fi
if git show "$base:$tests" | grep -q '^// BEGIN planning-fixed-capture'; then
  echo "compare-planning-outputs: $base already has a capture block; compare against an older base" >&2
  exit 2
fi

git worktree add --quiet --detach "$work/base" "$base"
printf '\n' >>"$work/base/$tests"
cat "$block" >>"$work/base/$tests"

echo "base  $base"
echo "new   $head"
echo "rustc $(rustc -V)"
echo "capture block sha256 $(shasum -a 256 "$block" | cut -d' ' -f1)"

# side name, source dir
capture() {
  local side="$1" dir="$2"
  local target="$work/$side-target"
  echo
  echo "== $side"
  (cd "$dir" && CARGO_TARGET_DIR="$target" ./scripts/build-plugins.sh >/dev/null)
  for wasm in planning test-faults; do
    echo "component $wasm.wasm sha256 $(shasum -a 256 "$target/plugins/$wasm.wasm" | cut -d' ' -f1)"
  done
  (cd "$dir" && CARGO_TARGET_DIR="$target" BEEKEEPER_PLUGIN_ARTIFACTS="$target/plugins" \
    PLANNING_FIXED_OUT="$work/$side.json" \
    cargo test --quiet -p beekeeper-plugin-host --test runtime capture_planning_fixed_outputs \
    -- --ignored --exact >/dev/null)
  if [[ ! -s "$work/$side.json" ]]; then
    echo "compare-planning-outputs: the $side capture wrote nothing" >&2
    exit 2
  fi
  echo "outputs sha256 $(shasum -a 256 "$work/$side.json" | cut -d' ' -f1) ($(jq length "$work/$side.json") calls)"
}

capture base "$work/base"
capture new "$root"

# The pinned file is formatted by hand; compare it after one canonical form.
jq -S . "$pinned" >"$work/pinned.json"
jq -S . "$work/base.json" >"$work/base.sorted.json"
jq -S . "$work/new.json" >"$work/new.sorted.json"
echo
echo "pinned outputs sha256 $(shasum -a 256 "$work/pinned.json" | cut -d' ' -f1) (canonical; base and new below are canonical too)"
echo "base   outputs sha256 $(shasum -a 256 "$work/base.sorted.json" | cut -d' ' -f1)"
echo "new    outputs sha256 $(shasum -a 256 "$work/new.sorted.json" | cut -d' ' -f1)"

status=0
if ! diff -u "$work/base.sorted.json" "$work/new.sorted.json"; then
  echo "DIFFERENT: base and new outputs differ"
  status=1
fi
if ! diff -u "$work/pinned.json" "$work/new.sorted.json"; then
  echo "DIFFERENT: new outputs differ from $pinned"
  status=1
fi
if [[ $status -eq 0 ]]; then
  echo "EQUAL: base, new and pinned outputs match for all $(jq length "$work/new.json") calls"
fi
exit $status
