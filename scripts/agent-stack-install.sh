#!/bin/sh
# install.sh — install this unpacked agent-stack package (scripts/package-agent-stack.sh ships this file as install.sh).
#
#   ./install.sh [--no-service] [--restart]
#
# 1. Verifies every file against stack.json; one mismatch installs nothing.
# 2. Copies the package to $BEEKEEPER_STACK_PREFIX/<name> (default ~/.local/lib/beekeeper), unless it runs from there.
# 3. Points <prefix>/current at it. Earlier versions stay beside it: rolling back is running their install.sh.
# 4. Links each bin/* into $BEEKEEPER_BIN_DIR (default ~/.local/bin) through current, so an upgrade moves them all.
# 5. Registers the host at login: `bee host install --program <prefix>/current/bin/beekeeper-host`, a path that
#    stays valid across upgrades. --no-service skips it (a container runs `beekeeper-host run` in the foreground).
#
# A host that is already running keeps its binary until it restarts. --restart restarts it (`bee host restart`);
# without it the command is printed. Never restarted silently: a restart ends the executions it is running.
set -eu

here=$(cd "$(dirname "$0")" && pwd -P)
name=$(basename "$here")
prefix=${BEEKEEPER_STACK_PREFIX:-$HOME/.local/lib/beekeeper}
bindir=${BEEKEEPER_BIN_DIR:-$HOME/.local/bin}
service=1
restart=0
for arg in "$@"; do
    case "$arg" in
        --no-service) service=0 ;;
        --restart) restart=1 ;;
        -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
        *) echo "install.sh: unknown option $arg" >&2; exit 2 ;;
    esac
done

if command -v sha256sum >/dev/null 2>&1; then
    sum() { sha256sum "$1" | cut -d' ' -f1; }
else
    sum() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

[ -f "$here/stack.json" ] || { echo "install.sh: no stack.json beside $0; this is not an unpacked package" >&2; exit 1; }
checked=0
bad=0
entries=$(sed -n 's/^ *{"path": "\([^"]*\)", "sha256": "\([0-9a-f]*\)"}.*$/\1 \2/p' "$here/stack.json")
while read -r path want; do
    [ -n "$path" ] || continue
    if [ ! -f "$here/$path" ]; then echo "install.sh: missing $path" >&2; bad=$((bad + 1))
    elif [ "$(sum "$here/$path")" != "$want" ]; then echo "install.sh: sha256 mismatch: $path" >&2; bad=$((bad + 1))
    fi
    checked=$((checked + 1))
done <<EOF
$entries
EOF
[ "$checked" -gt 0 ] || { echo "install.sh: stack.json lists no files" >&2; exit 1; }
[ "$bad" -eq 0 ] || { echo "install.sh: $bad of $checked files failed verification; nothing installed" >&2; exit 1; }
echo "verified $checked files against stack.json"

mkdir -p "$prefix" "$bindir"
dest="$prefix/$name"
if [ "$here" = "$(cd "$dest" 2>/dev/null && pwd -P || true)" ]; then
    :   # running from the installed copy (a rollback, or a re-run)
elif [ -d "$dest" ]; then
    # The name carries the commit and target, so an existing copy is the same build;
    # its manifest is compared anyway, never assumed.
    cmp -s "$here/stack.json" "$dest/stack.json" \
        || { echo "install.sh: $dest exists with a different stack.json; remove it or install elsewhere" >&2; exit 1; }
else
    # Copy beside, then rename: a half-copied package is never at $dest.
    tmp="$prefix/.$name.partial.$$"
    cp -R "$here" "$tmp"
    mv "$tmp" "$dest"
fi

# Relative, so the prefix can move. Not atomic (ln -sfn unlinks first); the gap is
# only visible to a process resolving `current` in that instant.
ln -sfn "$name" "$prefix/current"
for bin in "$prefix/current/bin/"*; do
    ln -sfn "$prefix/current/bin/$(basename "$bin")" "$bindir/$(basename "$bin")"
done
echo "installed $name -> $prefix/current; binaries linked into $bindir"
case ":$PATH:" in *":$bindir:"*) ;; *) echo "note: $bindir is not on PATH" ;; esac

if [ "$service" -eq 1 ]; then
    "$bindir/bee" host install --program "$prefix/current/bin/beekeeper-host"
fi
if [ "$restart" -eq 1 ]; then
    "$bindir/bee" host restart
else
    echo "a host that was already running keeps its old binary until: $bindir/bee host restart"
fi
