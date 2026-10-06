#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
mode="${2:-publish}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "usage: $0 <semver> [publish|validate-only]" >&2
  exit 1
}

remote="${RELEASE_REMOTE:-origin}"
git fetch "$remote" refs/heads/main:refs/remotes/origin/main --no-tags
git fetch "$remote" '+refs/tags/v*:refs/tags/v*' '+refs/tags/desktop-v*:refs/tags/desktop-v*'
base_sha="$(git rev-parse refs/remotes/origin/main)"
branch="version-bump/$version"

remote_branch="refs/heads/$branch"
remote_oid=""
if remote_oid="$(git ls-remote "$remote" "$remote_branch" | awk '{print $1}')" && [[ -n "$remote_oid" ]]; then
  git fetch "$remote" "$remote_branch:refs/remotes/origin/$branch"
fi

git checkout -B "$branch" "$base_sha"
just bump-desktop-version "$version"
scripts/desktop_release.py generate "$version" --base "$base_sha" --repo agiterra/beekeeper

git add \
  .release/desktop-candidate.json \
  CHANGELOG.md \
  desktop/package.json \
  desktop/src-tauri/tauri.conf.json \
  desktop/src-tauri/Cargo.toml \
  desktop/src-tauri/Cargo.lock \
  pnpm-lock.yaml

agent_name="${RELEASE_AUTOMATION_NAME:-${AGENT_NAME:-Release Automation}}"
agent_email="${RELEASE_AUTOMATION_EMAIL:-${AGENT_EMAIL:-release-automation@users.noreply.github.com}}"
msg="$(mktemp)"
trap 'rm -f "$msg"' EXIT
cat >"$msg" <<EOF
chore(release): release Beekeeper Desktop version $version

Co-authored-by: $agent_name <$agent_email>
EOF
git commit -s -F "$msg"
scripts/desktop_release.py validate --candidate HEAD --version "$version" --repo agiterra/beekeeper

candidate_sha="$(git rev-parse HEAD)"
previous_tag="$(python3 -c 'import json; print(json.load(open(".release/desktop-candidate.json"))["previous_tag"] or "initial")')"
printf 'base_sha=%s\ncandidate_sha=%s\nprevious_tag=%s\ntag=desktop-v%s\n' \
  "$base_sha" "$candidate_sha" "$previous_tag" "$version"

if [[ "$mode" == validate-only ]]; then
  exit 0
fi
[[ "$mode" == publish ]] || { echo "unknown mode: $mode" >&2; exit 1; }
# Push through the floor wrapper, not a bare `git push`: the bump touches
# crates, and a long pre-push floor outlives the relay's NIP-98 token window
# (AGENTS.md § Quality Gates).
if [[ -n "$remote_oid" ]]; then
  ./scripts/push-with-floor.sh --force-with-lease="$remote_branch:$remote_oid" "$remote" "HEAD:$remote_branch"
else
  ./scripts/push-with-floor.sh --force-with-lease="$remote_branch:" "$remote" "HEAD:$remote_branch"
fi

# GitHub is a mirror of hive, filled by a bridge within seconds of a push
# (docs/INTEGRATION.md § Remotes), so the PR's head branch may not exist there
# yet. Ask GitHub itself rather than naming a remote.
wait_for_github_branch() {
  local repo="$1" branch="$2" i
  for i in $(seq 1 30); do
    gh api "repos/$repo/branches/$branch" --silent 2>/dev/null && return 0
    sleep 2
  done
  echo "error: $branch never reached $repo on GitHub; check the bridge (docs/INTEGRATION.md § Remotes)" >&2
  return 1
}
wait_for_github_branch agiterra/beekeeper "$branch"

body="$(mktemp)"
trap 'rm -f "$msg" "$body"' EXIT
cat >"$body" <<EOF
## Beekeeper Desktop release v$version

- **Frozen main:** \`$base_sha\`
- **Reviewed candidate:** \`$candidate_sha\`
- **Previous desktop release:** \`$previous_tag\`
- **Proposed immutable tag:** \`desktop-v$version\`

The checked-in changelog accounts for every non-merge commit in the release range. The proposed Desktop tag points to the reviewed candidate commit, not a later squash commit.

**Review only — do not merge this PR on GitHub.** GitHub is a mirror of hive; merging here writes the mirror, never reaches hive, and races the bridge. To land, fast-forward \`main\` on hive to the reviewed candidate and push with \`just push\` (RELEASING.md). Nothing builds, signs, tags or publishes from it: the signing and publishing workflows were Block's and have been removed.
EOF
if existing="$(gh pr list --repo agiterra/beekeeper --head "$branch" --state open --json number --jq '.[0].number')" && [[ -n "$existing" ]]; then
  gh pr edit --repo agiterra/beekeeper "$existing" --title "chore(release): release Beekeeper Desktop version $version" --body-file "$body"
else
  gh pr create --repo agiterra/beekeeper --base main --head "$branch" \
    --title "chore(release): release Beekeeper Desktop version $version" --body-file "$body"
fi
