#!/usr/bin/env bash
# Package the headless agent stack — host, session provider, bee and the
# adapters the provider spawns — as one tarball that runs without the desktop
# app or a Rust toolchain.
#
#   scripts/package-agent-stack.sh [<target-triple>] [--out <dir>]
#
# Build first (the hint below is printed when a binary is missing):
#   cargo build --release --locked [--target <triple>] -p beekeeper-host \
#     -p beekeeper-session-provider -p beekeeper-cli -p beekeeper-acp \
#     -p beekeeper-agent -p beekeeper-dev-mcp -p git-credential-nostr
#
# Output: <out>/beekeeper-agent-stack-<version>-<target>.tar.gz, one top
# directory of the same name:
#   bin/                      the seven binaries
#   share/beekeeper/personas/ roles + templates (what the app bundle ships as
#                             Contents/Resources/personas)
#   stack.json                name, version, target, commit, sha256 per file.
#                             Its presence marks the package root: `bee` finds
#                             share/beekeeper/personas/templates from it.
#   install.sh                installs the unpacked package (see its header)
set -euo pipefail

BINARIES=(beekeeper-host beekeeper-session-provider bee beekeeper-acp beekeeper-agent beekeeper-dev-mcp git-credential-nostr)
BUILD_HINT="cargo build --release --locked -p beekeeper-host -p beekeeper-session-provider -p beekeeper-cli -p beekeeper-acp -p beekeeper-agent -p beekeeper-dev-mcp -p git-credential-nostr"

TARGET=""
OUT="dist"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
        -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
        -*) echo "Unknown option: $1" >&2; exit 2 ;;
        *) TARGET="$1"; shift ;;
    esac
done

cd "$(git rev-parse --show-toplevel)"
HOST=$(rustc -vV | sed -n 's|host: ||p')
# Same rule as bundle-sidecars.sh: an explicit --target puts binaries in
# target/<triple>/release even when it matches the host.
if [[ -n "$TARGET" ]]; then
    SRC_DIR="${CARGO_TARGET_DIR:-target}/${TARGET}/release"
    BUILD_HINT="${BUILD_HINT/--locked/--locked --target $TARGET}"
else
    TARGET="$HOST"
    SRC_DIR="${CARGO_TARGET_DIR:-target}/release"
fi
if [[ "$TARGET" == *windows* ]]; then
    # beekeeper-host refuses to run off Unix (crates/beekeeper-host/src/main.rs).
    echo "Error: the agent stack is Unix-only; $TARGET has no beekeeper-host" >&2
    exit 1
fi

missing=()
for bin in "${BINARIES[@]}"; do
    [[ -f "$SRC_DIR/$bin" ]] || missing+=("$bin")
done
if [[ ${#missing[@]} -gt 0 ]]; then
    echo "Error: missing release binaries in $SRC_DIR: ${missing[*]}" >&2
    echo "Run '$BUILD_HINT' first." >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
COMMIT=$(git rev-parse HEAD)
[[ -n "$VERSION" ]] || { echo "Error: no workspace version in Cargo.toml" >&2; exit 1; }
# The commit is in the name: every crate is 0.1.0, so the version alone would
# give two different stacks the same file name.
NAME="beekeeper-agent-stack-${VERSION}-g${COMMIT:0:9}-${TARGET}"

STAGE=$(mktemp -d)
trap 'rm -r -f "$STAGE"' EXIT
ROOT="$STAGE/$NAME"
mkdir -p "$ROOT/bin" "$ROOT/share/beekeeper/personas"
for bin in "${BINARIES[@]}"; do
    install -m 755 "$SRC_DIR/$bin" "$ROOT/bin/$bin"
done
cp -R personas/roles personas/templates "$ROOT/share/beekeeper/personas/"
install -m 755 scripts/agent-stack-install.sh "$ROOT/install.sh"

# stack.json lists every file but itself. Paths come from this tree, so they
# need no JSON escaping beyond what the check below refuses.
files=()
while IFS= read -r path; do
    case "$path" in *[\"\\]*) echo "Error: unexpected character in $path" >&2; exit 1 ;; esac
    files+=("$(printf '    {"path": "%s", "sha256": "%s"}' "$path" "$(sha256 "$ROOT/$path")")")
done < <(cd "$ROOT" && find . -type f ! -name stack.json | sed 's|^\./||' | LC_ALL=C sort)
{
    printf '{\n  "name": "beekeeper-agent-stack",\n  "version": "%s",\n  "target": "%s",\n  "commit": "%s",\n  "files": [\n' \
        "$VERSION" "$TARGET" "$COMMIT"
    for i in "${!files[@]}"; do
        if [[ $i -lt $((${#files[@]} - 1)) ]]; then printf '%s,\n' "${files[$i]}"; else printf '%s\n' "${files[$i]}"; fi
    done
    printf '  ]\n}\n'
} > "$ROOT/stack.json"

mkdir -p "$OUT"
ARCHIVE="$(cd "$OUT" && pwd)/$NAME.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGE" "$NAME"
echo "$ARCHIVE"
echo "sha256 $(sha256 "$ARCHIVE") (${#files[@]} files)"
