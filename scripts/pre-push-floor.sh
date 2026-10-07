#!/usr/bin/env bash
# The pre-push floor: the fast checks, scoped to what this push actually
# changed, with the cost disclosed in one line.
#
# Why this exists. `pre-push` used to run `just test-unit` — `cargo test
# --workspace`, every crate — for any `crates/**` change, plus the Tauri
# clippy+test pair on the same glob. On 2026-09-02 a seat pushing a one-line
# CLI fix hit its 60 s tool timeout on `git push`, and the answer that night
# was to tell seats `--no-verify`. A gate everyone skips is not a gate, so the
# budget is fixed here instead: fmt and unit tests for the crates the push
# touched, clippy for those plus their dependents (an API break shows in the
# dependants, not in the crate that changed), the JS surface's fast checks when
# that surface moved, and the repository-wide file-size ratchet.
#
# What moved to CI, on purpose: `cargo test --workspace`, every e2e project,
# `just ci` and `just check`. That is a real trade — a class of failure now
# reaches CI instead of the pushing machine — and it is the trade this script
# is for. CONTRIBUTING.md § "What pre-push runs, and what it does not" says so
# in those words.
#
# Three things are never budgeted and never skipped, because they are guards
# rather than tests: `.lefthook/pre-push/push-destination.sh`,
# `scripts/check-branch-skew.sh`, and the `commit-msg` DCO trailer. They stay
# in `lefthook.yml` as their own entries.
#
# The budget is DISCLOSED, not enforced by a kill. A floor that murders a
# nearly-finished clippy run teaches everyone to pass `--no-verify` again, so
# going over prints `over budget by Ns` and still reports the real result.
#
# Proof: `scripts/test-pre-push-floor.sh` (end to end, stubbed toolchain) and
# `node --test scripts/pre-push-floor-scope.test.mjs` (the path mapping).
set -uo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root" || exit 1

# Git exports the current repository's local environment into hooks. In a
# linked worktree that includes an absolute GIT_DIR pointing into the shared
# repository, so `cargo test` would pass that pointer to every test process and
# `git -C <temporary-repo>` would still operate on the checkout being pushed.
# Clear Git's canonical local-variable list after entering this checkout; the
# floor's own git commands rediscover it from cwd, while every tool and test it
# launches gets a neutral environment for its temporary repositories.
while IFS= read -r git_local_variable; do
  [ -n "$git_local_variable" ] && unset "$git_local_variable"
done < <(git rev-parse --local-env-vars)
unset git_local_variable

# Never prompt, and never fetch. `scripts/check-branch-skew.sh` learned this on
# 2026-08-24: a pre-push hook that waits on a terminal has no terminal to wait
# on, and `|| true` does not catch a command that never returns.
export GIT_TERMINAL_PROMPT=0
export GIT_ASKPASS=/usr/bin/true
export SSH_ASKPASS=/usr/bin/true

budget=${BEEKEEPER_PRE_PUSH_FLOOR_BUDGET_SECONDS:-120}
started=$SECONDS

ran_steps=""     # comma-joined names of what actually executed
skipped=""       # comma-joined "<scope> — <reason>"
scope_label=""   # what the floor decided this push is
failed_step=""
test_count=""
notes=""
slowest_step=""
slowest_seconds=0
changed_file=""

append() { # append <varname> <separator> <text>
  local current=${!1}
  if [ -z "$current" ]; then
    printf -v "$1" '%s' "$3"
  else
    printf -v "$1" '%s%s%s' "$current" "$2" "$3"
  fi
}

