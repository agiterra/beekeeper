#!/usr/bin/env bash
# Behavior tests for deploy/autodeploy/autodeploy.
#
# The script's only effects on the world go through `incus`, `flock`, `sleep`
# and `date`, so stubbing those on PATH exercises the whole decision tree with
# no host, no containers and no Woodpecker.
#
# Every case here is a regression test for something that actually happened.
# The most important is the last one: an unreadable mirror must be FATAL and
# say so, because when it silently reported "commit not in mirror yet" instead,
# the deployer became a permanent no-op that looked exactly like a healthy idle
# one — green timer, green service, healthy relay, and an update that would
# never arrive.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
script="$repo_root/deploy/autodeploy/autodeploy"
[[ -x "$script" ]] || { echo "not executable: $script" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

# ── stubs ────────────────────────────────────────────────────────────────────
# `incus` dispatches on the command text it is asked to run. Each stub call is
# appended to $INCUS_LOG so tests can assert on what was and was not attempted.
cat >"$tmp/bin/incus" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$INCUS_LOG"
args="$*"
case "$args" in
  *"docker cp woodpecker-server-1"*)  echo "$STUB_PIPELINE_ROW" ;;
  *"grep -oE '^BUZZ_IMAGE="*)         echo "$STUB_CURRENT" ;;
  *"rev-parse --git-dir"*)
      if [[ "${STUB_MIRROR_READABLE:-1}" == "1" ]]; then
        echo "."
      else
        echo "fatal: detected dubious ownership in repository at '/srv/git/x.git'" >&2
        exit 128
      fi ;;
  *"cat-file -e"*)                    exit "${STUB_COMMIT_PRESENT:-0}" ;;
  *"test -e"*)                        exit 1 ;;   # no failure marker
  *)                                  exit 0 ;;
esac
STUB
# flock is util-linux — absent on macOS. Without this stub these tests pass in
# CI and fail on a developer laptop, which is the worst of both.
printf '#!/usr/bin/env bash\nexit 0\n'            >"$tmp/bin/flock"
printf '#!/usr/bin/env bash\nexit 0\n'            >"$tmp/bin/sleep"
chmod +x "$tmp/bin/incus" "$tmp/bin/flock" "$tmp/bin/sleep"

export PATH="$tmp/bin:$PATH"
export INCUS_LOG="$tmp/incus.log"

# Valid configuration; individual cases override single values.
base_env=(
  AUTODEPLOY_NAME=testrelay
  REPO_ID=7
  MIRROR=/srv/git/test.git
  INSTANCE=testinst
  BASE=/opt/test
  IMAGE_NAME=test-relay
)

run() {  # run <extra env>... -- captures stdout+stderr, never aborts the suite
  : >"$INCUS_LOG"
  set +e
  out=$(env "${base_env[@]}" "$@" "$script" 2>&1)
  rc=$?
  set -e
}

fail() { echo "FAIL: $*" >&2; echo "--- output ---" >&2; echo "$out" >&2; exit 1; }

# ── 1. invalid config is fatal, before anything touches the host ─────────────
# REPO_ID="" is the dangerous one: set-but-empty slips past `set -u`, and the
# resulting SQL `where repo_id =  and ...` is a syntax error that yields an
# empty row, which the "no usable pipeline row" branch would treat as a normal
# quiet exit.
for bad in 'REPO_ID=' 'REPO_ID=0' 'REPO_ID=abc' 'REPO_ID=1;drop' 'MIRROR=' 'INSTANCE=' 'BASE=' 'IMAGE_NAME=' 'AUTODEPLOY_NAME='; do
  run "$bad"
  [[ $rc -eq 2 ]]                 || fail "$bad should exit 2, got $rc"
  grep -q "FATAL" <<<"$out"       || fail "$bad should say FATAL"
  [[ ! -s "$INCUS_LOG" ]]         || fail "$bad reached the host before validating: $(cat "$INCUS_LOG")"
done

# Relative paths are rejected too — BASE is interpolated into rm -rf and mv.
for bad in 'MIRROR=relative/path' 'BASE=relative/path'; do
  run "$bad"
  [[ $rc -eq 2 ]] || fail "$bad should exit 2, got $rc"
done

# ── 2. a still-building pipeline does not deploy ─────────────────────────────
run STUB_PIPELINE_ROW="running 1111111111111111111111111111111111111111" STUB_CURRENT=999999999
[[ $rc -eq 0 ]]                             || fail "running pipeline should exit 0, got $rc"
grep -q "is 'running'" <<<"$out"            || fail "should report the non-success status"
grep -q "docker build" "$INCUS_LOG"         && fail "must not build on a non-green pipeline"

# ── 3. already current is a silent no-op ─────────────────────────────────────
run STUB_PIPELINE_ROW="success 2222222222222222222222222222222222222222" STUB_CURRENT=222222222
[[ $rc -eq 0 ]]                             || fail "current relay should exit 0, got $rc"
[[ -z "$out" ]]                             || fail "current relay should print nothing, got: $out"

# ── 4. an unreadable mirror is FATAL, not a sync delay ───────────────────────
# The regression test for the bug that would have made the deployer useless
# forever. Note it must NOT reach for git-mirror.service: an unreadable repo is
# a configuration fault, and triggering a sync would paper over it.
run STUB_PIPELINE_ROW="success 3333333333333333333333333333333333333333" \
    STUB_CURRENT=999999999 STUB_MIRROR_READABLE=0
[[ $rc -eq 1 ]]                                   || fail "unreadable mirror should exit 1, got $rc"
grep -q "FATAL" <<<"$out"                         || fail "unreadable mirror must be FATAL"
grep -q "configuration fault" <<<"$out"           || fail "should name it a configuration fault"
grep -q "dubious ownership" <<<"$out"             || fail "should echo git's actual error"
grep -q "commit not in mirror yet" <<<"$out"      && fail "must not report a config fault as a sync delay"
grep -q "git-mirror.service" "$INCUS_LOG"         && fail "must not trigger a sync for an unreadable mirror"

# ── 5. the query is pinned to the configured repo ────────────────────────────
run STUB_PIPELINE_ROW="success 4444444444444444444444444444444444444444" STUB_CURRENT=999999999 STUB_COMMIT_PRESENT=0
grep -q "repo_id = 7" "$INCUS_LOG"          || fail "query must pin repo_id; log: $(head -1 "$INCUS_LOG")"
grep -q "wp-testrelay.sqlite" "$INCUS_LOG"  || fail "scratch sqlite path must be per-target, or concurrent runs race"

echo "autodeploy behavior tests passed"
