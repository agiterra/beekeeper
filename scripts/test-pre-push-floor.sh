#!/usr/bin/env bash
# End-to-end contract tests for the pre-push floor.
#
# The floor decides what a `git push` runs. Both ways of getting it wrong are
# expensive and neither is loud: run too much and the push blows a seat's 60 s
# tool timeout, which is how `--no-verify` became the house rule on 2026-09-02;
# run too little and a green push turns into a red CI run nobody was watching.
# So every case here is either "this ran and nothing else did" or "this was
# skipped and the summary said so".
#
# Hermetic: a throwaway git repo under $TMPDIR, with `cargo`, `pnpm`, `just`
# and `flutter` stubbed on PATH the way deploy/autodeploy/tests stub `incus`,
# `flock` and `sleep`. The package graph is injected through
# BUZZ_PRE_PUSH_FLOOR_GRAPH so `cargo metadata` is not one of the cargo
# invocations being counted. Only `node` is real — it is what runs the mapping
# under test. Seconds, no toolchain, no network.
set -uo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

pass=0
fail=0
note() { printf '  %s\n' "$*"; }
ok() { pass=$((pass + 1)); printf 'ok   %s\n' "$1"; }
bad() {
  fail=$((fail + 1))
  printf 'FAIL %s\n' "$1"
  shift
  for line in "$@"; do note "$line"; done
}

check_contains() { # check_contains <name> <haystack> <needle>
  case "$2" in
    *"$3"*) ok "$1" ;;
    *) bad "$1" "expected to contain: $3" "actual: $2" ;;
  esac
}

check_absent() { # check_absent <name> <haystack> <needle>
  case "$2" in
    *"$3"*) bad "$1" "expected NOT to contain: $3" "actual: $2" ;;
    *) ok "$1" ;;
  esac
}