summarise() {
  local status=$?
  local elapsed=$((SECONDS - started))
  local over=""
  # Only `scripts/push-with-floor.sh` sets this: it already ran the floor
  # before opening the connection, and wants the git hook's later run (the
  # one `git push` itself triggers) to find a stamp instead of running again.
  # A plain `git push` never sets it, so its own hook run never writes one —
  # the stamp is earned by running ahead of the connection, not by passing.
  if [ "$status" = "0" ] && [ -z "$failed_step" ] && [ "${BEEKEEPER_PRE_PUSH_FLOOR_STAMP_WRITE:-}" = "1" ] && [ -n "${changed_file:-}" ]; then
    stamp_git_dir=$(git rev-parse --git-dir 2>/dev/null || true)
    stamp_sha=$(git rev-parse HEAD 2>/dev/null || true)
    if [ -n "$stamp_git_dir" ] && [ -n "$stamp_sha" ]; then
      node "$repo_root/scripts/pre-push-floor-stamp.mjs" write \
        --git-dir "$stamp_git_dir" --sha "$stamp_sha" --scope-file "$changed_file" \
        --ttl "${BEEKEEPER_PRE_PUSH_FLOOR_STAMP_TTL:-600}" >/dev/null 2>&1 || true
    fi
  fi
  [ -n "$changed_file" ] && rm -f "$changed_file"
  # `-ge`, not `-gt`: $SECONDS has one-second resolution, so a budget of 0 run
  # against sub-second work can measure elapsed=0 and still owe a disclosure —
  # a budget of N seconds means "at most N", not "strictly less than N".
  [ "$elapsed" -ge "$budget" ] && over=" over budget by $((elapsed - budget))s"

  {
    echo
    echo "pre-push floor: ${scope_label:-unknown} (${ran_steps:-nothing}) ${elapsed}s / budget ${budget}s${over} · skipped: ${skipped:-nothing} — run in CI"
    if [ -n "$over" ] && [ -n "$slowest_step" ]; then
      # Naming the dominant step, rather than raising the budget to fit the
      # measurement — which is the same move as widening a timeout to fit a
      # flake.
      echo "pre-push floor: over budget; the dominant step was ${slowest_step} at ${slowest_seconds}s"
      if [ "${BEEKEEPER_PRE_PUSH_FLOOR_STAMP_WRITE:-}" != "1" ]; then
        # git already minted its NIP-98 credential before this hook ran, and
        # the relay's token window is +-900s (ledger 178(n)) — a floor this
        # long risks a green hook followed by `HTTP 401` at upload. Next time,
        # `scripts/push-with-floor.sh` (or `just push`) runs the floor first
        # and pushes only after it passes, so the credential is seconds old.
        echo "pre-push floor: next time, use scripts/push-with-floor.sh (or \`just push\`) to run the floor before git opens the connection"
      fi
    fi
    if [ -n "$notes" ]; then
      echo "pre-push floor: ${notes}"
    fi
    if [ -n "$failed_step" ]; then
      echo "pre-push floor: FAILED at ${failed_step}. Fix it, or push with --no-verify only on a SHA \`just ci\` already passed (docs/INTEGRATION.md § Landing a batch)."
    fi
  } >&2
  exit "$status"
}
trap summarise EXIT

# Run one step, name it in the summary, and stop the floor if it fails.
step() {
  local name=$1
  shift
  [ -n "$failed_step" ] && return 0
  local step_started=$SECONDS
  echo "==> floor: ${name}" >&2
  local status=0
  "$@" || status=1
  local step_elapsed=$((SECONDS - step_started))
  if [ -z "$slowest_step" ] || [ "$step_elapsed" -gt "$slowest_seconds" ]; then
    slowest_seconds=$step_elapsed
    slowest_step=$name
  fi
  if [ "$status" = "0" ]; then
    append ran_steps ", " "$name"
    return 0
  fi
  failed_step=$name
  append ran_steps ", " "${name} FAILED"
  return 1
}

# ── 1. what this push changed ────────────────────────────────────────────────
changed_file=$(mktemp)
force_full=0

if [ -n "${BEEKEEPER_PRE_PUSH_FLOOR_CHANGED_FILES:-}" ]; then
  # Used by scripts/test-pre-push-floor.sh, and by anyone timing the floor
  # without minting a commit. Disclosed in the summary: a floor that quietly
  # ran against a made-up change set would be the exact lie this lane removes.
  cat "${BEEKEEPER_PRE_PUSH_FLOOR_CHANGED_FILES}" >"$changed_file"
  append notes " · " "changed set injected by BEEKEEPER_PRE_PUSH_FLOOR_CHANGED_FILES"
