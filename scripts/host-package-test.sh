#!/usr/bin/env bash
# =============================================================================
# host-package-test.sh — does the packager do what it says?
# =============================================================================
#
#   scripts/host-package-test.sh
#       The fixture half. Compiles one tiny C program as eight fake binaries,
#       packages them, and proves each refusal. Needs only cc. About a minute.
#
#   scripts/host-package-test.sh --source-app <built bundle> \
#       [--expect-commit <rev>] [--identity <name|->] [--gui]
#       Also packages real binaries and proves how they behave in the package.
#
# What the real half never does, on purpose:
#   - Start a real provider, or anything that reaches a relay. The one host it
#     runs (probe 6) supervises a fake provider, in a throwaway home, against an
#     unresolvable relay, with a test key whose secret is the number 1.
#   - Touch launchd. HOME and BEEKEEPER_HOST_HOME are always two different temp
#     directories, because `home_is_ours` compares them: were they equal, `bee
#     host install` would boot out and replace this user's live host. Probe 5
#     compares launchd's view before and after.
#   - Touch the source bundle, a real key, or any real state directory.
#
# --gui runs the menu bar itself for a few seconds, exactly as launchd would: its
# icon and a Manage Host window appear on screen, then it is terminated. It reads
# only a socket in the throwaway home. Left off by default for that reason.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/host-package-lib.sh
source "$SCRIPT_DIR/host-package-lib.sh"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
PACKAGE="$SCRIPT_DIR/host-package.sh"
VERIFY="$SCRIPT_DIR/host-package-verify.sh"

SOURCE_APP="" EXPECT_COMMIT="" IDENTITY=- GUI=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-app) SOURCE_APP="${2:?}"; shift 2 ;;
        --expect-commit) EXPECT_COMMIT="${2:?}"; shift 2 ;;
        --identity) IDENTITY="${2:?}"; shift 2 ;;
        --gui) GUI=true; shift ;;
        -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

# /tmp rather than $TMPDIR: an AF_UNIX socket path is limited to 104 bytes.
T=$(mktemp -d /tmp/bkhostpkg.XXXXXX)
RUNNING_PIDS=()
cleanup() {
    local pid
    for pid in ${RUNNING_PIDS[@]+"${RUNNING_PIDS[@]}"}; do
        kill "$pid" 2>/dev/null
        wait "$pid" 2>/dev/null
    done
    # Only the directory this run made.
    case "$T" in /tmp/bkhostpkg.*) rm -rf "$T" ;; esac
}
trap cleanup EXIT

PASSED=0 FAILED=0
pass() { echo "  ok    $*"; PASSED=$((PASSED + 1)); }
fail() { echo "  FAIL  $*"; FAILED=$((FAILED + 1)); }
step() { echo; echo "== $*"; }
# expect_refusal <description> <text the refusal must contain> <command...>
expect_refusal() {
    local what="$1" want="$2" output status
    shift 2
    output=$("$@" 2>&1)
    status=$?
    if [[ $status -ne 0 ]] && grep -qF -- "$want" <<<"$output"; then
        pass "$what"
    else
        fail "$what (exit $status): $(tail -3 <<<"$output")"
    fi
}

HELPERS=()
while IFS= read -r helper; do HELPERS+=("$helper"); done < <(standalone_host_helpers)

# ── the fixture ──────────────────────────────────────────────────────────────
step "Fixture: a source bundle of eight compiled fakes"
FIXTURE_COMMIT=cafef00d
cat >"$T/fake.c" <<'C'
#include <stdio.h>
int main(void) { printf("%s 0.1.0 (%s)\n", NAME, COMMIT); return 0; }
C
SRC="$T/source/Source.app"
MENUBAR_BUNDLE="$SRC/Contents/Library/LoginItems/Beekeeper Menu Bar.app"
mkdir -p "$SRC/Contents/MacOS" "$SRC/Contents/Resources/personas" \
    "$MENUBAR_BUNDLE/Contents/MacOS" "$MENUBAR_BUNDLE/Contents/Resources"
