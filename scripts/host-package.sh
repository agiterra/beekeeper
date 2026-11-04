#!/usr/bin/env bash
# =============================================================================
# host-package.sh — stage `Beekeeper Host.app`, with no Desktop in it
# =============================================================================
#
#   scripts/host-package.sh --from-app <Beekeeper*.app> [options]
#       Reuse the exact binaries and resources of a built bundle. No compile.
#   scripts/host-package.sh --build [--debug] [options]
#       cargo build the eight binaries from this checkout, then package them.
#   scripts/host-package.sh --from-target <profile dir> \
#       --menubar-target <profile dir> [options]
#       Package what an earlier cargo build left.
#
# Options:
#   --out <dir>            Where to stage. Default dist/standalone-host. Refused
#                          under any Applications directory: this never installs.
#   --instance <i>         production (default) or dev. Sets the bundle id and the
#                          instance a Finder launch of the menu bar reads.
#   --identity <name|->    Signing identity; `-` is ad-hoc. Default: the local
#                          "Beekeeper Dev Local Signing" if it exists, else ad-hoc.
#   --expect-commit <rev>  Refuse unless `bee --version` names this commit.
#   --allow-dirty          Accept binaries stamped `-dirty`, built from a tree with
#                          uncommitted files. They are not that commit, so this is
#                          refused unless asked for, and recorded in the output.
#   --archive              Also write a zip beside the app, with its sha256.
#   --force                Replace an earlier output of this script.
#
# Output, in --out:
#   Beekeeper Host.app               the outer bundle: the existing menu bar is
#                                    its executable; Contents/MacOS also holds the
#                                    seven agent-stack binaries; Contents/Resources
#                                    holds personas/ and host-package.json.
#   Beekeeper Host.app.receipt.json  cdhash, tree digest, and what was not done.
#
# Nothing here commissions, starts, registers or installs anything, and it reads
# no key. It finishes by running host-package-verify.sh, whose failure is this
# script's failure. See docs/standalone-host-package.md.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/host-package-lib.sh
source "$SCRIPT_DIR/host-package-lib.sh"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

die() { echo "host-package: $*" >&2; exit 1; }

FROM_APP="" FROM_TARGET="" MENUBAR_TARGET="" BUILD=false PROFILE=release
OUT="$REPO/dist/standalone-host" INSTANCE=production IDENTITY=auto
EXPECT_COMMIT="" ARCHIVE=false FORCE=false ALLOW_DIRTY=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --from-app) FROM_APP="${2:?--from-app needs a bundle}"; shift 2 ;;
        --from-target) FROM_TARGET="${2:?--from-target needs a directory}"; shift 2 ;;
        --menubar-target) MENUBAR_TARGET="${2:?--menubar-target needs a directory}"; shift 2 ;;
        --build) BUILD=true; shift ;;
        --debug) PROFILE=debug; shift ;;
        --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
        --instance) INSTANCE="${2:?--instance needs production or dev}"; shift 2 ;;
        --identity) IDENTITY="${2:?--identity needs a name, or - for ad-hoc}"; shift 2 ;;
        --expect-commit) EXPECT_COMMIT="${2:?--expect-commit needs a revision}"; shift 2 ;;
        --allow-dirty) ALLOW_DIRTY=true; shift ;;
        --archive) ARCHIVE=true; shift ;;
        --force) FORCE=true; shift ;;
        -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

[[ "$(uname -s)" == Darwin ]] || die "this builds a macOS bundle; Linux uses scripts/package-agent-stack.sh"
modes=0
[[ -n "$FROM_APP" ]] && modes=$((modes + 1))
[[ -n "$FROM_TARGET" ]] && modes=$((modes + 1))
$BUILD && modes=$((modes + 1))
[[ $modes -eq 1 ]] || die "name exactly one of --from-app, --from-target, --build"
if [[ -n "$FROM_TARGET" && -z "$MENUBAR_TARGET" ]]; then
    die "--from-target also needs --menubar-target (the desktop/src-tauri workspace builds it)"
fi

case "$INSTANCE" in
    production) BUNDLE_ID=io.agiterra.beekeeper.host ;;
    dev) BUNDLE_ID=io.agiterra.beekeeper.host.dev ;;
    *) die "--instance must be production or dev, not $INSTANCE" ;;
