#!/usr/bin/env bash
# =============================================================================
# host-package-verify.sh — is this `Beekeeper Host.app` what it says it is?
# =============================================================================
#
#   scripts/host-package-verify.sh [--expect-commit <rev>] [--no-receipt] <app>
#
# Read-only. It starts nothing: the only processes it runs out of the bundle are
# `--version` calls under an empty environment. Exits non-zero on any FAIL. INFO
# lines are facts a reader should know that are not failures, such as "not
# notarized".
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/host-package-lib.sh
source "$SCRIPT_DIR/host-package-lib.sh"

EXPECT_COMMIT="" USE_RECEIPT=true APP=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --expect-commit) EXPECT_COMMIT="${2:?--expect-commit needs a revision}"; shift 2 ;;
        --no-receipt) USE_RECEIPT=false; shift ;;
        -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
        -*) echo "unknown option: $1" >&2; exit 2 ;;
        *) APP="$1"; shift ;;
    esac
done
[[ -n "$APP" && -d "$APP" ]] || { echo "name an app bundle" >&2; exit 2; }
APP="$(cd "$APP" && pwd -P)"

FAILS=0
pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }
info() { echo "INFO  $*"; }
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$APP/Contents/Info.plist" 2>/dev/null; }
MANIFEST="$APP/$STANDALONE_HOST_MANIFEST"

HELPERS=()
while IFS= read -r helper; do HELPERS+=("$helper"); done < <(standalone_host_helpers)

# ── Contents/MacOS is exactly the list ───────────────────────────────────────
expected=$(printf '%s\n' "${HELPERS[@]}" "$STANDALONE_HOST_MAIN" | LC_ALL=C sort)
actual=$(ls -A "$APP/Contents/MacOS" 2>/dev/null | LC_ALL=C sort)
if [[ "$expected" == "$actual" ]]; then
    pass "Contents/MacOS holds exactly the ${#HELPERS[@]} helpers and the menu bar"
else
    fail "Contents/MacOS differs from the list: $(diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") | grep '^[<>]' | tr '\n' ' ')"
fi
for name in "${HELPERS[@]}" "$STANDALONE_HOST_MAIN"; do
    [[ -x "$APP/Contents/MacOS/$name" ]] || fail "$name is not executable"
done

# ── no Desktop ───────────────────────────────────────────────────────────────
desktop_found=false
for forbidden in "${STANDALONE_HOST_FORBIDDEN[@]}"; do
    if [[ -n "$(find "$APP" -name "$forbidden*" -print -quit)" ]]; then
        fail "$forbidden is in the bundle"
        desktop_found=true
    fi
done
if [[ -e "$APP/Contents/Library" ]]; then
    fail "Contents/Library exists: this bundle nests nothing"
    desktop_found=true
fi
# The Desktop's Vite output is hashed assets; a stray one means someone copied it.
if [[ -n "$(find "$APP" \( -name '*.asar' -o -name 'index.html' -o -name '*.js' -o -name '*.css' -o -name '*.dylib' \) -print -quit)" ]]; then
    fail "web assets or a loose library are in the bundle"
    desktop_found=true
fi
$desktop_found || pass "no Desktop executable, sidecar, nested login item or frontend build"

if [[ -n "$(find "$APP" -type l -print -quit)" ]]; then
    fail "the bundle holds a symlink, so its contents are not all its own: $(find "$APP" -type l | head -3)"
else
    pass "no symlinks"
fi

# ── nothing is linked from outside the system ────────────────────────────────
foreign=""
for name in "${HELPERS[@]}" "$STANDALONE_HOST_MAIN"; do
    linked=$(otool -L "$APP/Contents/MacOS/$name" 2>/dev/null | tail -n +2 | awk '{print $1}' \
        | grep -vE '^(/usr/lib/|/System/Library/)' || true)
    [[ -z "$linked" ]] || foreign+=" $name:$linked"
done
if [[ -z "$foreign" ]]; then
    pass "every binary links only system libraries"
else
    fail "linked from outside the system:$foreign"
fi
ARCHS=$(for name in "${HELPERS[@]}" "$STANDALONE_HOST_MAIN"; do lipo -archs "$APP/Contents/MacOS/$name" 2>/dev/null; done | sort -u | tr '\n' ' ')
info "architectures: $ARCHS"

# ── the launch identity ──────────────────────────────────────────────────────
[[ "$(plist CFBundleExecutable)" == "$STANDALONE_HOST_MAIN" ]] \
    && pass "the bundle's executable is the menu bar" \
    || fail "CFBundleExecutable is $(plist CFBundleExecutable)"
[[ "$(plist LSUIElement)" == true ]] \
    && pass "LSUIElement: no Dock icon" || fail "LSUIElement is $(plist LSUIElement)"