build_fake() {
    cc -DNAME="\"$1\"" -DCOMMIT="\"$FIXTURE_COMMIT\"" "$T/fake.c" -o "$2" || {
        echo "cc is needed for the fixture" >&2
        exit 1
    }
}
for helper in "${HELPERS[@]}"; do build_fake "$helper" "$SRC/Contents/MacOS/$helper"; done
# Desktop-only siblings, which the package must leave behind.
build_fake beekeeper-desktop "$SRC/Contents/MacOS/beekeeper-desktop"
build_fake beekeeper-shell-host "$SRC/Contents/MacOS/beekeeper-shell-host"
build_fake "$STANDALONE_HOST_MAIN" "$MENUBAR_BUNDLE/Contents/MacOS/$STANDALONE_HOST_MAIN"
cp -R "$REPO/personas/roles" "$REPO/personas/templates" "$SRC/Contents/Resources/personas/"
cp "$REPO/desktop/src-tauri/icons/icon.icns" "$MENUBAR_BUNDLE/Contents/Resources/icon.icns"
cat >"$MENUBAR_BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleShortVersionString</key><string>9.9.9</string>
<key>LSMinimumSystemVersion</key><string>10.13</string>
<key>LSUIElement</key><true/>
</dict></plist>
PLIST

GOOD="$T/good/$STANDALONE_HOST_APP"
SRC_TREE_BEFORE=$(standalone_host_tree_sha "$SRC")
if "$PACKAGE" --from-app "$SRC" --out "$T/good" --identity - --expect-commit "$FIXTURE_COMMIT" --archive >"$T/good.log" 2>&1; then
    pass "packages a complete source"
else
    fail "did not package a complete source: $(tail -5 "$T/good.log")"
    echo "cannot continue without a package"; exit 1
fi

step "What arrives, and what stays behind"
[[ ! -e "$GOOD/Contents/MacOS/beekeeper-desktop" && ! -e "$GOOD/Contents/MacOS/beekeeper-shell-host" ]] \
    && pass "the Desktop executable and shell host stay behind" || fail "a Desktop binary came along"
[[ ! -e "$GOOD/Contents/Library" ]] && pass "nothing is nested" || fail "Contents/Library arrived"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$GOOD/Contents/Info.plist")" == "$STANDALONE_HOST_MAIN" ]] \
    && pass "the menu bar is the outer executable" || fail "the outer executable is wrong"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$GOOD/Contents/Info.plist")" == 9.9.9 ]] \
    && pass "the version is the source menu bar's, with no check against a Desktop's" || fail "wrong version"
[[ "$(standalone_host_tree_sha "$SRC")" == "$SRC_TREE_BEFORE" ]] \
    && pass "the source bundle is untouched" || fail "the source bundle changed"
[[ -z "$(find "$T/good" -maxdepth 1 -name '.stage.*' -print -quit)" ]] \
    && pass "no staging directory is left behind" || fail "a staging directory survived"
"$VERIFY" "$GOOD" >"$T/verify.log" 2>&1 && pass "verify passes, with the receipt" || fail "verify failed: $(grep FAIL "$T/verify.log")"

step "The archive unpacks to the same verified bundle"
mkdir -p "$T/unzipped"
ditto -x -k "$T/good/"*.zip "$T/unzipped" 2>/dev/null
"$VERIFY" --no-receipt "$T/unzipped/$STANDALONE_HOST_APP" >"$T/verify-zip.log" 2>&1 \
    && pass "verify passes on the unpacked archive" || fail "the unpacked archive fails: $(grep FAIL "$T/verify-zip.log")"
[[ "$(standalone_host_tree_sha "$T/unzipped/$STANDALONE_HOST_APP")" == "$(standalone_host_tree_sha "$GOOD")" ]] \
    && pass "same tree digest" || fail "the archive's tree differs"

step "An ad-hoc package reproduces from the same inputs"
"$PACKAGE" --from-app "$SRC" --out "$T/again" --identity - >/dev/null 2>&1
[[ "$(standalone_host_tree_sha "$T/again/$STANDALONE_HOST_APP")" == "$(standalone_host_tree_sha "$GOOD")" ]] \
    && pass "byte-identical tree on a second run" || fail "two runs differ"

