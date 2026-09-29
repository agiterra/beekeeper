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
  # ── Woodpecker ────────────────────────────────────────────────────────────
  *"count(*) from pipelines"*)        echo "${STUB_REPO_ROWS-9}" ;;
  *"docker cp woodpecker-server-1"*)  echo "$STUB_PIPELINE_ROW" ;;
  # ── mirror ────────────────────────────────────────────────────────────────
  *"rev-parse --git-dir"*)
      if [[ "${STUB_MIRROR_READABLE:-1}" == "1" ]]; then
        echo "."
      else
        echo "fatal: detected dubious ownership in repository at '/srv/git/x.git'" >&2
        exit 128
      fi ;;
  *"cat-file -e"*)                    exit "${STUB_COMMIT_PRESENT:-0}" ;;
  *"git-mirror.service"*)             : >"$STUB_DIR/mirror-triggered" ;;
  # ── instance ──────────────────────────────────────────────────────────────
  *"cat "*"/compose/.env"*)
      # Model the file, not the extraction: the script greps locally so it can
      # tell "unreadable instance" from "no BUZZ_IMAGE line".
      if [[ "${STUB_ENV_READABLE-1}" == "1" ]]; then
        echo "BUZZ_IMAGE=${STUB_IMAGE_NAME-test-relay}:${STUB_CURRENT-}"
      else
        exit 1
      fi ;;
  *"autodeploy-failed-"*)             exit 1 ;;   # no failure marker
  *"docker build"*)                   : >"$STUB_DIR/built" ;;
  *"State.Health.Status"*)            echo "${STUB_HEALTH-healthy}" ;;
  *"docker builder prune"*)           : >"$STUB_DIR/cache-pruned" ;;
  # Anything else is a deploy-path side effect (tar, pg_dump, run.sh, the
  # retention globs). Record it so an unmodelled call is visible rather than
  # silently succeeding — that is how the .env read drifted out from under
  # this stub and reported "deployed is unknown" instead of failing.
  *)                                  echo "$args" >> "$STUB_DIR/unmodelled" ;;
esac
STUB
# flock is util-linux — absent on macOS. Without this stub these tests pass in
# CI and fail on a developer laptop, which is the worst of both.
printf '#!/usr/bin/env bash\nexit 0\n'            >"$tmp/bin/flock"
printf '#!/usr/bin/env bash\nexit 0\n'            >"$tmp/bin/sleep"
chmod +x "$tmp/bin/incus" "$tmp/bin/flock" "$tmp/bin/sleep"

export PATH="$tmp/bin:$PATH"
export INCUS_LOG="$tmp/incus.log"
export STUB_DIR="$tmp"
mkdir -p "$tmp/lock"

# Valid configuration; individual cases override single values. LOCK_DIR keeps
# the suite hermetic — without it every run drops a lock file in the real /tmp.
base_env=(
  AUTODEPLOY_NAME=testrelay
  REPO_ID=7
  BRANCH=main
  MIRROR=/srv/git/test.git
  INSTANCE=testinst
  BASE=/opt/test
  IMAGE_NAME=test-relay
  LOCK_DIR="$tmp/lock"
)

