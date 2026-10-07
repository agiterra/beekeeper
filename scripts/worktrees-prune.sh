#!/usr/bin/env bash
# Remove the lane worktrees nobody records, and refuse everything else.
#
# The hire host records the worktrees it cuts for seats; orchestration cuts
# `lane/*` and `batch*-*` trees by hand and records nothing. This is for the
# second kind. It walks `git worktree list --porcelain` — never a directory
# glob, because a glob finds directories git has already forgotten and misses
# the ones it has not — and for each tree that is not protected:
#
#   merged into <remote>/main AND clean  -> git worktree remove (no --force)
#   merged but dirty                     -> listed with its file count, kept
#   unmerged                             -> refused in one line, however old
#
# The full plan is printed before anything is removed. The remote name is read
# from `git remote`, never hard-coded: two pre-push guards hard-coded one and
# both broke silently the day the remote names moved.
#
# Usage:
#   scripts/worktrees-prune.sh [--dry-run] [--targets] [--repo <path>]
#
#   --dry-run   print the plan and stop. Nothing is removed.
#   --targets   also delete the build state every non-protected merged tree
#               declares in its own sandbox.yml, held or not: no commit lives
#               in a build directory. Needs `bee` to read that declaration,
#               and refuses without it rather than guessing at target/ alone.
#   --repo      operate on this repository instead of the current one. Used by
#               the tests, which build a throwaway layout under a temp dir.

set -euo pipefail

# The repository is `--repo` or the current directory, never the caller's
# environment. Git exports GIT_DIR (and friends) to hooks, and the pre-push
# gate runs this script's tests inside one; inherited, every `git` below would
# answer for the pushing repository instead and find no trunk in it.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_NAMESPACE \
  GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES

DRY_RUN=0
DO_TARGETS=0
REPO=""

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --targets) DO_TARGETS=1 ;;
    --repo) REPO="${2:-}"; shift ;;
    -h|--help) sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "worktrees-prune: unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -n "$REPO" ]; then
  cd "$REPO"
fi

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "worktrees-prune: not inside a git repository" >&2
  exit 2
fi

# The repository's main worktree: never a candidate, whatever else is true.
MAIN_WORKTREE="$(git worktree list --porcelain | awk 'NR==1 && $1=="worktree" {print $2; exit}')"
SELF="$(pwd -P)"

# ── the trunk to measure "merged" against ────────────────────────────────────
#
# Preference order: a remote-tracking main, then a local one. The remote list
# is read, not assumed — `origin` is this project's relay host today and was
# something else a week ago.
TRUNK=""
for remote in $(git remote); do
  for branch in main master; do
    if git rev-parse --verify --quiet "refs/remotes/$remote/$branch" >/dev/null; then
      TRUNK="refs/remotes/$remote/$branch"
      break 2
    fi
  done
done
if [ -z "$TRUNK" ]; then
  for branch in main master; do
    if git rev-parse --verify --quiet "refs/heads/$branch" >/dev/null; then
      TRUNK="refs/heads/$branch"
      break
    fi
  done
fi
if [ -z "$TRUNK" ]; then
  echo "worktrees-prune: no main or master found on any remote or locally; refusing to guess a trunk" >&2
  exit 2
fi

echo "worktrees-prune: measuring merged against $TRUNK"
echo "worktrees-prune: main worktree $MAIN_WORKTREE is protected"
echo

# ── walk the porcelain listing ───────────────────────────────────────────────
PLAN_REMOVE=()
PLAN_HELD=()
PLAN_REFUSE=()
PLAN_PROTECTED=()