step "Instance and bundle id go together"
"$PACKAGE" --from-app "$SRC" --out "$T/dev" --identity - --instance dev >/dev/null 2>&1
PL="$T/dev/$STANDALONE_HOST_APP/Contents/Info.plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PL")" == io.agiterra.beekeeper.host.dev \
    && "$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:BEEKEEPER_HOST_INSTANCE' "$PL")" == dev ]] \
    && pass "--instance dev sets both" || fail "--instance dev did not"
expect_refusal "an unknown instance" "must be production or dev" \
    "$PACKAGE" --from-app "$SRC" --out "$T/x" --identity - --instance staging

step "The packager refuses"
source_without() {
    rm -rf "$T/variant"; mkdir -p "$T/variant"; ditto "$SRC" "$T/variant/Source.app"; rm -rf "$T/variant/Source.app/$1"
}
source_without Contents/MacOS/beekeeper-session-provider
expect_refusal "a source with no provider" "missing from the source: beekeeper-session-provider" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
source_without "Contents/Library/LoginItems/Beekeeper Menu Bar.app"
expect_refusal "a source with no menu bar" "no menu bar Info.plist" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
source_without Contents/Resources/personas/templates
expect_refusal "a source with no templates" "no personas/templates" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
rm -rf "$T/variant"; mkdir -p "$T/variant"; ditto "$SRC" "$T/variant/Source.app"
/usr/libexec/PlistBuddy -c 'Set :LSUIElement false' \
    "$T/variant/Source.app/Contents/Library/LoginItems/Beekeeper Menu Bar.app/Contents/Info.plist"
expect_refusal "a menu bar that would take a Dock icon" "not LSUIElement" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
expect_refusal "the wrong commit" "not the expected" \
    "$PACKAGE" --from-app "$SRC" --out "$T/x" --identity - --expect-commit deadbeef
rm -rf "$T/variant"; mkdir -p "$T/variant"; ditto "$SRC" "$T/variant/Source.app"
FIXTURE_COMMIT=unknown build_fake bee "$T/variant/Source.app/Contents/MacOS/bee"
expect_refusal "a bee that names no commit" "names no commit" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
rm -rf "$T/variant"; mkdir -p "$T/variant"; ditto "$SRC" "$T/variant/Source.app"
FIXTURE_COMMIT="$FIXTURE_COMMIT-dirty" build_fake bee "$T/variant/Source.app/Contents/MacOS/bee"
expect_refusal "a dirty bee, unless asked" "--allow-dirty" \
    "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/x" --identity -
if "$PACKAGE" --from-app "$T/variant/Source.app" --out "$T/dirty" --identity - --allow-dirty >"$T/dirty.log" 2>&1 \
    && grep -q '"dirty": true' "$T/dirty/$STANDALONE_HOST_APP.receipt.json" \
    && grep -q '^INFO  DIRTY' <<<"$("$VERIFY" "$T/dirty/$STANDALONE_HOST_APP" 2>&1)"; then
    pass "--allow-dirty packages it, and the receipt and the verifier both say so"
else
    fail "--allow-dirty: $(tail -3 "$T/dirty.log")"
fi
expect_refusal "staging into an Applications directory" "never installs" \
    "$PACKAGE" --from-app "$SRC" --out "$T/home/Applications/here" --identity -
expect_refusal "two sources at once" "exactly one" \
    "$PACKAGE" --from-app "$SRC" --build --out "$T/x" --identity -
expect_refusal "replacing an output without --force" "pass --force" \
    "$PACKAGE" --from-app "$SRC" --out "$T/good" --identity -
mkdir -p "$T/stranger/$STANDALONE_HOST_APP"; echo mine >"$T/stranger/$STANDALONE_HOST_APP/keep"
expect_refusal "--force over something this script did not make" "left alone" \
    "$PACKAGE" --from-app "$SRC" --out "$T/stranger" --identity - --force
[[ -f "$T/stranger/$STANDALONE_HOST_APP/keep" ]] && pass "the stranger's file survives" || fail "--force deleted a stranger's file"
"$PACKAGE" --from-app "$SRC" --out "$T/good" --identity - --force >/dev/null 2>&1 \
    && pass "--force replaces its own output" || fail "--force did not replace its own output"