check_equal() { # check_equal <name> <actual> <expected>
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "expected: $3" "actual:   $2"; fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# ── the stub toolchain ───────────────────────────────────────────────────────
# Every stub records its full argv, one line per invocation, and honours
# STUB_FAIL_MATCH so a failing step can be exercised.
mkdir -p "$work/bin"
for tool in cargo pnpm just flutter node_stub; do
  [ "$tool" = node_stub ] && continue
  cat >"$work/bin/$tool" <<STUB
#!/usr/bin/env bash
echo "$tool \$*" >>"\$STUB_LOG"
if [ "\${STUB_REFUSE_GIT_REPO_ENV:-}" = "1" ]; then
  for key in \$(git rev-parse --local-env-vars); do
    if env | grep -q "^\${key}="; then
      echo "$tool: inherited \${key} from the repository running the hook" >&2
      exit 97
    fi
  done
fi
if [ -n "\${STUB_FAIL_MATCH:-}" ] && [[ "$tool \$*" == *"\$STUB_FAIL_MATCH"* ]]; then
  echo "$tool: stubbed failure" >&2
  exit 1
fi
# A stubbed \`cargo test\` prints the shape the floor counts, so the summary's
# test count is exercised rather than assumed.
if [ "$tool" = cargo ] && [ "\${1:-}" = test ]; then
  echo "test result: ok. 847 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
fi
exit 0
STUB
  chmod +x "$work/bin/$tool"
done

# ── the injected package graph ───────────────────────────────────────────────
# Shaped like this repository's: beekeeper-cli is not the leaf it looks like
# (crates/beekeeper-dev-mcp/Cargo.toml:17 and crates/sprig both depend on it), and
# beekeeper-desktop lives in the second workspace.
cat >"$work/graph.json" <<'GRAPH'
{
  "packages": {
    "beekeeper-cli":          {"workspace": "root",  "dir": "crates/beekeeper-cli"},
    "beekeeper-core":         {"workspace": "root",  "dir": "crates/beekeeper-core"},
    "beekeeper-dev-mcp":      {"workspace": "root",  "dir": "crates/beekeeper-dev-mcp"},
    "sprig":             {"workspace": "root",  "dir": "crates/sprig"},
    "beekeeper-persona":      {"workspace": "root",  "dir": "crates/beekeeper-persona"},
    "beekeeper-desktop": {"workspace": "tauri", "dir": "desktop/src-tauri"}
  },
  "dependents": {
    "beekeeper-cli":          ["beekeeper-dev-mcp", "sprig"],
    "beekeeper-core":         ["beekeeper-desktop", "beekeeper-cli", "beekeeper-dev-mcp", "beekeeper-persona", "sprig"],
    "beekeeper-dev-mcp":      [],
    "sprig":             [],
    "beekeeper-persona":      ["beekeeper-cli"],
    "beekeeper-desktop": []
  }
}
GRAPH

# ── a throwaway repository the floor can diff ────────────────────────────────
scratch="$work/repo"
mkdir -p "$scratch/scripts"
cp "$repo_root/scripts/pre-push-floor.sh" "$repo_root/scripts/pre-push-floor-scope.mjs" "$scratch/scripts/"
mkdir -p "$scratch/crates/beekeeper-cli/src" "$scratch/desktop/src/app" "$scratch/docs"
: >"$scratch/justfile"
echo "seed" >"$scratch/crates/beekeeper-cli/src/lib.rs"
echo "seed" >"$scratch/desktop/src/app/App.tsx"
echo "seed" >"$scratch/docs/notes.md"
echo "seed" >"$scratch/crates/beekeeper-cli/src/doomed.rs"
echo '{"name":"beekeeper-desktop"}' >"$scratch/desktop/package.json"
# Real fixtures for the scoped desktop test step: `node` is the one real tool
# this suite never stubs, so a case that exercises `run_desktop_tests_scoped`
# needs an actually-runnable loader and test file, not a stub. Foo.tsx has a
# real sibling; App.tsx (seeded above) deliberately does not, for the
# no-sibling disclosure case.
echo "export {};" >"$scratch/desktop/test-loader.mjs"
echo "seed" >"$scratch/desktop/src/app/Foo.tsx"
cat >"$scratch/desktop/src/app/Foo.test.mjs" <<'FOOTEST'
import test from "node:test";
import assert from "node:assert/strict";
test("Foo's own sibling test passes", () => {
  assert.equal(1, 1);
});
FOOTEST

git -C "$scratch" init --quiet --initial-branch=main
git -C "$scratch" config user.email floor@example.com
git -C "$scratch" config user.name "Floor Test"
git -C "$scratch" config commit.gpgsign false
git -C "$scratch" add -A
git -C "$scratch" commit --quiet -m "seed"

# Each case gets its own branch off main so the merge-base diff is exactly the
# change under test.
make_branch() { # make_branch <name> <shell doing the edit>
  git -C "$scratch" checkout --quiet -B "$1" main
  (cd "$scratch" && eval "$2")
  git -C "$scratch" add -A
  git -C "$scratch" commit --quiet -m "$1"
}

run_floor() { # run_floor <branch> [STUB_FAIL_MATCH] [BUDGET]
  git -C "$scratch" checkout --quiet "$1"
  : >"$work/stub.log"
  (
    cd "$scratch" || exit 1
    PATH="$work/bin:$PATH" \
      STUB_LOG="$work/stub.log" \
      STUB_FAIL_MATCH="${2:-}" \
      BUZZ_PRE_PUSH_FLOOR_BUDGET_SECONDS="${3:-120}" \
      BUZZ_PRE_PUSH_FLOOR_GRAPH="$work/graph.json" \
      ./scripts/pre-push-floor.sh
  ) >"$work/stdout.txt" 2>"$work/stderr.txt"
  floor_status=$?
  # The summary is the line carrying the budget; the floor also prints
  # narrative lines with the same prefix.
  summary=$(grep '^pre-push floor: .*/ budget ' "$work/stderr.txt" | head -1)
  invocations=$(cat "$work/stub.log")
}

count_of() { grep -c "^$1 " "$work/stub.log" 2>/dev/null || true; }

echo "── 1. a CLI-only push runs the CLI's fmt, clippy and tests, and nothing else"
make_branch cli-only 'echo change >>crates/beekeeper-cli/src/lib.rs'
run_floor cli-only
check_equal "floor exits 0" "$floor_status" "0"
check_equal "cargo invoked exactly three times" "$(count_of cargo)" "3"
check_equal "pnpm invoked zero times" "$(count_of pnpm)" "0"
check_equal "flutter invoked zero times" "$(count_of flutter)" "0"
check_contains "fmt is scoped to the changed crate" "$invocations" "cargo fmt -p beekeeper-cli -- --check"
check_contains "clippy widens to the dependents cargo metadata names" "$invocations" \
  "cargo clippy -p beekeeper-cli -p beekeeper-dev-mcp -p sprig --all-targets -- -D warnings"
check_contains "tests stay on the changed crate" "$invocations" "cargo test -p beekeeper-cli"
check_absent "no workspace test run" "$invocations" "--workspace"
check_absent "just test-unit is gone" "$invocations" "just test-unit"
check_absent "just ci is not a hook" "$invocations" "just ci"
check_equal "the file-size ratchet still runs, unfiltered" \
  "$(grep -c '^just file-size-check' "$work/stub.log" || true)" "1"

echo "── 2. the summary line names the cost and every skip"
check_contains "summary names the scope" "$summary" "pre-push floor: beekeeper-cli ("
check_contains "summary carries the measured test count" "$summary" "847 tests"
check_contains "summary discloses the budget" "$summary" "/ budget 120s"
check_contains "summary names the desktop skip" "$summary" "desktop — no desktop/ change"
check_contains "summary names the web skip" "$summary" "web — no web/ change"
check_contains "summary names the mobile skip" "$summary" "mobile — no mobile/ change"
check_contains "summary names the workspace-test skip" "$summary" "workspace tests — CI"
check_contains "summary names the e2e skip" "$summary" "e2e — CI"
check_contains "summary ends by saying where the skips run" "$summary" "— run in CI"
check_equal "the summary goes to stderr, not stdout" \
  "$(grep -c 'pre-push floor' "$work/stdout.txt" || true)" "0"

echo "── 3. a desktop-only push runs the JS floor, its own sibling test, and no cargo"
# batch3 L14 fix round 1: the desktop step no longer runs the whole suite.
# Foo.tsx has a real sibling Foo.test.mjs (seeded above) — node is the one
# real tool this suite never stubs, so this exercises run_desktop_tests_scoped
# for real, not through a stub.
make_branch desktop-only 'echo change >>desktop/src/app/Foo.tsx'
run_floor desktop-only
check_equal "floor exits 0" "$floor_status" "0"
check_equal "cargo invoked zero times" "$(count_of cargo)" "0"
check_contains "biome runs" "$invocations" "just desktop-check"
check_contains "tsc runs" "$invocations" "just desktop-typecheck"
check_absent "the full desktop suite does NOT run" "$invocations" "just desktop-test"
check_contains "the file-size ratchet runs" "$invocations" "just file-size-check"
check_contains "summary names the cargo skip" "$summary" "cargo — no crate changed"
check_contains "summary discloses the full suite is CI-only" "$summary" "desktop full suite — CI"
check_contains "the step names exactly the sibling test it ran" "$summary"   "desktop tests (src/app/Foo.test.mjs)"
check_contains "the real node run actually executed the sibling test"   "$(cat "$work/stdout.txt")" "Foo's own sibling test passes"
check_absent "no mobile run" "$invocations" "just mobile-test"
check_absent "no web run" "$invocations" "just web-test"

echo "── 3b. a changed file with no sibling test is named, and nothing is run for it"
make_branch desktop-untested 'echo change >>desktop/src/app/App.tsx'
run_floor desktop-untested
check_equal "floor exits 0" "$floor_status" "0"
check_absent "the full desktop suite does NOT run" "$invocations" "just desktop-test"
check_absent "no scoped test step ran either — there was nothing to run"   "$(cat "$work/stderr.txt")" "desktop tests ("
check_contains "the untested file is named in the summary" "$(cat "$work/stderr.txt")"   "no test beside: desktop/src/app/App.tsx"
check_contains "biome and tsc still ran" "$invocations" "just desktop-check"

echo "── 3c. desktop config/tooling outside desktop/src/** forces the full suite"
make_branch desktop-tooling 'echo "// change" >>desktop/package.json'
run_floor desktop-tooling
check_contains "the full desktop suite runs" "$invocations" "just desktop-test"
check_contains "the step says it is the full run" "$summary" "desktop tests (full)"
check_contains "and names the reason" "$(cat "$work/stderr.txt")"   "desktop/package.json is desktop build/tooling config"

echo "── 4. a docs-only push runs the ratchet and says it found no build surface"
make_branch docs-only 'echo change >>docs/notes.md'
run_floor docs-only
check_equal "floor exits 0" "$floor_status" "0"
check_equal "cargo invoked zero times" "$(count_of cargo)" "0"
check_equal "just invoked once, for the ratchet" "$(count_of just)" "1"
check_contains "summary says no build surface changed" "$summary" "no build surface in 1 changed file(s)"

echo "── 5. the justfile, and anything unmapped, select the full floor"
make_branch justfile-change 'echo "# change" >>justfile'
run_floor justfile-change
check_contains "summary says full floor" "$summary" "pre-push floor: full floor ("
check_contains "the full floor lints both workspaces" "$invocations" \
  "cargo clippy --workspace --all-targets -- -D warnings"
check_contains "including the Tauri one" "$invocations" \
  "cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --workspace --all-targets -- -D warnings"
check_contains "the full floor runs the desktop surface" "$invocations" "just desktop-typecheck"
check_contains "the full floor runs the WHOLE desktop suite too, not a sibling guess" \
  "$invocations" "just desktop-test"
check_contains "the full floor runs mobile" "$invocations" "just mobile-test"
check_contains "the full floor still refuses the workspace test run" "$summary" "workspace tests — CI"
check_absent "even the full floor never runs cargo test" "$invocations" "cargo test"

make_branch unmapped 'mkdir -p some/new/surface && echo hi >some/new/surface/main.go'
run_floor unmapped
check_contains "an unmapped path selects the full floor" "$summary" "pre-push floor: full floor ("
check_contains "and the floor prints which path did it" \
  "$(cat "$work/stderr.txt")" "some/new/surface/main.go maps to no scope"

echo "── 6. a deleted file is still in the change set"
# lefthook 2.1.x drops deleted paths from push-file discovery, which is why
# lefthook.yml's header says deletion-only surface changes reach no local hook.
# The floor reads git itself, so it does not have that hole.
make_branch deletion 'git rm --quiet crates/beekeeper-cli/src/doomed.rs'
run_floor deletion
check_contains "a deletion under crates/ still selects that crate" "$invocations" "cargo test -p beekeeper-cli"

echo "── 7. a failing step exits non-zero and still prints the summary"
run_floor cli-only "clippy"
check_equal "floor exits non-zero" "$floor_status" "1"
check_contains "the summary is printed anyway" "$summary" "pre-push floor: beekeeper-cli ("
check_contains "the failing step is named" "$summary" "clippy FAILED"
check_contains "and the remedy names the rule, not a shortcut" \
  "$(cat "$work/stderr.txt")" "only on a SHA \`just ci\` already passed"
check_absent "the floor stops rather than running the tests anyway" "$invocations" "cargo test"
# A run that dies at step two used to print `skipped: nothing` — false, and the
# same silent skip this floor exists to remove.
check_contains "a failed run still names every skip" "$summary" "mobile — no mobile/ change"
check_contains "and still names the CI-only work" "$summary" "workspace tests — CI"

echo "── 8. going over budget discloses, it does not kill"
run_floor cli-only "" 0
check_equal "the floor still exits 0 — the budget never kills a run" "$floor_status" "0"
check_contains "the overrun is disclosed" "$summary" "over budget by"
check_contains "and the dominant step is named, not the budget raised" \
  "$(cat "$work/stderr.txt")" "the dominant step was"
check_contains "the real result is still reported" "$summary" "847 tests"

echo "── 9. a real git push runs the floor"
# Wired through core.hooksPath rather than lefthook so this stays hermetic;
# lefthook.yml's own wiring is asserted in case 9.
bare="$work/remote.git"
git init --quiet --bare "$bare"
mkdir -p "$scratch/.githooks"
cat >"$scratch/.githooks/pre-push" <<'HOOK'
#!/usr/bin/env bash
exec ./scripts/pre-push-floor.sh
HOOK
chmod +x "$scratch/.githooks/pre-push"
git -C "$scratch" config core.hooksPath .githooks
git -C "$scratch" checkout --quiet cli-only
: >"$work/stub.log"
push_out=$(
  cd "$scratch" && PATH="$work/bin:$PATH" STUB_LOG="$work/stub.log" \
    BUZZ_PRE_PUSH_FLOOR_GRAPH="$work/graph.json" \
    git push --quiet "$bare" cli-only 2>&1
)
check_contains "the push printed the floor's summary" "$push_out" "pre-push floor: beekeeper-cli ("
check_equal "and ran cargo exactly three times" "$(count_of cargo)" "3"

echo "── 9b. a push from a linked worktree does not leak its repository into tests"
# Git gives a linked-worktree hook an absolute GIT_DIR in the shared
# repository. That pointer used to survive into `cargo test`, where a fixture's
# `git -C <temp>` consequently committed to and reconfigured the real branch.
linked="$work/linked"
git -C "$scratch" worktree add --quiet -b linked-hook "$linked" cli-only
git -C "$scratch" config core.hooksPath "$scratch/.githooks"
: >"$work/stub.log"
push_out=$(
  cd "$linked" && PATH="$work/bin:$PATH" STUB_LOG="$work/stub.log" \
    STUB_REFUSE_GIT_REPO_ENV=1 \
    BUZZ_PRE_PUSH_FLOOR_GRAPH="$work/graph.json" \
    git push --quiet "$bare" linked-hook 2>&1
)
push_status=$?
check_equal "the linked-worktree push exits 0" "$push_status" "0"
check_contains "its hook printed the floor's summary" "$push_out" "pre-push floor: beekeeper-cli ("
check_equal "all three cargo steps received a neutral git environment" "$(count_of cargo)" "3"

git -C "$scratch" config --unset core.hooksPath
: >"$work/stub.log"
STUB_FAIL_MATCH="" # reset

echo "── 10. lefthook.yml wires the guards unconditionally and the floor once"
hook_block=$(awk '/^pre-push:/ { inblock = 1 } inblock { print }' "$repo_root/lefthook.yml")
check_contains "push-destination stays a script" "$hook_block" 'push-destination.sh'
check_contains "branch-skew stays a command" "$hook_block" './scripts/check-branch-skew.sh'
check_contains "the floor is wired" "$hook_block" './scripts/pre-push-floor.sh'
check_absent "no glob may gate a pre-push entry" "$hook_block" "glob:"
check_absent "just test-unit is no longer a hook" "$hook_block" "test-unit"
check_absent "the Tauri clippy+test pair is no longer a hook" "$hook_block" "desktop-tauri-clippy"
check_absent "mobile tests are no longer an unscoped hook row" "$hook_block" "mobile-test"

echo "── 11. lefthook itself runs all three, with no glob standing in the way"
# `lefthook run pre-push` executes the hook without installing anything, so
# this stays a throwaway repo and never touches the developer's .git config.
if command -v lefthook >/dev/null 2>&1; then
  cp "$repo_root/lefthook.yml" "$scratch/lefthook.yml"
  mkdir -p "$scratch/.lefthook/pre-push"
  cp "$repo_root/.lefthook/pre-push/push-destination.sh" "$scratch/.lefthook/pre-push/"
  cp "$repo_root/scripts/check-branch-skew.sh" "$scratch/scripts/"
  git -C "$scratch" add -A
  git -C "$scratch" commit --quiet -m "wire lefthook"
  : >"$work/stub.log"
  lefthook_out=$(
    cd "$scratch" && PATH="$work/bin:$PATH" STUB_LOG="$work/stub.log" \
      BUZZ_PRE_PUSH_FLOOR_GRAPH="$work/graph.json" \
      lefthook run pre-push 2>&1
  )
  check_contains "lefthook ran the floor" "$lefthook_out" "pre-push floor:"
  check_contains "lefthook ran the destination tripwire" "$lefthook_out" "push-destination"
  check_contains "lefthook ran the skew guard" "$lefthook_out" "branch-skew"
else
  # Named rather than silently passed: a skip nobody can see is the failure
  # mode this whole lane exists to remove.
  note "SKIPPED: lefthook is not on PATH, so its own wiring was not exercised"
fi

echo
printf '%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
