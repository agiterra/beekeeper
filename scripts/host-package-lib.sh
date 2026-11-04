# shellcheck shell=bash
# Shared by host-package.sh, host-package-verify.sh and their
# test. Sourced, never run. What belongs in `Beekeeper Host.app`, stated once.

# The app's name on disk, and the file that marks a directory as this package.
STANDALONE_HOST_APP="Beekeeper Host.app"
STANDALONE_HOST_MANIFEST="Contents/Resources/host-package.json"
STANDALONE_HOST_SCHEMA="beekeeper-host-package/v1"
# The outer bundle's executable. Everything else in Contents/MacOS is a sibling
# the Host, the provider, or `bee` finds beside itself.
STANDALONE_HOST_MAIN="beekeeper-menubar"
# Never in this package. Each is either the Desktop product or a sidecar nothing
# on the Host path names: see the assessment's table, and
# docs/standalone-host-package.md for how that was checked.
STANDALONE_HOST_FORBIDDEN=(beekeeper-desktop beekeeper-shell-host beekeeper-backend-kubernetes sprig)

# The helper binaries: exactly the agent stack's list, read out of its packager
# so a binary added there cannot be silently missing here.
standalone_host_helpers() {
    local dir list
    dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    list=$(sed -n 's/^BINARIES=(\(.*\))$/\1/p' "$dir/package-agent-stack.sh")
    [[ -n "$list" ]] || {
        echo "cannot read BINARIES from $dir/package-agent-stack.sh" >&2
        return 1
    }
    printf '%s\n' "$list" | tr ' ' '\n'
}

# A stable digest of every file under a bundle: path and content, sorted bytewise.
standalone_host_tree_sha() {
    (
        cd "$1"
        find . -type f -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' path; do
            printf '%s  %s\n' "$(shasum -a 256 "$path" | cut -d' ' -f1)" "$path"
        done
    ) | shasum -a 256 | cut -d' ' -f1
}

standalone_host_sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