else
  base_ref=""
  for candidate in \
    "$(git rev-parse --abbrev-ref main@{upstream} 2>/dev/null || true)" \
    origin/main \
    upstream/main \
    main; do
    [ -n "$candidate" ] || continue
    if git rev-parse --verify --quiet "$candidate" >/dev/null 2>&1; then
      base_ref=$candidate
      break
    fi
  done

  if [ -z "$base_ref" ]; then
    # No base to diff against. Running nothing would be a silent skip — the
    # failure mode this floor exists to remove — so run everything and say why.
    force_full=1
    append notes " · " "no main ref to diff against, so the full floor ran"
    : >"$changed_file"
  else
    base=$(git merge-base HEAD "$base_ref" 2>/dev/null || echo "$base_ref")
    git diff --name-only "$base" HEAD >"$changed_file" 2>/dev/null || true
  fi
fi

changed_count=$(grep -c . "$changed_file" 2>/dev/null || true)
[ -n "$changed_count" ] || changed_count=0

# ── 1a. did scripts/push-with-floor.sh already run this exact floor? ────────
# The stamp names the sha and the exact changed-file set the floor ran
# against, and it is single-use (consumed here whether it turns out valid or
# not) so it can never answer for a later, different push. This is the fix
# for ledger 178(n): without it, a crate-touching push mints its NIP-98
# credential at ref discovery and then spends the floor's ~20 minutes before
# uploading, so the token is expired by the time the pack goes up. A push
# without the wrapper still runs the full floor below, exactly as before.
stamp_git_dir=$(git rev-parse --git-dir 2>/dev/null || true)
stamp_sha=$(git rev-parse HEAD 2>/dev/null || true)
if [ -n "$stamp_git_dir" ] && [ -n "$stamp_sha" ]; then
  stamp_result=$(node "$repo_root/scripts/pre-push-floor-stamp.mjs" check \
    --git-dir "$stamp_git_dir" --sha "$stamp_sha" --scope-file "$changed_file" 2>/dev/null)
  stamp_status=$?
  node "$repo_root/scripts/pre-push-floor-stamp.mjs" consume --git-dir "$stamp_git_dir" >/dev/null 2>&1 || true
  if [ "$stamp_status" = "0" ]; then
    scope_label="reused: scripts/push-with-floor.sh already ran the floor (${stamp_result#valid }s ago)"
    ran_steps="wrapper stamp"
    exit 0
  fi
fi

# ── 2. what that means ───────────────────────────────────────────────────────
scope_sh=$(node "$repo_root/scripts/pre-push-floor-scope.mjs" --shell <"$changed_file")
if [ -z "$scope_sh" ]; then
  echo "pre-push floor: could not derive a scope. Running nothing is not an option — run 'just check' before pushing." >&2
  failed_step="scope"
  exit 1
fi
eval "$scope_sh"

if [ "$force_full" = "1" ]; then
  FLOOR_FULL=1
  FLOOR_FULL_REASONS="no main ref to diff against"
  # Every scope, derived the same way the mapping derives it for a full push.
  eval "$(printf 'Cargo.lock\n' | node "$repo_root/scripts/pre-push-floor-scope.mjs" --shell)"
fi

if [ "$FLOOR_FULL" = "1" ]; then
  scope_label="full floor"
  echo "pre-push floor: full floor — ${FLOOR_FULL_REASONS}" >&2
else
  parts=""
  [ -n "$FLOOR_CHANGED_ROOT" ] && append parts " " "$FLOOR_CHANGED_ROOT"
  [ -n "$FLOOR_CHANGED_TAURI" ] && append parts " " "$FLOOR_CHANGED_TAURI"
  [ "$FLOOR_DESKTOP" = "1" ] && append parts " " "desktop"
  [ "$FLOOR_WEB" = "1" ] && append parts " " "web"
  [ "$FLOOR_MOBILE" = "1" ] && append parts " " "mobile"
  if [ -z "$parts" ]; then
    scope_label="no build surface in ${changed_count} changed file(s)"
  else
    scope_label="$parts"
  fi
