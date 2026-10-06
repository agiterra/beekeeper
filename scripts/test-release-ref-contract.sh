#!/usr/bin/env bash
# Contract for the release tooling that survives the removal of Block's
# publishing and signing lanes (see RELEASING.md): `just release-desktop`
# prepares a release PR on agiterra/beekeeper, and the desktop canaries keep
# their exact release Cargo cache shape.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
prepare="$repo_root/scripts/prepare-desktop-release.sh"

grep -q 'test-release-ref-contract\.sh' "$repo_root/.github/workflows/ci.yml"
"$repo_root/scripts/test-desktop-release-cache-key.sh"
"$repo_root/scripts/test-desktop-release-cache-workflow.sh"

grep -Fq 'reviewed candidate' "$prepare"
grep -Fq 'gh pr list --repo agiterra/beekeeper' "$prepare"
grep -Fq 'gh pr edit --repo agiterra/beekeeper' "$prepare"
grep -Fq 'gh pr create --repo agiterra/beekeeper' "$prepare"
grep -Fq -- '--repo agiterra/beekeeper' "$prepare"
if grep -Fq 'block/buzz' "$prepare" "$repo_root/scripts/desktop_release.py"; then
  echo "desktop release tooling still targets block/buzz" >&2
  exit 1
fi
if grep -Fq 'current `main`' "$prepare"; then
  echo "desktop release PR body contains executable command substitution" >&2
  exit 1
fi

echo "release ref contract passed"
