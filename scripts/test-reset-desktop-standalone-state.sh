#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export HOME="$tmp/home"
export BUZZ_TEST_PLATFORM=Darwin
mkdir -p "$HOME/Library/Application Support/io.agiterra.beekeeper.dev.example"
mkdir -p "$HOME/Library/Application Support/io.agiterra.beekeeper.dev.other"
mkdir -p "$HOME/Library/Application Support/io.agiterra.beekeeper"
mkdir -p "$HOME/.beekeeper-dev"
touch "$HOME/.beekeeper-dev/keep"
mkdir -p "$tmp/bin"
cat > "$tmp/bin/security" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$HOME/security-calls"
exit 1
MOCK
chmod +x "$tmp/bin/security"
export PATH="$tmp/bin:$PATH"

"$repo_root/scripts/reset-desktop-standalone-state.sh" \
    io.agiterra.beekeeper.dev.example beekeeper-desktop-dev.example

[[ ! -e "$HOME/Library/Application Support/io.agiterra.beekeeper.dev.example" ]]
[[ -d "$HOME/Library/Application Support/io.agiterra.beekeeper.dev.other" ]]
[[ -d "$HOME/Library/Application Support/io.agiterra.beekeeper" ]]
[[ -f "$HOME/.beekeeper-dev/keep" ]]
grep -Fx -- "delete-generic-password -s beekeeper-desktop-dev.example" "$HOME/security-calls" >/dev/null

if "$repo_root/scripts/reset-desktop-standalone-state.sh" \
    io.agiterra.beekeeper buzz-desktop >/dev/null 2>&1; then
    echo "expected production scope guard to reject reset" >&2
    exit 1
fi

echo "standalone desktop reset scope test passed"