run() {  # run <extra env>... -- captures stdout+stderr, never aborts the suite
  : >"$INCUS_LOG"
  rm -f "$tmp/built" "$tmp/mirror-triggered" "$tmp/unmodelled" "$tmp/cache-pruned"
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
for bad in 'REPO_ID=' 'REPO_ID=0' 'REPO_ID=abc' 'REPO_ID=1;drop' 'MIRROR=' 'INSTANCE=' 'BASE=' 'IMAGE_NAME=' 'AUTODEPLOY_NAME=' \
           'KEEP_BUILD_CACHE=0GB' 'KEEP_BUILD_CACHE=10GB;rm'; do
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
[[ ! -e "$tmp/built" ]]                     || fail "must not build on a non-green pipeline"

# ── 3. already current: no build, but it must still say what it decided ──────
# Deliberately NOT asserting silence. A quiet tick and a deployer broken into
# permanent silence are indistinguishable from outside, which is how the
# safe.directory fault stayed invisible; requiring silence here would have
# locked that in as a feature.
run STUB_PIPELINE_ROW="success 2222222222222222222222222222222222222222" STUB_CURRENT=222222222
[[ $rc -eq 0 ]]                             || fail "current relay should exit 0, got $rc"
[[ ! -e "$tmp/built" ]]                     || fail "must not build when already current"
grep -q "up to date" <<<"$out"              || fail "a quiet tick must still report its decision"
grep -q "rev-parse --git-dir" "$INCUS_LOG"  || fail "the mirror must be proved readable BEFORE the up-to-date exit, or a config fault is only ever discovered on a tick that wants to deploy"

# ── 4. an unreadable mirror is FATAL, not a sync delay ───────────────────────
# The regression test for the bug that would have made the deployer useless
# forever. It must NOT reach for git-mirror.service: an unreadable repo is a
# configuration fault, and triggering a sync would paper over it.
run STUB_PIPELINE_ROW="success 3333333333333333333333333333333333333333" \
    STUB_CURRENT=999999999 STUB_MIRROR_READABLE=0
[[ $rc -ne 0 ]]                                   || fail "unreadable mirror must not exit 0"
grep -q "FATAL" <<<"$out"                         || fail "unreadable mirror must be FATAL"
grep -q "configuration fault" <<<"$out"           || fail "should name it a configuration fault"
grep -q "dubious ownership" <<<"$out"             || fail "should echo git's actual error"
grep -q "not in mirror yet" <<<"$out"             && fail "must not report a config fault as a sync delay"
[[ ! -e "$tmp/mirror-triggered" ]]                || fail "must not trigger a sync for an unreadable mirror"

# ── 5. zero rows is a fault, not a quiet exit ────────────────────────────────
# This branch used to log a benign line and exit 0, swallowing a REPO_ID that
# names no repo and a retired BRANCH — both permanent, neither self-resolving.
run STUB_PIPELINE_ROW="" STUB_CURRENT=999999999
[[ $rc -ne 0 ]]                             || fail "no pipeline row must be fatal, got exit 0"
grep -q "none on branch" <<<"$out"          || fail "should name the retired-branch case"
run STUB_PIPELINE_ROW="" STUB_CURRENT=999999999 STUB_REPO_ROWS=0
[[ $rc -ne 0 ]]                             || fail "no pipelines for REPO_ID must be fatal"
grep -q "no pipelines at all" <<<"$out"     || fail "should name the wrong-repo_id case"

# ── 6. an unreadable instance is not an undeployed one ───────────────────────
# `|| true` on the .env read used to mean "nothing is deployed" even when the
# instance was down — which leads straight to a 15-minute build.
run STUB_PIPELINE_ROW="success 6666666666666666666666666666666666666666" STUB_ENV_READABLE=0
[[ $rc -ne 0 ]]                             || fail "unreadable instance must be fatal"
grep -q "unreadable relay" <<<"$out"        || fail "should refuse to treat an unreadable relay as undeployed"
[[ ! -e "$tmp/built" ]]                     || fail "must not build when the relay state is unknown"

# ── 7. the happy path completes, and prunes ──────────────────────────────────
# The previous version of this suite never asserted an exit code here, so it
# silently exercised the rollback branch and passed anyway.
run STUB_PIPELINE_ROW="success 8888888888888888888888888888888888888888" STUB_CURRENT=999999999
[[ $rc -eq 0 ]]                             || fail "happy path should exit 0, got $rc"
[[ -e "$tmp/built" ]]                       || fail "happy path must build"
grep -q "DEPLOYED" <<<"$out"                || fail "happy path must report DEPLOYED"
grep -q "retention done" <<<"$out"          || fail "retention must run on the success path"
grep -q "docker builder prune -f --keep-storage 10GB" "$INCUS_LOG" \
                                             || fail "retention must cap the BuildKit cache; image removal frees none of it"
grep -q "repo_id = 7" "$INCUS_LOG"          || fail "query must pin repo_id; log: $(head -1 "$INCUS_LOG")"
grep -q "wp-testrelay.sqlite" "$INCUS_LOG"  || fail "scratch sqlite path must be per-target, or concurrent runs race"
# The Dockerfile compiles this in as the relay's disclosed NIP-11
# `software_commit` / `GET /health` build identity (finding 32). `git
# archive` never includes `.git`, so nothing inside the image build can
# discover the commit on its own — this build-arg is the only place on this
# path that still knows it, and it must be the full sha (bee git check --ref
# needs the whole object name), not the short one used for the image tag.
grep -q "docker build --build-arg BUZZ_SOURCE_SHA=8888888888888888888888888888888888888888 " "$INCUS_LOG" \
                                             || fail "docker build must pass --build-arg BUZZ_SOURCE_SHA=<full sha>; log: $(grep 'docker build' "$INCUS_LOG")"
grep -q "docker build .* -t test-relay:888888888 " "$INCUS_LOG" \
                                             || fail "docker build must tag with the short sha; log: $(grep 'docker build' "$INCUS_LOG")"
# The ordinal of that same commit, for NIP-11 `software_commit_count`. The
# stub mirror answers `rev-list --count` with nothing, so this asserts the
# *shape* — the arg is always passed, and carries either a plain positive
# decimal or the empty string. Never `0`, and never a word: both would
# subtract as data on the client rather than reading as absent.
grep -qE "docker build .*--build-arg BUZZ_SOURCE_COMMIT_COUNT=([1-9][0-9]*)? " "$INCUS_LOG" \
                                             || fail "docker build must pass --build-arg BUZZ_SOURCE_COMMIT_COUNT=<positive decimal or empty>; log: $(grep 'docker build' "$INCUS_LOG")"

# ── 8. an unhealthy relay rolls back ─────────────────────────────────────────
run STUB_PIPELINE_ROW="success 9999999999999999999999999999999999999999" STUB_CURRENT=111111111 STUB_HEALTH=unhealthy
[[ $rc -ne 0 ]]                             || fail "an unhealthy relay must not report success"
grep -q "ROLLING BACK" <<<"$out"            || fail "should roll back on an unhealthy relay"
[[ ! -e "$tmp/cache-pruned" ]]              || fail "a failed deploy must not prune the build cache"

echo "autodeploy behavior tests passed"