BUNDLE_ID=$(plist CFBundleIdentifier)
INSTANCE=$(plist LSEnvironment:BEEKEEPER_HOST_INSTANCE)
case "$INSTANCE/$BUNDLE_ID" in
    production/io.agiterra.beekeeper.host | dev/io.agiterra.beekeeper.host.dev)
        pass "bundle $BUNDLE_ID reads instance $INSTANCE when launched without launchd" ;;
    *) fail "bundle id $BUNDLE_ID and instance ${INSTANCE:-<unset>} do not go together" ;;
esac
if plist CFBundleURLTypes >/dev/null; then
    fail "the bundle claims a URL scheme: it would answer beekeeper:// instead of Desktop"
else
    pass "claims no URL scheme"
fi
for key in NSCameraUsageDescription NSMicrophoneUsageDescription; do
    if plist "$key" >/dev/null; then fail "declares $key, which this app does not use"; fi
done

# ── resources ────────────────────────────────────────────────────────────────
TEMPLATES="$APP/Contents/Resources/personas/templates"
ROLES="$APP/Contents/Resources/personas/roles"
template_count=$(find "$TEMPLATES" -mindepth 3 -maxdepth 3 -name TEMPLATE.md 2>/dev/null | wc -l | tr -d ' ')
role_count=$(find "$ROLES" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
if [[ "$template_count" -gt 0 && "$role_count" -gt 0 ]]; then
    pass "resources: $role_count roles, $template_count template versions"
else
    fail "resources incomplete: $role_count roles, $template_count template versions"
fi
[[ -f "$APP/Contents/Resources/$(plist CFBundleIconFile)" ]] || fail "the icon the plist names is missing"

# ── the manifest ─────────────────────────────────────────────────────────────
if [[ ! -f "$MANIFEST" ]]; then
    fail "no $STANDALONE_HOST_MANIFEST"
else
    manifest_report=$(python3 -I - "$APP" "$MANIFEST" "$STANDALONE_HOST_SCHEMA" "$STANDALONE_HOST_MAIN" "$BUNDLE_ID" <<'PY'
import hashlib, json, os, sys
app, manifest_path, schema, main, bundle_id = sys.argv[1:6]
manifest = json.load(open(manifest_path))
problems = []
if manifest.get("schema") != schema:
    problems.append(f"schema is {manifest.get('schema')}")
if manifest.get("bundleIdentifier") != bundle_id:
    problems.append("its bundle id is not this bundle's")
listed = {entry["path"]: entry["sha256"] for entry in manifest.get("files", [])}
own = {"Contents/Resources/host-package.json", f"Contents/MacOS/{main}"}
found = set()
for root, _, names in os.walk(app):
    for name in names:
        rel = os.path.relpath(os.path.join(root, name), app)
        if rel in own or rel.startswith("Contents/_CodeSignature/"):
            continue
        found.add(rel)
for rel in sorted(found - set(listed)):
    problems.append(f"not in the manifest: {rel}")
for rel in sorted(set(listed) - found):
    problems.append(f"in the manifest but gone: {rel}")
for rel in sorted(found & set(listed)):
    digest = hashlib.sha256(open(os.path.join(app, rel), "rb").read()).hexdigest()
    if digest != listed[rel]:
        problems.append(f"changed since packaging: {rel}")
for problem in problems[:12]:
    print(f"FAIL  manifest: {problem}")
if not problems:
    print(f"PASS  manifest: {len(listed)} files match their sha256, and none is unlisted")
source = manifest.get("source", {})
print(f"INFO  source: {source.get('kind')}, {source.get('buildProfile')} profile; "
      f"{len(manifest.get('helpersKeptByteForByte', []))} helpers kept byte-for-byte")
print(f"COMMIT {manifest.get('commit', '')}")
print(f"DIRTY {'true' if manifest.get('dirty') else 'false'}")
PY
)
    while IFS= read -r line; do
        case "$line" in
            COMMIT\ *) MANIFEST_COMMIT="${line#COMMIT }" ;;
            DIRTY\ *) MANIFEST_DIRTY="${line#DIRTY }" ;;
            FAIL*) echo "$line"; FAILS=$((FAILS + 1)) ;;
            *) echo "$line" ;;
        esac
    done <<<"$manifest_report"
fi

# ── which commit, from the binaries ──────────────────────────────────────────
PROBE_DIR=$(mktemp -d)
trap 'rm -rf "$PROBE_DIR"' EXIT
BEE_LINE=$(cd "$PROBE_DIR" && env -i HOME="$PROBE_DIR" PATH=/usr/bin:/bin "$APP/Contents/MacOS/bee" --version 2>&1 | head -1)
BEE_COMMIT=$(printf '%s' "$BEE_LINE" | sed -n 's/^bee [^ ]* (\([0-9a-f]\{7,40\}\)\(-dirty\)\{0,1\}).*$/\1/p')
BEE_DIRTY=false
grep -qE '\([0-9a-f]+-dirty\)' <<<"$BEE_LINE" && BEE_DIRTY=true
if [[ -z "$BEE_COMMIT" ]]; then
    fail "bee names no commit: $BEE_LINE"
