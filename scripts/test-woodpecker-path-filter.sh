#!/usr/bin/env bash
# Contract tests for the docs-only path filter in .woodpecker/gate.yml.
#
# The filter decides whether a push to `main` runs the gate at all — and since
# `beekeeper-autodeploy` deploys the newest *green pipeline*, a skipped gate is
# also a skipped relay deploy. That makes an over-broad exclusion a silent
# failure: the relay quietly keeps running older code, and the deployer's log
# line for it is indistinguishable from a healthy quiet tick.
#
# So the exclusion list may only ever name files that cannot reach the relay
# image. The trap this exists to prevent is `**/*.md`: sixteen `.md` files live
# under crates/, and `crates/beekeeper-acp/src/base_prompt.md` is `include_str!`'d
# into a `const` — compiled source that happens to end in `.md`.
#
# Pure bash: no toolchain, no python, no GNU-only flags (this runs on the
# buzz-ci image and on developer macs). Seconds.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"
gate=.woodpecker/gate.yml

fail() { echo "FAIL: $*" >&2; exit 1; }

[[ -f "$gate" ]] || fail "missing $gate"

# ── read the exclusion list out of the push rule ─────────────────────────────
# Everything between `exclude:` and the first following line that is not a
# `- '…'` list item. Quotes stripped; comments and blanks ignored.
patterns=$(awk '
  /^[[:space:]]*exclude:[[:space:]]*$/ { inlist = 1; next }
  inlist {
    if ($0 ~ /^[[:space:]]*#/ || $0 ~ /^[[:space:]]*$/) next
    if ($0 !~ /^[[:space:]]*-[[:space:]]*/) { inlist = 0; next }
    gsub(/^[[:space:]]*-[[:space:]]*/, ""); gsub(/^['"'"'"]|['"'"'"]$/, "")
    print
  }
' "$gate")

[[ -n "$patterns" ]] || fail "no exclude patterns found in $gate — did the key move?"

# ── 1. the specific regression: never a global markdown glob ─────────────────
while IFS= read -r pat; do
  [[ "$pat" != '**/*.md' ]] || fail "'**/*.md' in the exclusion list. crates/beekeeper-acp/src/base_prompt.md is include_str!'d into BASE_PROMPT — excluding it skips the rebuild for a real code change. Exclude documentation locations, not an extension."
done <<<"$patterns"

# ── 2. expand the patterns over the tracked tree ─────────────────────────────
# Only the two shapes actually in use are understood. An unrecognised pattern
# is FATAL rather than ignored: silently failing to expand one would report
# "nothing excluded is dangerous" about a pattern never examined, which is the
# same shape of lie the filter itself could tell.
excluded=$(mktemp); trap 'rm -f "$excluded"' EXIT
tracked=$(git ls-files)

while IFS= read -r pat; do
  case "$pat" in
    \*.[A-Za-z]*)                 # *.md          → repo top level only
      ext=${pat#\*}
      grep -E "^[^/]+${ext//./\\.}$" <<<"$tracked" >>"$excluded" || true ;;
    */\*\*/\*.[A-Za-z]*)          # docs/**/*.md  → that extension at any depth
      prefix=${pat%%/**}; ext=${pat##*\*}
      grep -E "^${prefix}/.*${ext//./\\.}$" <<<"$tracked" >>"$excluded" || true ;;
    */\*.[A-Za-z]*)               # docs/*.md     → that extension, one level
      prefix=${pat%/*}; ext=${pat##*\*}
      grep -E "^${prefix}/[^/]+${ext//./\\.}$" <<<"$tracked" >>"$excluded" || true ;;
    */\*\*)                       # docs/**       → everything under docs/
      prefix=${pat%/**}
      grep -E "^${prefix}/" <<<"$tracked" >>"$excluded" || true ;;
    *)
      fail "pattern '$pat' has a shape this test cannot expand. Teach it the shape rather than removing the pattern — an unchecked exclusion is how a rebuild goes missing." ;;
  esac
done <<<"$patterns"

sort -u -o "$excluded" "$excluded"
[[ -s "$excluded" ]] || fail "the exclusion list matches no tracked file — the patterns are wrong, or the paths moved"

# ── 3. nothing excluded may be compiled into a binary ────────────────────────
# Resolve every include_str!/include_bytes! argument relative to its own source
# file, then intersect with the excluded set. Paths really do climb out of the
# crate — crates/beekeeper-db/src/migration.rs embeds ../../../schema/schema.sql —
# so `..` is resolved rather than rejected. No `realpath --relative-to`: that
# flag is GNU-only and this runs on macOS too.
normpath() {  # normpath <path-with-dot-segments>
  local part out=()
  local IFS=/
  for part in $1; do
    case "$part" in
      ''|.) ;;
      ..)   [[ ${#out[@]} -gt 0 ]] || fail "path '$1' climbs above the repo root"
            unset 'out[${#out[@]}-1]' ;;
      *)    out+=("$part") ;;
    esac
  done
  echo "${out[*]}"
}

while IFS= read -r hit; do
  src=${hit%%:*}
  arg=${hit#*\"}; arg=${arg%%\"*}
  [[ "$arg" != /* ]] || fail "include path '$arg' in $src is absolute — resolve it by hand and extend this check."
  embedded=$(normpath "$(dirname "$src")/$arg")
  if grep -Fxq "$embedded" "$excluded"; then
    fail "$embedded is excluded from CI but is embedded into a binary by $src. A change to it alters compiled output while skipping the gate and the relay rebuild."
  fi
done < <(grep -rHo -E 'include_(str|bytes)!\("[^"]+"\)' --include='*.rs' crates/ 2>/dev/null || true)

# ── 4. nothing excluded may live in a relay build input ──────────────────────
# The relay image is `COPY . .` minus .dockerignore, built into beekeeper-relay,
# beekeeper-admin and beekeeper-pair-relay plus the web/admin-web bundles. Anything under
# these paths can change the image and must never be excluded from the gate.
while IFS= read -r f; do
  case "$f" in
    crates/*|migrations/*|schema/*|web/*|admin-web/*|patches/*|Cargo.toml|Cargo.lock|Dockerfile|package.json|pnpm-lock.yaml|pnpm-workspace.yaml)
      fail "$f is excluded from CI but is a relay build input — the image would not be rebuilt for a change to it." ;;
  esac
done <"$excluded"

echo "woodpecker path filter contract passed ($(wc -l <"$excluded" | tr -d ' ') excluded files checked)"