fi

# ── 3. every skip, named with its reason, BEFORE anything runs ───────────────
# Computed here rather than after the steps: a run that fails at step two would
# otherwise print `skipped: nothing`, which is false and is exactly the silent
# skip this floor exists to remove. The scope is already known, so the list is
# knowable now.
[ "$FLOOR_DESKTOP" = "1" ] || append skipped ", " "desktop — no desktop/ change"
[ "$FLOOR_WEB" = "1" ] || append skipped ", " "web — no web/ change"
[ "$FLOOR_MOBILE" = "1" ] || append skipped ", " "mobile — no mobile/ change"
if [ "$FLOOR_FULL" != "1" ] && [ -z "$FLOOR_CHANGED_ROOT$FLOOR_CHANGED_TAURI" ]; then
  append skipped ", " "cargo — no crate changed"
else
  # Even the full floor lints every crate without running their tests.
  append skipped ", " "workspace tests — CI"
fi
append skipped ", " "e2e — CI"
# The desktop suite itself (7588 tests / 81 suites) is where `cargo test
# --workspace` was: CI only. A desktop/src/** push runs just the tests a
# changed file could plausibly break (desktopTestPlan); the full suite runs
# locally only when a desktop/** change fell outside desktop/src/**, where no
# narrower scope is honest (fullReasons names which path).
if [ "$FLOOR_DESKTOP" = "1" ] && [ "$FLOOR_FULL" != "1" ] && [ "$FLOOR_DESKTOP_TEST_FULL" != "1" ]; then
  append skipped ", " "desktop full suite — CI"
fi
if [ -n "$FLOOR_DESKTOP_UNTESTED" ]; then
  append notes " · " "no test beside: ${FLOOR_DESKTOP_UNTESTED}"
fi
if [ "$FLOOR_DESKTOP_TEST_FULL" = "1" ] && [ "$FLOOR_FULL" != "1" ] && [ -n "$FLOOR_DESKTOP_TEST_FULL_REASONS" ]; then
  append notes " · " "full desktop suite ran: ${FLOOR_DESKTOP_TEST_FULL_REASONS}"
fi
if [ -n "$FLOOR_UNMAPPED" ]; then
  append notes " · " "unmapped, so the full floor ran: ${FLOOR_UNMAPPED}"
fi

# ── 4. the steps ─────────────────────────────────────────────────────────────
run_cargo_fmt() {
  local manifest=$1 names=$2
  local args=() name
  for name in $names; do args+=(-p "$name"); done
  if [ -n "$manifest" ]; then
    cargo fmt --manifest-path "$manifest" "${args[@]}" -- --check
  else
    cargo fmt "${args[@]}" -- --check
  fi
}

run_cargo_clippy() {
  local manifest=$1 names=$2
  local args=() name
  for name in $names; do args+=(-p "$name"); done
  if [ -n "$manifest" ]; then
    cargo clippy --manifest-path "$manifest" "${args[@]}" --all-targets -- -D warnings
  else
    cargo clippy "${args[@]}" --all-targets -- -D warnings
  fi
}

# Runs the tests and counts them, so the summary can say "847 tests" rather
# than a number nobody measured. With no recognisable count in the output the
# summary says "tests" and no number — never a guess.
run_cargo_test() {
  local manifest=$1 names=$2
  local args=() log status=0 counted name
  for name in $names; do args+=(-p "$name"); done
  log=$(mktemp)
  if [ -n "$manifest" ]; then
    cargo test --manifest-path "$manifest" "${args[@]}" 2>&1 | tee "$log" || status=1
  else
    cargo test "${args[@]}" 2>&1 | tee "$log" || status=1
  fi
  counted=$(awk '/^test result:/ { for (i = 1; i < NF; i++) if ($(i + 1) ~ /^passed;?$/) total += $i } END { if (total > 0) print total }' "$log")
  if [ -n "$counted" ]; then
    if [ -n "$test_count" ]; then test_count=$((test_count + counted)); else test_count=$counted; fi
  fi
  rm -f "$log"
  return "$status"
}