elif [[ -n "${MANIFEST_COMMIT:-}" && "$BEE_COMMIT" != "$MANIFEST_COMMIT" ]]; then
    fail "bee says $BEE_COMMIT, the manifest says $MANIFEST_COMMIT"
elif [[ -n "${MANIFEST_DIRTY:-}" && "$BEE_DIRTY" != "$MANIFEST_DIRTY" ]]; then
    fail "bee says dirty=$BEE_DIRTY, the manifest says dirty=$MANIFEST_DIRTY"
elif [[ -n "$EXPECT_COMMIT" && "$EXPECT_COMMIT" != "$BEE_COMMIT"* && "$BEE_COMMIT" != "$EXPECT_COMMIT"* ]]; then
    fail "bee says $BEE_COMMIT, expected $EXPECT_COMMIT"
else
    pass "bee runs from the bundle under an empty environment: $BEE_LINE"
fi
$BEE_DIRTY && info "DIRTY: these binaries were built from uncommitted files, so they are not commit $BEE_COMMIT"
# A prefix that appears in the bytes, or a disclosed non-answer. Not every crate
# embeds the build stamp, and a missing one is not a mismatch.
if [[ -n "$BEE_COMMIT" ]]; then
    stamped="" unstamped=""
    for name in "${HELPERS[@]}" "$STANDALONE_HOST_MAIN"; do
        if grep -aq "$BEE_COMMIT" "$APP/Contents/MacOS/$name"; then stamped+=" $name"; else unstamped+=" $name"; fi
    done
    info "bytes carry commit $BEE_COMMIT:${stamped:- none}; embed no stamp, so only the source vouches:${unstamped:- none}"
fi

# ── signatures ───────────────────────────────────────────────────────────────
if output=$(codesign --verify --deep --strict "$APP" 2>&1); then
    pass "codesign --verify --deep --strict"
else
    fail "bundle signature: $output"
fi
bad=""
for name in "${HELPERS[@]}" "$STANDALONE_HOST_MAIN"; do
    codesign --verify --strict "$APP/Contents/MacOS/$name" 2>/dev/null || bad+=" $name"
done
if [[ -z "$bad" ]]; then pass "every binary's own signature verifies"; else fail "signature does not verify:$bad"; fi
DETAILS=$(codesign -dvvv "$APP" 2>&1)
SIGNED_ID=$(printf '%s\n' "$DETAILS" | sed -n 's/^Identifier=//p' | head -1)
[[ "$SIGNED_ID" == "$BUNDLE_ID" ]] \
    && pass "signed under the bundle's own identifier" \
    || fail "signed as $SIGNED_ID, the bundle is $BUNDLE_ID"
ENTITLEMENTS=$(codesign -d --entitlements :- "$APP" 2>/dev/null)
if [[ -z "$ENTITLEMENTS" ]] || ! grep -q '<key>' <<<"$ENTITLEMENTS"; then
    pass "no entitlements: no camera, microphone or library-validation grant"
else
    fail "carries entitlements"
fi
AUTHORITY=$(printf '%s\n' "$DETAILS" | sed -n 's/^Authority=//p' | head -1)
TEAM=$(printf '%s\n' "$DETAILS" | sed -n 's/^TeamIdentifier=//p' | head -1)
info "signer: ${AUTHORITY:-ad-hoc}; team: ${TEAM:-unknown}; cdhash $(printf '%s\n' "$DETAILS" | sed -n 's/^CDHash=//p' | head -1)"
info "requirement: $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^[# ]*designated => //p')"
# Said, because it is the difference between this and a public download.
info "Gatekeeper: $(spctl --assess --type execute "$APP" 2>&1 | tail -1 | sed "s|$APP: ||"); no hardened runtime, not notarized (local distribution only)"

# ── the receipt, when there is one beside the bundle ─────────────────────────
RECEIPT="$APP.receipt.json"
if $USE_RECEIPT; then
    if [[ ! -f "$RECEIPT" ]]; then
        info "no receipt beside the bundle"
    else
        CDHASH=$(printf '%s\n' "$DETAILS" | sed -n 's/^CDHash=//p' | head -1)
        receipt_field() { python3 -I -c 'import json,sys
value=json.load(open(sys.argv[1]))
for key in sys.argv[2].split("."): value=value[key]
print(value)' "$RECEIPT" "$1"; }
        [[ "$(receipt_field treeSha256)" == "$(standalone_host_tree_sha "$APP")" ]] \
            && pass "the tree matches the receipt" || fail "the tree differs from the receipt"
        [[ "$(receipt_field signing.cdhash)" == "$CDHASH" ]] \
            && pass "the cdhash matches the receipt" || fail "the cdhash differs from the receipt"
        [[ "$(receipt_field mainExecutableSha256)" == "$(standalone_host_sha "$APP/Contents/MacOS/$STANDALONE_HOST_MAIN")" ]] \
            && pass "the main executable matches the receipt" || fail "the main executable differs from the receipt"
    fi
fi

if [[ $FAILS -gt 0 ]]; then
    echo "NOT OK: $FAILS failed"
    exit 1
fi
echo "OK"