step "The verifier refuses a package that has been changed"
mutated() { rm -rf "$T/mut"; mkdir -p "$T/mut"; ditto "$GOOD" "$T/mut/$STANDALONE_HOST_APP"; MUT="$T/mut/$STANDALONE_HOST_APP"; }
expect_verify_fail() {
    local what="$1" want="$2" output status
    output=$("$VERIFY" --no-receipt "$MUT" 2>&1)
    status=$?
    if [[ $status -ne 0 ]] && grep -E '^FAIL' <<<"$output" | grep -qF -- "$want"; then
        pass "$what"
    else
        fail "$what (exit $status): $(grep -E '^FAIL' <<<"$output" | head -3)"
    fi
}
mutated; rm "$MUT/Contents/MacOS/beekeeper-dev-mcp"
expect_verify_fail "a removed helper" "differs from the list"
mutated; build_fake beekeeper-desktop "$MUT/Contents/MacOS/beekeeper-desktop"
expect_verify_fail "an added Desktop" "beekeeper-desktop is in the bundle"
mutated; mkdir -p "$MUT/Contents/Library/LoginItems"
expect_verify_fail "a nested login item" "Contents/Library exists"
mutated; echo 'x' >>"$MUT/Contents/Resources/personas/templates/working-contract/1.2.2/TEMPLATE.md"
expect_verify_fail "an edited template" "changed since packaging"
mutated; echo '<script>' >"$MUT/Contents/Resources/index.html"
expect_verify_fail "a stray frontend" "web assets"
mutated; ln -s /usr/bin/true "$MUT/Contents/Resources/escape"
expect_verify_fail "a symlink out of the bundle" "symlink"
mutated; /usr/libexec/PlistBuddy -c 'Add :CFBundleURLTypes array' "$MUT/Contents/Info.plist"
expect_verify_fail "a claimed URL scheme" "URL scheme"
mutated; /usr/libexec/PlistBuddy -c 'Set :LSEnvironment:BEEKEEPER_HOST_INSTANCE dev' "$MUT/Contents/Info.plist"
expect_verify_fail "an instance the bundle id disagrees with" "do not go together"
mutated; rm -rf "$MUT/Contents/Resources/personas/templates"
expect_verify_fail "missing templates" "resources incomplete"
mutated; codesign --remove-signature "$MUT"
expect_verify_fail "a removed signature" "signature"
mutated
RCPT="$T/mut/$STANDALONE_HOST_APP.receipt.json"; cp "$GOOD.receipt.json" "$RCPT"
sed -i '' 's/"treeSha256": "[0-9a-f]*"/"treeSha256": "00"/' "$RCPT"
receipt_out=$("$VERIFY" "$MUT" 2>&1); receipt_status=$?
[[ $receipt_status -ne 0 ]] && grep -q 'the tree differs from the receipt' <<<"$receipt_out" \
    && pass "a tree the receipt does not describe" || fail "the receipt mismatch went unnoticed"

# ── the real binaries ────────────────────────────────────────────────────────
if [[ -n "$SOURCE_APP" ]]; then
    REAL_OUT="$T/real"
    REAL="$REAL_OUT/$STANDALONE_HOST_APP"
    step "1. Real binaries package and verify"
    EXPECT_ARGS=()
    [[ -n "$EXPECT_COMMIT" ]] && EXPECT_ARGS=(--expect-commit "$EXPECT_COMMIT")
    SOURCE_BEFORE=$(standalone_host_tree_sha "$SOURCE_APP")
    if "$PACKAGE" --from-app "$SOURCE_APP" --out "$REAL_OUT" --identity "$IDENTITY" \
        ${EXPECT_ARGS[@]+"${EXPECT_ARGS[@]}"} >"$T/real.log" 2>&1; then
        pass "packaged and verified: $(sed -n 's/^commit /commit /p; /^signed by/p' "$T/real.log" | tr '\n' ' ')"
    else
        fail "real packaging failed: $(tail -8 "$T/real.log")"
        echo; echo "$PASSED passed, $FAILED failed"; exit 1
    fi
    "$VERIFY" "$REAL" >"$T/real-verify.log" 2>&1 \
        && pass "verify passes, with the receipt" || fail "verify: $(grep FAIL "$T/real-verify.log")"
    grep -E '^INFO' "$T/real-verify.log" | sed 's/^/        /'

    step "2. Parity with the source, minus what is Desktop's"
    MISSING=""
    for binary in $(ls "$SOURCE_APP/Contents/MacOS"); do
        case " ${STANDALONE_HOST_FORBIDDEN[*]} " in *" $binary "*) continue ;; esac
        [[ -e "$REAL/Contents/MacOS/$binary" ]] || MISSING+=" $binary"
    done
    [[ -z "$MISSING" ]] && pass "every source binary but the Desktop's is here" || fail "left behind:$MISSING"
    diff -r "$SOURCE_APP/Contents/Resources/personas" "$REAL/Contents/Resources/personas" >/dev/null \
        && pass "personas are identical to the source's" || fail "personas differ from the source's"
    python3 -I - "$REAL" "$SOURCE_APP" <<'PY' && pass "every helper's source hash is recorded and right" || fail "a recorded source hash is wrong"