# Runs exactly the desktop test files desktopTestPlan named (repo-relative
# paths, e.g. `desktop/src/app/AppShell.helpers.test.mjs`), mirroring
# `desktop/package.json`'s own `test` script but with an explicit file list
# instead of the `src/**/*.test.mjs` glob — that glob is the full suite this
# step exists to avoid.
run_desktop_tests_scoped() {
  local files=$1 relative=() f
  for f in $files; do relative+=("${f#desktop/}"); done
  (cd desktop && node --import ./test-loader.mjs --experimental-strip-types --test "${relative[@]}")
}

# `just mobile-test` runs `flutter test --reporter expanded`, which is
# line-oriented and never redraws over itself — but a failing run's summary
# is still buried inside however many hundred lines the full suite printed.
# Capture that output once (no second `flutter test` invocation — this floor
# is budget-conscious) and, only on failure, hand it to
# scripts/mobile-test-failure-summary.mjs so the disclosure names the test
# and its file:line instead of just "mobile tests FAILED" (item 208).
#
# A file that "Failed to load … Unable to connect to flutter_tester process:
# WebSocketException: Invalid WebSocket upgrade request" is not a failed test:
# no test in it ran. flutter_tester attaches to the tool over a fresh loopback
# listener, and the tool hands the FIRST connection to the WebSocket
# upgrader; on this Mac T3 Code's preview port scanner polls `lsof -sTCP:LISTEN`
# every 3 s and GETs every loopback listener it sees, so under load (slower
# flutter_tester startup, wider window) a few random files lose that race.
# Those files, and only those, are run once more; the first run's result is
# disclosed in the notes either way. A real assertion failure in the same run
# is never retried and still fails the step.
run_mobile_test() {
  local log status=0
  log=$(mktemp)
  just mobile-test 2>&1 | tee "$log" || status=1
  if [ "$status" != "0" ]; then
    local names load_failed failed_count load_count
    names=$(node "$repo_root/scripts/mobile-test-failure-summary.mjs" "$log" 2>/dev/null | tr '\n' ';' | sed 's/;$//; s/;/; /g')
    load_failed=$(node "$repo_root/scripts/mobile-test-failure-summary.mjs" --load-failures "$log" 2>/dev/null)
    failed_count=$(node "$repo_root/scripts/mobile-test-failure-summary.mjs" "$log" 2>/dev/null | grep -c .)
    load_count=$(printf '%s\n' "$load_failed" | grep -c .)
    if [ "$load_count" -gt 0 ] && [ "$load_count" = "$failed_count" ]; then
      local -a files=()
      while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$load_failed"
      echo "==> floor: ${load_count} mobile test file(s) never attached to flutter_tester (harness socket taken by a foreign HTTP probe); running those files once more" >&2
      if (unset GIT_DIR GIT_WORK_TREE; cd mobile && flutter test --reporter expanded "${files[@]}" 2>&1 | tee "$log"); then
        status=0
        append notes "; " "mobile: ${load_count} file(s) failed to load on the first run (flutter_tester harness socket probed), passed on rerun"
      else
        names=$(node "$repo_root/scripts/mobile-test-failure-summary.mjs" "$log" 2>/dev/null | tr '\n' ';' | sed 's/;$//; s/;/; /g')
        append notes "; " "mobile: ${load_count} file(s) failed to load twice"
      fi
    fi
    if [ "$status" != "0" ] && [ -n "$names" ]; then
      append notes "; " "$names"
    fi
  fi
  rm -f "$log"
  return "$status"
}

# The file-size ratchet stays unfiltered: its own merge-base diff is the path
# filter, and duplicating its governed roots here is the coverage drift
# lefthook.yml's header warns about. It is seconds, so it runs first, where a
# failure costs nothing.
step "file-size" just file-size-check