esac

HELPERS=()
while IFS= read -r helper; do HELPERS+=("$helper"); done < <(standalone_host_helpers)
for helper in "${HELPERS[@]}"; do
    for forbidden in "${STANDALONE_HOST_FORBIDDEN[@]}"; do
        [[ "$helper" != "$forbidden" ]] || die "$forbidden is on the helper list and must not be"
    done
done

# ── where the bytes come from ────────────────────────────────────────────────
if $BUILD; then
    BIN_FLAGS=(--locked)
    if [[ "$PROFILE" == release ]]; then
        BIN_FLAGS+=(--release)
    else
        # A Dev bundle's profile, as scripts/app-from.sh builds it: debug
        # assertions on, unoptimized code off (ledger 310).
        export CARGO_PROFILE_DEV_OPT_LEVEL="${CARGO_PROFILE_DEV_OPT_LEVEL:-2}"
    fi
    # package-agent-stack.sh's own build line, by construction: one package per binary.
    (cd "$REPO" && cargo build "${BIN_FLAGS[@]}" -p beekeeper-host -p beekeeper-session-provider \
        -p beekeeper-cli -p beekeeper-acp -p beekeeper-agent -p beekeeper-dev-mcp -p git-credential-nostr)
    # `tauri/custom-protocol` is what `tauri build` passes. Without it this is a
    # dev build of the menu bar, not the bundled one. The frontend is two tracked
    # static files embedded at compile time; no Node and no frontend build.
    (cd "$REPO" && cargo build "${BIN_FLAGS[@]}" --manifest-path desktop/src-tauri/Cargo.toml \
        -p beekeeper-menubar --features tauri/custom-protocol)
    FROM_TARGET="${CARGO_TARGET_DIR:-$REPO/target}/$PROFILE"
    MENUBAR_TARGET="${CARGO_TARGET_DIR:-$REPO/desktop/src-tauri/target}/$PROFILE"
fi

if [[ -n "$FROM_APP" ]]; then
    [[ -d "$FROM_APP/Contents/MacOS" ]] || die "$FROM_APP is not an app bundle"
    SOURCE_KIND=app
    BIN_SOURCE="$FROM_APP/Contents/MacOS"
    # Nested, as a Desktop bundle carries it today, or already outer.
    MENUBAR_BUNDLE="$FROM_APP/Contents/Library/LoginItems/Beekeeper Menu Bar.app"
    if [[ -x "$MENUBAR_BUNDLE/Contents/MacOS/$STANDALONE_HOST_MAIN" ]]; then
        MENUBAR_BIN="$MENUBAR_BUNDLE/Contents/MacOS/$STANDALONE_HOST_MAIN"
        MENUBAR_PLIST="$MENUBAR_BUNDLE/Contents/Info.plist"
        ICON_SOURCE="$MENUBAR_BUNDLE/Contents/Resources/icon.icns"
    else
        MENUBAR_BIN="$BIN_SOURCE/$STANDALONE_HOST_MAIN"
        MENUBAR_PLIST="$FROM_APP/Contents/Info.plist"
        ICON_SOURCE="$FROM_APP/Contents/Resources/icon.icns"
    fi
    PERSONAS_SOURCE="$FROM_APP/Contents/Resources/personas"
    [[ -f "$MENUBAR_PLIST" ]] || die "no menu bar Info.plist in $FROM_APP"
    lsui=$(/usr/libexec/PlistBuddy -c 'Print :LSUIElement' "$MENUBAR_PLIST" 2>/dev/null || true)
    [[ "$lsui" == true ]] || die "the source menu bar is not LSUIElement, so it would take a Dock icon"
    VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$MENUBAR_PLIST")
    MIN_SYSTEM=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$MENUBAR_PLIST" 2>/dev/null || echo 10.13)
else
    SOURCE_KIND=cargo
    BIN_SOURCE="$FROM_TARGET"
    MENUBAR_BIN="$MENUBAR_TARGET/$STANDALONE_HOST_MAIN"
    ICON_SOURCE="$REPO/desktop/src-tauri/icons/icon.icns"
    PERSONAS_SOURCE="$REPO/personas"
    VERSION=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["version"])' \
        "$REPO/desktop/src-tauri/crates/beekeeper-menubar/tauri.conf.json")
    MIN_SYSTEM=10.13