import hashlib, json, sys
app, source = sys.argv[1:3]
manifest = json.load(open(f"{app}/Contents/Resources/host-package.json"))
sha = lambda path: hashlib.sha256(open(path, "rb").read()).hexdigest()
for entry in manifest["files"]:
    if entry["path"].startswith("Contents/MacOS/"):
        name = entry["path"].rsplit("/", 1)[1]
        assert entry["sourceSha256"] == sha(f"{source}/Contents/MacOS/{name}"), name
        if name in manifest["helpersKeptByteForByte"]:
            assert entry["sha256"] == entry["sourceSha256"], name
PY

    # The traps. Each is a fake that exits loudly if anything runs it, sitting
    # where a lookup that escaped the bundle would find it.
    TRAP="$T/trap"; WORK="$T/work"
    mkdir -p "$TRAP" "$WORK/target/debug" "$WORK/target/release"
    for binary in "${HELPERS[@]}"; do
        for place in "$TRAP" "$WORK/target/debug" "$WORK/target/release"; do
            printf '#!/bin/sh\necho "TRAP %s" >&2\nexit 66\n' "$place/$binary" >"$place/$binary"
            chmod +x "$place/$binary"
        done
    done
    OS_HOME="$T/os-home"; HOST_HOME="$T/host-home"
    mkdir -p "$OS_HOME" "$HOST_HOME"
    SOCK="$T/h.sock"
    # Every invocation below: two different homes, a poisoned PATH, nothing inherited.
    isolated() {
        env -i HOME="$OS_HOME" BEEKEEPER_HOST_HOME="$HOST_HOME" BEEKEEPER_HOST_SOCK="$SOCK" \
            PATH="$TRAP:/usr/bin:/bin" "$@"
    }

    step "3. Templates come from this bundle, wherever it is run from"
    mkdir -p "$T/team/roles"
    printf '%s\n' '---' 'name: probe' 'description: packaging probe' '---' '![[beekeeper/working-contract@^1.0.0]]' >"$T/team/roles/probe.md"
    if composed=$(cd "$WORK" && isolated "$REAL/Contents/MacOS/bee" pack compose "$T/team" --role probe 2>&1) \
        && grep -q 'Working contract' <<<"$composed"; then
        pass "bee pack compose expands a shipped template"
    else
        fail "bee pack compose: $(tail -3 <<<"$composed")"
    fi
    NO_T="$T/no-templates"; mkdir -p "$NO_T"; ditto "$REAL" "$NO_T/$STANDALONE_HOST_APP"
    rm -rf "$NO_T/$STANDALONE_HOST_APP/Contents/Resources/personas/templates"
    if composed=$(cd "$REPO" && isolated "$NO_T/$STANDALONE_HOST_APP/Contents/MacOS/bee" pack compose "$T/team" --role probe 2>&1); then
        fail "a bundle with no templates still composed, so it borrowed someone's"
    else
        grep -q 'templates' <<<"$composed" \
            && pass "a bundle missing its templates refuses, standing in a checkout that has them" \
            || fail "wrong refusal: $(tail -2 <<<"$composed")"
    fi

    step "4. Registration names this bundle's binaries"
    LAUNCHD_BEFORE=$(launchctl list | grep -i 'io.agiterra.beekeeper' | LC_ALL=C sort)
    if isolated "$REAL/Contents/MacOS/bee" host install >"$T/install.json" 2>"$T/install.err"; then
        PLIST="$HOST_HOME/Library/LaunchAgents/io.agiterra.beekeeper.host.plist"
        [[ -f "$PLIST" ]] && pass "bee host install wrote into the throwaway home" || fail "no plist at $PLIST"
        program=$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$PLIST" 2>/dev/null)
        [[ "$program" == "$(cd "$REAL" && pwd -P)/Contents/MacOS/beekeeper-host" ]] \
            && pass "the program is this bundle's beekeeper-host, space in the path and all" || fail "the program is $program"
        [[ "$(/usr/libexec/PlistBuddy -c 'Print :EnvironmentVariables:BEEKEEPER_HOST_INSTANCE' "$PLIST" 2>/dev/null)" == production ]] \
            && pass "the registration carries the instance" || fail "no instance in the registration"
    else
        fail "bee host install: $(cat "$T/install.err")"
    fi

    step "5. launchd did not notice"
    LAUNCHD_AFTER=$(launchctl list | grep -i 'io.agiterra.beekeeper' | LC_ALL=C sort)
    [[ "$LAUNCHD_BEFORE" == "$LAUNCHD_AFTER" ]] \
        && pass "launchctl list is unchanged for io.agiterra.beekeeper.*" \
        || fail "launchd changed: before [$LAUNCHD_BEFORE] after [$LAUNCHD_AFTER]"

    # A commissioning with nothing behind it: its secret is the number 1, its
    # public key is the curve's generator, and its relay does not resolve.
    PUBKEY=79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798
    TEST_KEY=0000000000000000000000000000000000000000000000000000000000000001
    RELAY="wss://unreachable.invalid"
    HOST_DIR="$HOST_HOME/.local/state/buzz/host"
    BASE_DIR="$HOST_HOME/app-data/session-provider"
    mkdir -p "$HOST_DIR" "$BASE_DIR/$PUBKEY" "$BASE_DIR/logs"
    python3 -I - "$BASE_DIR" "$HOST_DIR" "$PUBKEY" "$RELAY" <<'PY'
