#!/usr/bin/env bash
# Rename the pre-rename BUZZ_<X>= keys in a local .env to BEEKEEPER_<X>=, and
# rewrite pre-rename `buzz_*` tracing targets, in one pass with one .env.bak.
#
#   scripts/env-migrate.sh [FILE]     (default: .env; `just env-migrate`)
#
# Nothing here is required: every binary still reads a BUZZ_<X> whose
# BEEKEEPER_<X> is unset, and prints one stderr line naming the legacy names it
# adopted (beekeeper_core::env_compat). This makes that line go away.
#
# Names that Docker Compose interpolates itself are left alone, because
# compose.yml still spells them BUZZ_*, and renaming one would make Compose
# substitute an empty string:
#   - always: BUZZ_IMAGE, BUZZ_DOMAIN, BUZZ_COMPOSE_TLS, BUZZ_COMPOSE_DEV,
#     BUZZ_HTTP_PORT (deploy/compose and its run.sh);
#   - also any BUZZ_<X> that a compose*.yml / docker-compose*.yml beside FILE
#     interpolates as ${BUZZ_<X>...} (deploy/compose/.env: the S3 credentials,
#     BUZZ_AUTO_MIGRATE, BUZZ_GIT_CONFORMANCE_PROBE).
#
# A BUZZ_<X> line whose BEEKEEPER_<X> is already set in FILE is dropped: the new
# name wins at runtime, so the old line was already dead. Commented-out lines
# (`# BUZZ_X=...`) are renamed too, so the file's documentation stays true.
# Values are never printed. Idempotent: a second run changes nothing.
set -euo pipefail

FILE="${1:-.env}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -f "$FILE" ]]; then
  echo "no $FILE here; nothing to do"
  exit 0
fi

SKIP=(BUZZ_IMAGE BUZZ_DOMAIN BUZZ_COMPOSE_TLS BUZZ_COMPOSE_DEV BUZZ_HTTP_PORT)
dir="$(dirname "$FILE")"
beside_compose=false
shopt -s nullglob
for compose in "$dir"/compose*.yml "$dir"/compose*.yaml "$dir"/docker-compose*.yml "$dir"/docker-compose*.yaml; do
  beside_compose=true
  while IFS= read -r name; do
    SKIP+=("$name")
  done < <(grep -oE '\$\{BUZZ_[A-Z0-9_]+' "$compose" | sed 's/^\${//' | sort -u)
done
shopt -u nullglob

# A .env beside a compose file is a deployment's, read by containers that may
# run a pre-rename image (a pinned BUZZ_IMAGE, or an autodeploy rollback). Such
# an image reads only BUZZ_* names, so renaming BUZZ_RELAY_PRIVATE_KEY here
# would roll back to a relay with no key. Refuse unless the operator says every
# image that could start from this file is post-rename.
if $beside_compose && [[ "${ENV_MIGRATE_DEPLOY:-}" != "1" ]]; then
  echo "$FILE sits beside a compose file: it is a deployment .env, and a pre-rename"
  echo "image started from it reads only BUZZ_* names. Left unchanged; the relay reads"
  echo "BUZZ_* names already. Re-run with ENV_MIGRATE_DEPLOY=1 once no older image can start."
  exit 0
fi

# The tracing-target program the relay deployer applies to the live .env, read
# out of deploy/autodeploy/autodeploy so the two cannot disagree (the same one
# `just env-log-targets` runs). It matches RUST_LOG and BUZZ_OTEL_FILTER, so it
# runs before the keys are renamed.
LOG_TARGET_SED=""
autodeploy="$REPO_ROOT/deploy/autodeploy/autodeploy"
if [[ -f "$autodeploy" ]]; then
  eval "$(sed -n '/^# >>> log-target-sed$/,/^# <<< log-target-sed$/p' "$autodeploy")"
fi

tmp="$(mktemp "${FILE}.migrate.XXXXXX")"
trap 'rm -f "$tmp" "$tmp.keys" "$tmp.out"' EXIT

if [[ -n "$LOG_TARGET_SED" ]]; then
  sed -E "$LOG_TARGET_SED" "$FILE" > "$tmp"
else
  cp "$FILE" "$tmp"
fi
log_targets_rewritten=false
cmp -s "$FILE" "$tmp" || log_targets_rewritten=true

# Key rename. awk prints the new file on stdout and one "renamed X" /
# "dropped X" line per key on fd 3 (names only).
awk -v skip="${SKIP[*]}" '
  BEGIN {
    n = split(skip, s, " ")
    for (i = 1; i <= n; i++) skipped[s[i]] = 1
  }
  # First pass: which BEEKEEPER_<X> keys are already set (uncommented).
  NR == FNR {
    if (match($0, /^[[:space:]]*(export[[:space:]]+)?BEEKEEPER_[A-Z0-9_]+=/)) {
      key = substr($0, RSTART, RLENGTH - 1)
      sub(/^[[:space:]]*(export[[:space:]]+)?/, "", key)
      have[key] = 1
    }
    next
  }
  {
    line = $0
    if (match(line, /^[[:space:]]*(#[[:space:]]*)?(export[[:space:]]+)?BUZZ_[A-Z0-9_]+=/)) {
      head = substr(line, 1, RLENGTH)
      rest = substr(line, RLENGTH + 1)
      key = head
      sub(/^[[:space:]]*(#[[:space:]]*)?(export[[:space:]]+)?/, "", key)
      sub(/=$/, "", key)
      commented = (head ~ /^[[:space:]]*#/)
      if (!(key in skipped)) {
        newkey = "BEEKEEPER_" substr(key, 6)
        if (!commented && (newkey in have)) {
          print "dropped " key " (" newkey " is already set)" > "/dev/fd/3"
          next
        }
        prefix = substr(head, 1, length(head) - length(key) - 1)
        print prefix newkey "=" rest
        if (!commented) print "renamed " key " -> " newkey > "/dev/fd/3"
        next
      }
    }
    print line
  }
' "$tmp" "$tmp" 3> "$tmp.keys" > "$tmp.out"
mv "$tmp.out" "$tmp"

if cmp -s "$FILE" "$tmp"; then
  echo "$FILE: already current"
  exit 0
fi

cp -p "$FILE" "$FILE.bak"
# Keep the original's mode (a .env holds keys; ensure-local-relay-key.sh makes it 0600).
cat "$tmp" > "$FILE"
echo "$FILE: migrated (the original is in $FILE.bak)"
if [[ -s "$tmp.keys" ]]; then
  sed 's/^/  /' "$tmp.keys"
fi
if [[ "$log_targets_rewritten" == true ]]; then
  echo "  rewrote pre-rename log targets"
fi
skipped_present=()
for name in "${SKIP[@]}"; do
  grep -qE "^[[:space:]]*(export[[:space:]]+)?${name}=" "$FILE" && skipped_present+=("$name")
done
if [[ ${#skipped_present[@]} -gt 0 ]]; then
  echo "  kept for Compose: ${skipped_present[*]}"
fi
