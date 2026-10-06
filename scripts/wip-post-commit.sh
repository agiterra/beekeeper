#!/usr/bin/env bash
# Share the commit that just happened under a wip ref — or do nothing at all.
#
# This hook is the whole of Project Pulse's local-commit data path. Nothing in
# it asks anybody to report anything: the hire host installs it, the agent runs
# `git commit`, and the ref appears on the wire. A person gets the same hook
# from lefthook, and it stays inert until they run `just wip-share-on`.
#
# It never publishes uncommitted work. Nothing below inspects the working tree,
# stages anything, or stashes anything — a test asserts those commands are
# absent from these bytes. `HEAD`, the commit git has already made by the time a
# post-commit hook runs, is the only thing this can push; whatever is still in
# the working tree or the index stays on the machine.
#
# `set -e` is deliberately absent. A post-commit hook that fails somebody's
# commit is worse than no hook at all, so every path here exits 0 and leaves a
# line in "$(git rev-parse --git-dir)/buzz-wip-push.log" instead.
#
# There is no `eval` and no unquoted expansion anywhere in this file.
#
# These exact bytes are also compiled into
# `beekeeper_core::seat_git_hooks::WIP_POST_COMMIT_HOOK`, so the seat installer and
# lefthook write the same script; a test holds the two together.
set -uo pipefail

git_dir="$(git rev-parse --git-dir 2>/dev/null || true)"
log_file="${git_dir:-.git}/buzz-wip-push.log"

log() {
  printf '%s wip-post-commit: %s\n' \
    "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$1" >>"$log_file" 2>/dev/null || true
}

# Off unless this checkout turned it on. `just wip-share-on` is the only thing
# that sets this for a person; `just setup` and `just hooks` never do.
[ "$(git config --get buzz.wipShare 2>/dev/null || true)" = "true" ] || exit 0

# Lowercase, `[a-z0-9-]` only, collapsed, trimmed, bounded to 40 bytes — the
# same shape `beekeeper_core::seat_git_hooks::wip_ref_name` derives.
sanitize() {
  printf '%s' "$1" \
    | tr '[:upper:]' '[:lower:]' \
    | sed -e 's/[^a-z0-9-]/-/g' -e 's/--*/-/g' -e 's/^-*//' -e 's/-*$//' \
    | cut -c1-40 \
    | sed -e 's/-*$//'
}

branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [ "$branch" = "HEAD" ]; then
  branch=""
fi

ref=""
configured_ref="$(git config --get buzz.wipRef 2>/dev/null || true)"
case "$configured_ref" in
  refs/heads/wip/*) ref="$configured_ref" ;;
esac

if [ -z "$ref" ]; then
  role="$(sanitize "$(git config --get buzz.seatRole 2>/dev/null || true)")"
  assignment="$(git config --get buzz.assignmentId 2>/dev/null || true)"
  if [ -n "$role" ] && printf '%s' "$assignment" | grep -Eq '^[0-9a-f]{64}$'; then
    # A seat: role plus the assignment's first 8 hex is already unique to one
    # seat on one assignment.
    slug="$(printf '%s' "$assignment" | cut -c1-8)"
    ref="refs/heads/wip/$role/$slug"
  else
    # A person: the first segment is **their own pubkey**, never a shared word.
    # `wip/human/<branch>` collided — two people on `main` overwrote each
    # other's ref and 30618 re-attributed the loser's commits to the winner
    # (REVIEW-L9 F6). `just wip-share-on` resolves and records the identity;
    # without one this hook shares nothing rather than sharing a ref that is
    # not solely yours.
    identity="$(sanitize "$(git config --get buzz.wipIdentity 2>/dev/null || true)")"
    if [ -z "$identity" ]; then
      log "no buzz.wipIdentity recorded; run 'just wip-share-on' so this checkout pushes to a ref that is yours alone"
      exit 0
    fi
    slug="$(sanitize "$branch")"
    if [ -z "$slug" ]; then
      log "no branch name to derive a wip ref from"
      exit 0
    fi
    ref="refs/heads/wip/$identity/$slug"
  fi
fi

# Force is allowed on this namespace and nowhere else.
#
# Every ladder above produces a `refs/heads/wip/` name by construction, so this
# is a belt-and-braces assertion rather than a reachable branch: it exists so
# that a future edit to the ladder cannot turn a force-push loose on a real
# branch without tripping here first (REVIEW-L9 F9 measured it unreachable
# today, and unreachable is the point).
case "$ref" in
  refs/heads/wip/*) ;;
  *)
    log "refusing to push '$ref': only refs/heads/wip/* may be force-pushed"
    exit 0
    ;;
esac

# Never a hard-coded remote name. Two pre-push guards in this repo named one
# and both broke silently the day the remote names moved.
remote="$(git config --get buzz.wipRemote 2>/dev/null || true)"
if [ -z "$remote" ] && [ -n "$branch" ]; then
  remote="$(git config --get "branch.$branch.pushRemote" 2>/dev/null || true)"
fi
if [ -z "$remote" ]; then
  remote="$(git config --get remote.pushDefault 2>/dev/null || true)"
fi
if [ -z "$remote" ] && [ -n "$branch" ]; then
  remote="$(git config --get "branch.$branch.remote" 2>/dev/null || true)"
fi
if [ -z "$remote" ]; then
  remotes="$(git remote 2>/dev/null || true)"
  if [ "$(printf '%s\n' "$remotes" | grep -c '.')" = "1" ]; then
    remote="$(printf '%s\n' "$remotes" | grep '.')"
  fi
fi
if [ -z "$remote" ]; then
  log "no remote resolved for '$ref'; the commit stays local"
  exit 0
fi

sha="$(git rev-parse HEAD 2>/dev/null || true)"
if [ -z "$sha" ]; then
  log "no HEAD to share"
  exit 0
fi

if ! git push --no-verify --force "$remote" "HEAD:$ref" >>"$log_file" 2>&1; then
  log "push of $sha to '$remote' '$ref' failed; the commit is local and untouched"
  exit 0
fi
log "pushed $sha to '$remote' '$ref'"

# No checkpoint is published here, and that is deliberate rather than missing.
# Kind 44246's checkpoint body is `deny_unknown_fields` and has, on this base,
# no field for a commit sha, a branch or a subject — so a checkpoint could not
# carry the commit at all — while its required test counts are numbers nobody
# measured. Publishing one would mean inventing both. The `buzz.sessionRef`,
# `buzz.genesisRef` and `buzz.channel` lines are configured and waiting for the
# wire to gain those fields.
log "pushed; no checkpoint published: the 44246 checkpoint body has no field for a commit sha yet"

exit 0