import json, pathlib, sys
base, host, pubkey, relay = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4]
(base / "coding-session-provider.json").write_text(json.dumps({
    "version": 1,
    "providers": {relay: {"providerPubkey": pubkey, "instanceId": pubkey[:16],
                          "createdAt": "2026-11-04T00:00:00Z", "relayUrl": relay}},
}))
(host / "host.json").write_text(json.dumps({
    "version": 1, "instance": "production", "relayUrl": relay, "providerPubkey": pubkey,
    "sessionProviderBaseDir": str(base), "providerStateDir": str(base / pubkey),
    "runtimes": [], "writtenAt": "2026-11-04T00:00:00Z",
}))
PY
    printf '%s\n' "$TEST_KEY" >"$HOST_DIR/provider-key"; chmod 600 "$HOST_DIR/provider-key"

    step "6. The Host picks this bundle's provider, past every trap"
    check=$(cd "$WORK" && isolated "$REAL/Contents/MacOS/beekeeper-host" check 2>&1)
    [[ "$(sed -n 's/^provider bin:  //p' <<<"$check")" == "$(cd "$REAL" && pwd -P)/Contents/MacOS/beekeeper-session-provider" ]] \
        && pass "beekeeper-host check names Contents/MacOS, from a directory with target/ traps and a poisoned PATH" \
        || fail "beekeeper-host check: $check"

    # The same host, really supervising, with the provider swapped for a recorder.
    SUP="$T/sup"; mkdir -p "$SUP"; ditto "$REAL" "$SUP/$STANDALONE_HOST_APP"
    SUP_APP="$(cd "$SUP/$STANDALONE_HOST_APP" && pwd -P)"
    MARK="$T/provider-saw"
    cat >"$SUP_APP/Contents/MacOS/beekeeper-session-provider" <<SH