current=""
while IFS= read -r line; do
  case "$line" in
    "worktree "*) current="${line#worktree }" ;;
    "")
      [ -n "$current" ] || continue
      path="$current"
      current=""

      resolved="$(cd "$path" 2>/dev/null && pwd -P || echo "$path")"
      if [ "$resolved" = "$MAIN_WORKTREE" ] || [ "$resolved" = "$SELF" ]; then
        PLAN_PROTECTED+=("$path")
        continue
      fi
      case "$SELF/" in
        "$resolved"/*) PLAN_PROTECTED+=("$path"); continue ;;
      esac

      if [ ! -d "$path" ]; then
        PLAN_REFUSE+=("$path (its directory is gone; run git worktree prune)")
        continue
      fi

      tip="$(git -C "$path" rev-parse HEAD 2>/dev/null || echo "")"
      if [ -z "$tip" ]; then
        PLAN_REFUSE+=("$path (no resolvable HEAD)")
        continue
      fi

      dirty="$(git -C "$path" status --porcelain 2>/dev/null | grep -c . || true)"

      if git merge-base --is-ancestor "$tip" "$TRUNK" 2>/dev/null; then
        if [ "$dirty" -eq 0 ]; then
          PLAN_REMOVE+=("$path")
        else
          PLAN_HELD+=("$path ($dirty uncommitted files)")
        fi
      else
        PLAN_REFUSE+=("$path (not merged into $TRUNK)")
      fi
      ;;
  esac
done < <(git worktree list --porcelain; echo)

print_group() {
  local title="$1"; shift
  echo "$title"
  if [ "$#" -eq 0 ]; then
    echo "  (none)"
  else
    for entry in "$@"; do echo "  $entry"; done
  fi
  echo
}

print_group "protected — never touched:" ${PLAN_PROTECTED+"${PLAN_PROTECTED[@]}"}
print_group "merged and clean — will be removed:" ${PLAN_REMOVE+"${PLAN_REMOVE[@]}"}
print_group "merged but dirty — kept, listed for you:" ${PLAN_HELD+"${PLAN_HELD[@]}"}
print_group "unmerged — refused, however old:" ${PLAN_REFUSE+"${PLAN_REFUSE[@]}"}

if [ "$DO_TARGETS" -eq 1 ]; then
  echo "--targets: build state will also be removed from every merged tree above"
  echo "           (what counts as build state comes from each tree's sandbox.yml)"
  echo
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "worktrees-prune: --dry-run, nothing removed"
  exit 0
fi

# ── act ──────────────────────────────────────────────────────────────────────
#
# What counts as build state is the project's to declare, in its own
# sandbox.yml, and `bee sandbox reclaim-paths` is the one reader of it. This
# script used to hardcode `target/`, which is how four copies of one list came
# to disagree: the list missed desktop/src-tauri/target, so a built tree had
# roughly half its build output freed by something printing success.
#
# Without `bee`, this REFUSES rather than falling back to `target/` alone. A
# half-reclaim that prints a whole-reclaim summary is the failure this exists
# to prevent.
if [ "$DO_TARGETS" -eq 1 ]; then
  BEE="${BEEKEEPER_BEE:-$(command -v bee 2>/dev/null || true)}"
  if [ -z "$BEE" ] || [ ! -x "$BEE" ]; then
    echo "worktrees-prune: --targets needs 'bee' to read each tree's sandbox.yml," >&2
    echo "  which is what says any of this is build state. Build it with" >&2
    echo "  'cargo build -p beekeeper-cli', install the app, or set BEEKEEPER_BEE." >&2
    echo "  Refusing rather than guessing at 'target/' alone." >&2
    exit 1
  fi
  for entry in ${PLAN_REMOVE+"${PLAN_REMOVE[@]}"} ${PLAN_HELD+"${PLAN_HELD[@]}"}; do
    path="${entry%% (*}"
    # NUL-separated `path\0action` pairs, so a path with a space cannot split.
    while IFS= read -r -d '' relative && IFS= read -r -d '' action; do
      # `${var:?}` on every removal: an empty $path or $relative would make
      # these `rm -rf /`, and the shell must stop rather than find out.
      case "$action" in
        delete)
          # Never follow a link: a path declared a directory that turns out to
          # be a link is unlinked, not deleted through.
          if [ -L "${path:?}/${relative:?}" ]; then
            rm -f "${path:?}/${relative:?}"
            echo "unlinked $path/$relative (a link, so this frees nothing)"
          elif [ -d "${path:?}/${relative:?}" ]; then
            rm -rf "${path:?}/${relative:?}"
            echo "removed $path/$relative"
          fi
          ;;
        unlink)
          if [ -L "${path:?}/${relative:?}" ]; then
            rm -f "${path:?}/${relative:?}"
            echo "unlinked $path/$relative (frees nothing)"
          fi
          ;;
      esac
    done < <("$BEE" sandbox reclaim-paths --checkout "$path")
  done
fi

removed=0
for path in ${PLAN_REMOVE+"${PLAN_REMOVE[@]}"}; do
  # No --force, ever: git's own refusal is the last guard under every decision
  # made above, and it is the one that has never been wrong.
  if git worktree remove "$path"; then
    echo "removed $path"
    removed=$((removed + 1))
  else
    echo "git refused to remove $path; left alone" >&2
  fi
done
git worktree prune

echo
echo "worktrees-prune: removed $removed worktrees, kept ${#PLAN_HELD[@]} dirty and refused ${#PLAN_REFUSE[@]} unmerged"