fi

missing=()
for helper in "${HELPERS[@]}"; do
    [[ -x "$BIN_SOURCE/$helper" ]] || missing+=("$helper")
done
[[ -x "$MENUBAR_BIN" ]] || missing+=("$STANDALONE_HOST_MAIN")
[[ ${#missing[@]} -eq 0 ]] || die "missing from the source: ${missing[*]}"
[[ -f "$ICON_SOURCE" ]] || die "no icon at $ICON_SOURCE"
for dir in roles templates; do
    [[ -n "$(ls -A "$PERSONAS_SOURCE/$dir" 2>/dev/null)" ]] || die "no personas/$dir at $PERSONAS_SOURCE"
done

# ── which commit this is, from the binary and not from a label ───────────────
BEE_VERSION=$(cd /tmp && env -i HOME=/nonexistent PATH=/usr/bin:/bin "$BIN_SOURCE/bee" --version) \
    || die "the source's bee does not run"
STAMP=$(printf '%s\n' "$BEE_VERSION" | sed -n 's/^bee [^ ]* (\([0-9a-f]\{7,40\}\)\(-dirty\)\{0,1\}).*$/\1/p' | head -1)
[[ -n "$STAMP" ]] || die "the source's bee names no commit: $(printf '%s' "$BEE_VERSION" | head -1)"
DIRTY=false
if grep -qE '^bee [^ ]* \([0-9a-f]+-dirty\)' <<<"$BEE_VERSION"; then
    DIRTY=true
    $ALLOW_DIRTY || die "the source's bee is $STAMP-dirty: built from uncommitted files, so it is not that commit. Pass --allow-dirty to package it anyway; the output will say so"
fi
if [[ -n "$EXPECT_COMMIT" && "$EXPECT_COMMIT" != "$STAMP"* && "$STAMP" != "$EXPECT_COMMIT"* ]]; then
    die "the source's bee is $STAMP, not the expected $EXPECT_COMMIT"
fi
if [[ "$SOURCE_KIND" == cargo ]]; then
    HEAD=$(git -C "$REPO" rev-parse HEAD)
    [[ "$HEAD" == "$STAMP"* ]] \
        || die "the binaries are $STAMP but this checkout is $HEAD: a stale target directory, or the wrong one"
fi
# The debug profile is a fact about the bytes, and a Dev bundle carries it.
if grep -aq '/target/debug/build/' "$BIN_SOURCE/bee"; then BUILD_PROFILE=debug; else BUILD_PROFILE=release; fi

# ── which identity ───────────────────────────────────────────────────────────
if [[ "$IDENTITY" == auto ]]; then
    local_name="${BEEKEEPER_DEV_SIGNING_IDENTITY:-Beekeeper Dev Local Signing}"
    if security find-identity -p codesigning 2>/dev/null | grep -qF "\"$local_name\""; then
        IDENTITY="$local_name"
    else
        IDENTITY=-
        echo "host-package: no \"$local_name\" identity, so this is ad-hoc signed" >&2
    fi
fi

# ── where it goes ────────────────────────────────────────────────────────────
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd -P)"
case "$OUT/" in
    */Applications/*) die "$OUT is an Applications directory; this stages a package and never installs one" ;;
esac
FINAL="$OUT/$STANDALONE_HOST_APP"
RECEIPT="$FINAL.receipt.json"
ZIP="$OUT/Beekeeper Host-$VERSION-g$STAMP$($DIRTY && echo -dirty || true).zip"
if [[ -e "$FINAL" ]]; then
    $FORCE || die "$FINAL exists; pass --force to replace it"
    # Look before deleting: --force replaces this script's own output, nothing else.
    [[ -f "$FINAL/$STANDALONE_HOST_MANIFEST" ]] \
        || die "$FINAL is not an output of this script, so it is left alone"
    if holders=$(pgrep -lf "^$FINAL/Contents/MacOS/"); then
        die "something is running out of $FINAL, so it is left alone: $holders"
    fi
fi
STAGE=$(mktemp -d "$OUT/.stage.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
APP="$STAGE/$STANDALONE_HOST_APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# ── copy ─────────────────────────────────────────────────────────────────────
for helper in "${HELPERS[@]}"; do
    # `cp`, so the bytes (and any signature in them) arrive unchanged.
    cp "$BIN_SOURCE/$helper" "$APP/Contents/MacOS/$helper"
    chmod 755 "$APP/Contents/MacOS/$helper"
done
cp "$MENUBAR_BIN" "$APP/Contents/MacOS/$STANDALONE_HOST_MAIN"
chmod 755 "$APP/Contents/MacOS/$STANDALONE_HOST_MAIN"
cp "$ICON_SOURCE" "$APP/Contents/Resources/icon.icns"
mkdir -p "$APP/Contents/Resources/personas"
cp -R "$PERSONAS_SOURCE/roles" "$PERSONAS_SOURCE/templates" "$APP/Contents/Resources/personas/"
xattr -cr "$APP"

# Written, not copied: the menu bar's own plist describes a nested login item.
# No URL scheme (this app does not answer `beekeeper://`), no camera or microphone
# strings (it uses neither), and the instance a launch without launchd must read.
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>English</string>
	<key>CFBundleDisplayName</key>
	<string>Beekeeper Host</string>
	<key>CFBundleExecutable</key>
	<string>$STANDALONE_HOST_MAIN</string>
	<key>CFBundleIconFile</key>
	<string>icon.icns</string>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Beekeeper Host</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$VERSION</string>
	<key>CFBundleVersion</key>
	<string>$VERSION</string>
	<key>LSEnvironment</key>
	<dict>
		<key>BEEKEEPER_HOST_INSTANCE</key>
		<string>$INSTANCE</string>
	</dict>
	<key>LSMinimumSystemVersion</key>
	<string>$MIN_SYSTEM</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST
plutil -lint -s "$APP/Contents/Info.plist" || die "the generated Info.plist is not valid"

# ── sign, inside out ─────────────────────────────────────────────────────────
# A helper already signed by this identity stays byte-for-byte what was reviewed.
# Anything else (a linker's ad-hoc signature, another identity) is signed here,
# keeping the identifier a keychain or requirement would know it by: its own name.
signed_by_identity() {
    [[ "$IDENTITY" != - ]] || return 1
    codesign --verify --strict "$1" 2>/dev/null || return 1
    # Into a variable: `grep -q` stops reading, and pipefail would read the
    # resulting SIGPIPE in codesign as "not signed by this identity".
    local details
    details=$(codesign -dvv "$1" 2>&1)
    grep -qxF "Authority=$IDENTITY" <<<"$details"
}
KEPT=()
for helper in "${HELPERS[@]}"; do
    if signed_by_identity "$APP/Contents/MacOS/$helper"; then
        KEPT+=("$helper")
    else
        codesign --force --sign "$IDENTITY" --identifier "$helper" "$APP/Contents/MacOS/$helper" 2>/dev/null \
            || die "signing $helper with \"$IDENTITY\" failed"
    fi
done

# ── the manifest, sealed by the signature below ──────────────────────────────
# Every file but the main executable, which the bundle signature rewrites (its
# hash is in the receipt) and the seal itself.
python3 -I - "$APP" "$BIN_SOURCE" "$STANDALONE_HOST_SCHEMA" "$BUNDLE_ID" "$INSTANCE" "$VERSION" "$STAMP" \
    "$SOURCE_KIND" "$BUILD_PROFILE" "$IDENTITY" "$STANDALONE_HOST_MAIN" "$MENUBAR_BIN" "${KEPT[*]:-}" "$DIRTY" <<'PY'
import hashlib, json, os, sys
app, bin_source, schema, bundle_id, instance, version, commit, kind, profile, identity, main, main_src, kept, dirty = sys.argv[1:15]

def sha(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()

files = []
for root, _, names in os.walk(app):
    for name in names:
        path = os.path.join(root, name)
        rel = os.path.relpath(path, app)
        if rel == f"Contents/MacOS/{main}":
            continue
        entry = {"path": rel, "sha256": sha(path)}
        if rel.startswith("Contents/MacOS/"):
            entry["sourceSha256"] = sha(os.path.join(bin_source, name))
        files.append(entry)
files.sort(key=lambda entry: entry["path"].encode())
manifest = {
    "schema": schema,
    "bundleIdentifier": bundle_id,
    "instance": instance,
    "version": version,
    "commit": commit,
    "dirty": dirty == "true",
    "source": {"kind": kind, "buildProfile": profile},
    "signing": {"identity": "ad-hoc" if identity == "-" else identity},
    "mainExecutable": {"name": main, "sourceSha256": sha(main_src)},
    "helpersKeptByteForByte": sorted(kept.split()),
    "files": files,
}
with open(os.path.join(app, "Contents/Resources/host-package.json"), "w") as handle:
    json.dump(manifest, handle, indent=2)
    handle.write("\n")
PY

codesign --force --sign "$IDENTITY" --identifier "$BUNDLE_ID" "$APP" 2>/dev/null \
    || die "signing the bundle with \"$IDENTITY\" failed"

# ── verify the staged bytes, then move them into place ───────────────────────
"$SCRIPT_DIR/host-package-verify.sh" --no-receipt --expect-commit "$STAMP" "$APP"

[[ ! -e "$FINAL" ]] || rm -rf "$FINAL"
rm -f "$RECEIPT"
# A rename: the bytes verified above are the bytes delivered.
mv "$APP" "$FINAL"

DETAILS=$(codesign -dvvv "$FINAL" 2>&1)
CDHASH=$(printf '%s\n' "$DETAILS" | sed -n 's/^CDHash=//p' | head -1)
REQUIREMENT=$(codesign -d -r- "$FINAL" 2>&1 | sed -n 's/^[# ]*designated => //p')
ARCHIVE_SHA=""
if $ARCHIVE; then
    rm -f "$ZIP"
    ditto -c -k --keepParent "$FINAL" "$ZIP"
    ARCHIVE_SHA=$(standalone_host_sha "$ZIP")
fi
python3 -I - "$RECEIPT" "$BUNDLE_ID" "$INSTANCE" "$VERSION" "$STAMP" "$IDENTITY" "$CDHASH" "$REQUIREMENT" \
    "$(standalone_host_sha "$FINAL/Contents/MacOS/$STANDALONE_HOST_MAIN")" \
    "$(standalone_host_sha "$FINAL/$STANDALONE_HOST_MANIFEST")" \
    "$(standalone_host_tree_sha "$FINAL")" "$($ARCHIVE && basename "$ZIP" || true)" "$ARCHIVE_SHA" "$DIRTY" <<'PY'
import json, sys
out, bundle_id, instance, version, commit, identity, cdhash, requirement, main_sha, manifest_sha, tree_sha, zip_name, zip_sha, dirty = sys.argv[1:15]
receipt = {
    "bundleIdentifier": bundle_id,
    "instance": instance,
    "version": version,
    "commit": commit,
    "dirty": dirty == "true",
    "signing": {
        "identity": "ad-hoc" if identity == "-" else identity,
        "cdhash": cdhash,
        "designatedRequirement": requirement,
        "teamIdentifier": None,
        "hardenedRuntime": False,
        "notarized": False,
    },
    "mainExecutableSha256": main_sha,
    "manifestSha256": manifest_sha,
    "treeSha256": tree_sha,
    "notDone": ["commissioned", "registered at login", "installed", "started"],
}
if zip_name:
    receipt["archive"] = {"name": zip_name, "sha256": zip_sha}
with open(out, "w") as handle:
    json.dump(receipt, handle, indent=2)
    handle.write("\n")
PY

echo "$FINAL"
$DIRTY && echo "DIRTY: built from uncommitted files; this is not commit $STAMP"
echo "commit $STAMP ($BUILD_PROFILE profile), instance $INSTANCE, bundle $BUNDLE_ID"
echo "signed by ${IDENTITY/#-/ad-hoc}, cdhash $CDHASH; kept ${#KEPT[@]:-0} of ${#HELPERS[@]} helpers byte-for-byte"
echo "tree sha256 $(standalone_host_tree_sha "$FINAL")"
$ARCHIVE && echo "archive $ZIP sha256 $ARCHIVE_SHA"
exit 0