#!/bin/sh
# Names and paths only: never a value from a variable holding a key.
{
    echo "self=\$0"
    echo "path=\$PATH"
    echo "mcp=\$BEEKEEPER_CSP_CONTEXT_MCP_COMMAND"
    echo "instance=\$BEEKEEPER_HOST_INSTANCE"
    echo "home=\$BEEKEEPER_HOST_HOME"
} >"$MARK.tmp"
mv "$MARK.tmp" "$MARK"
exec sleep 120
SH
    chmod +x "$SUP_APP/Contents/MacOS/beekeeper-session-provider"
    ( cd "$WORK" && exec env -i HOME="$OS_HOME" BEEKEEPER_HOST_HOME="$HOST_HOME" BEEKEEPER_HOST_SOCK="$SOCK" \
        PATH="$TRAP:/usr/bin:/bin" "$SUP_APP/Contents/MacOS/beekeeper-host" run ) >"$T/host-run.log" 2>&1 &
    HOST_PID=$!
    RUNNING_PIDS+=("$HOST_PID")
    for _ in $(seq 1 100); do [[ -f "$MARK" ]] && break; perl -e 'select(undef,undef,undef,0.1)'; done
    if [[ -f "$MARK" ]]; then
        [[ "$(sed -n 's/^self=//p' "$MARK")" == "$SUP_APP/Contents/MacOS/beekeeper-session-provider" ]] \
            && pass "the host started the provider beside it" || fail "the host started $(sed -n 's/^self=//p' "$MARK")"
        [[ "$(sed -n 's/^path=//p' "$MARK")" == "$SUP_APP/Contents/MacOS:"* ]] \
            && pass "the provider's PATH leads with Contents/MacOS, ahead of the trap" \
            || fail "the provider's PATH is $(sed -n 's/^path=//p' "$MARK")"
        [[ "$(sed -n 's/^mcp=//p' "$MARK")" == "$SUP_APP/Contents/MacOS/beekeeper-dev-mcp" ]] \
            && pass "the context MCP sidecar resolves to this bundle" || fail "mcp is $(sed -n 's/^mcp=//p' "$MARK")"
        [[ "$(sed -n 's/^instance=//p' "$MARK")" == production && "$(sed -n 's/^home=//p' "$MARK")" == "$HOST_HOME" ]] \
            && pass "the provider inherits this instance and this home" || fail "wrong instance or home: $(cat "$MARK")"
    else
        fail "the host never started the provider: $(tail -5 "$T/host-run.log")"
    fi
    status=$(isolated "$REAL/Contents/MacOS/bee" host status 2>&1)
    grep -q '"state"' <<<"$status" && pass "this bundle's bee reads this host over the throwaway socket" \
        || fail "bee host status: $(tail -2 <<<"$status")"
    inventory=$(isolated "$REAL/Contents/MacOS/$STANDALONE_HOST_MAIN" --manage-inventory 2>/dev/null)
    grep -q '"ok":true' <<<"$inventory" && pass "the menu bar reads the same host's inventory, with no window" \
        || fail "--manage-inventory: $inventory"
    kill "$HOST_PID" 2>/dev/null; wait "$HOST_PID" 2>/dev/null
    RUNNING_PIDS=()
    [[ ! -S "$SOCK" ]] && pass "the host stopped and removed its socket" || fail "the socket outlived the host"

    step "7. A missing sidecar: the runtime and the verifier both refuse"
    GONE="$T/gone"; mkdir -p "$GONE"; ditto "$REAL" "$GONE/$STANDALONE_HOST_APP"
    rm "$GONE/$STANDALONE_HOST_APP/Contents/MacOS/beekeeper-session-provider"
    check=$(cd "$WORK" && isolated "$GONE/$STANDALONE_HOST_APP/Contents/MacOS/beekeeper-host" check 2>&1)
    found=$(sed -n 's/^provider bin:  //p' <<<"$check")
    if [[ "$found" == "<unresolved:"*beekeeper-session-provider* && "$found" != *"$TRAP"* ]]; then
        pass "with its own provider gone, the Host refuses by name and does not take the one on PATH"
    else
        fail "the Host did not refuse a missing provider: $found"
    fi
    "$VERIFY" --no-receipt "$GONE/$STANDALONE_HOST_APP" >/dev/null 2>&1 \
        && fail "verify accepted a bundle with no provider" || pass "verify refuses that bundle"

    step "8. The menu bar uses its own bee, and never one from PATH"
    BEESWAP="$T/beeswap"; mkdir -p "$BEESWAP"; ditto "$REAL" "$BEESWAP/$STANDALONE_HOST_APP"
    SWAP_APP="$(cd "$BEESWAP/$STANDALONE_HOST_APP" && pwd -P)"
    cat >"$SWAP_APP/Contents/MacOS/bee" <<'SH'