if [ "$FLOOR_FULL" = "1" ]; then
  # Everything is in scope, so name it that way rather than listing thirty-odd
  # `-p` flags. Tests are still not run here: `cargo test --workspace` is the
  # one thing this floor exists to keep out of a push.
  step "fmt (all)" cargo fmt --all -- --check
  step "tauri fmt (all)" cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --all -- --check
else
  [ -n "$FLOOR_CHANGED_ROOT" ] && step "fmt" run_cargo_fmt "" "$FLOOR_CHANGED_ROOT"
  [ -n "$FLOOR_CHANGED_TAURI" ] && step "tauri fmt" run_cargo_fmt desktop/src-tauri/Cargo.toml "$FLOOR_CHANGED_TAURI"
fi

if [ "$FLOOR_DESKTOP" = "1" ]; then
  step "biome" just desktop-check
  step "tsc" just desktop-typecheck
  if [ "$FLOOR_FULL" = "1" ] || [ "$FLOOR_DESKTOP_TEST_FULL" = "1" ]; then
    # Full floor, or a desktop/** change outside desktop/src/** (build
    # config/tooling) that the sibling rule cannot narrow honestly.
    step "desktop tests (full)" just desktop-test
  elif [ -n "$FLOOR_DESKTOP_TEST_FILES" ]; then
    # Named in the step itself: "print the list it ran" (batch3 L14 fix
    # round 1) rather than just a count nobody can check.
    listed=${FLOOR_DESKTOP_TEST_FILES//desktop\//}
    step "desktop tests (${listed})" run_desktop_tests_scoped "$FLOOR_DESKTOP_TEST_FILES"
  fi
  # else: every changed desktop/src/** file was untested (no sibling test) —
  # nothing to run, already disclosed via FLOOR_DESKTOP_UNTESTED above.
fi

# Tauri's build script refuses to compile when an externalBin placeholder is
# missing, so a checkout made before a sidecar joined the list (beekeeper-host)
# failed the floor here (2026-10-08) though `just desktop-tauri-clippy`, which
# depends on the stubs, passed. Same placeholders, created before any Tauri cargo
# step.
if [ "$FLOOR_FULL" = "1" ] || [ -n "$FLOOR_LINT_TAURI" ] || [ -n "$FLOOR_CHANGED_TAURI" ]; then
  step "sidecar stubs" just _ensure-sidecar-stubs
fi

if [ "$FLOOR_FULL" = "1" ]; then
  step "clippy (workspace)" cargo clippy --workspace --all-targets -- -D warnings
  step "tauri clippy (workspace)" cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
else
  [ -n "$FLOOR_LINT_ROOT" ] && step "clippy" run_cargo_clippy "" "$FLOOR_LINT_ROOT"
  [ -n "$FLOOR_LINT_TAURI" ] && step "tauri clippy" run_cargo_clippy desktop/src-tauri/Cargo.toml "$FLOOR_LINT_TAURI"

  [ -n "$FLOOR_CHANGED_ROOT" ] && step "tests" run_cargo_test "" "$FLOOR_CHANGED_ROOT"
  [ -n "$FLOOR_CHANGED_TAURI" ] && step "tauri tests" run_cargo_test desktop/src-tauri/Cargo.toml "$FLOOR_CHANGED_TAURI"
fi

if [ "$FLOOR_WEB" = "1" ]; then
  step "web tests" just web-test
fi

if [ "$FLOOR_MOBILE" = "1" ]; then
  step "mobile tests" run_mobile_test
fi

# Fold the measured test count into the step name now that it is known.
# `file-size` always runs first, so ", tests" cannot collide with the leading
# entry; matching on the separator keeps it off "desktop tests".
if [ -n "$test_count" ]; then
  before=$ran_steps
  ran_steps=${ran_steps/, tests/, ${test_count} tests}
  if [ "$ran_steps" = "$before" ]; then
    ran_steps=${ran_steps/, tauri tests/, ${test_count} tauri tests}
  fi
fi

[ -n "$failed_step" ] && exit 1
exit 0