#!/bin/sh
printf '{"ranFrom":"%s"}\n' "$0"
SH
    chmod +x "$SWAP_APP/Contents/MacOS/bee"
    answer=$(cd "$WORK" && isolated "$SWAP_APP/Contents/MacOS/$STANDALONE_HOST_MAIN" \
        --manage-action '{"kind":"roles","project":"probe"}' 2>/dev/null)
    grep -qF "\"ranFrom\":\"$SWAP_APP/Contents/MacOS/bee\"" <<<"$answer" \
        && pass "Manage Host's actions run Contents/MacOS/bee" || fail "the action answered: $answer"
    rm "$SWAP_APP/Contents/MacOS/bee"
    answer=$(cd "$WORK" && isolated "$SWAP_APP/Contents/MacOS/$STANDALONE_HOST_MAIN" \
        --manage-action '{"kind":"roles","project":"probe"}' 2>/dev/null)
    grep -q 'BEE_NOT_FOUND' <<<"$answer" \
        && pass "with no bee beside it, it refuses by name and does not take the one on PATH" \
        || fail "the action answered: $answer"
    answer=$(isolated "$REAL/Contents/MacOS/$STANDALONE_HOST_MAIN" --manage-inventory 2>/dev/null)
    grep -q '"ok":false' <<<"$answer" && pass "with no host running, the inventory says so rather than guessing" \
        || fail "the inventory answered: $answer"

    if $GUI; then
        step "9. The menu bar launches as launchd would start it, with this identity"
        ( exec env -i HOME="$OS_HOME" BEEKEEPER_HOST_HOME="$HOST_HOME" BEEKEEPER_HOST_SOCK="$SOCK" \
            BEEKEEPER_HOST_INSTANCE=production PATH="$TRAP:/usr/bin:/bin" \
            "$REAL/Contents/MacOS/$STANDALONE_HOST_MAIN" --manage-host ) >"$T/menubar.log" 2>&1 &
        MENU_PID=$!
        RUNNING_PIDS+=("$MENU_PID")
        for _ in $(seq 1 100); do
            grep -q 'manage-host read the inventory' "$T/menubar.log" && break
            perl -e 'select(undef,undef,undef,0.1)'
        done
        kill -0 "$MENU_PID" 2>/dev/null && pass "still running" || fail "it exited: $(tail -3 "$T/menubar.log")"
        seen=$(lsappinfo info "$(lsappinfo find pid="$MENU_PID" 2>/dev/null)" 2>/dev/null)
        grep -qF 'bundleID="io.agiterra.beekeeper.host"' <<<"$seen" \
            && pass "macOS sees it as io.agiterra.beekeeper.host" || fail "macOS sees: $seen"
        # /tmp is a symlink here, and macOS may report the path either way.
        grep -qE "bundle path=\"(/private)?$REAL\"" <<<"$seen" \
            && pass "macOS resolves its bundle to this package" || fail "macOS sees: $seen"
        grep -qF 'type="UIElement"' <<<"$seen" \
            && pass "an accessory, with no Dock icon" || fail "macOS sees: $seen"
        grep -q 'manage-host read the inventory' "$T/menubar.log" \
            && pass "the embedded Manage Host page loaded and called back over IPC, with no frontend build" \
            || fail "the window never asked for the inventory: $(tail -3 "$T/menubar.log")"
        kill "$MENU_PID" 2>/dev/null; wait "$MENU_PID" 2>/dev/null
        RUNNING_PIDS=()
    else
        echo; echo "== 9. Skipped: the on-screen launch runs only with --gui"
    fi

    step "10. The source bundle is untouched"
    [[ "$(standalone_host_tree_sha "$SOURCE_APP")" == "$SOURCE_BEFORE" ]] \
        && pass "same tree digest as before this run" || fail "the source bundle changed"
fi

echo
echo "$PASSED passed, $FAILED failed"
[[ $FAILED -eq 0 ]]
