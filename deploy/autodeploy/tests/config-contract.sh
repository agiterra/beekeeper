#!/usr/bin/env bash
# Contract tests for the per-relay autodeploy configs.
#
# One script now serves both relays, so the configs are the only thing keeping
# them apart. A swapped, duplicated or half-edited config would put one
# product's build on the other's relay — and it would come up healthy, because
# a relay is a relay. These assertions are cheap; that failure is not.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
dir="$repo_root/deploy/autodeploy/etc-default"
units="$repo_root/deploy/autodeploy"

fail() { echo "FAIL: $*" >&2; exit 1; }

value() { grep -m1 "^$2=" "$dir/$1" | cut -d= -f2-; }

# ── every config sets every required key ─────────────────────────────────────
for cfg in buzz-autodeploy beekeeper-autodeploy; do
  [[ -f "$dir/$cfg" ]] || fail "missing config: $cfg"
  for key in AUTODEPLOY_NAME REPO_ID MIRROR INSTANCE BASE IMAGE_NAME; do
    v=$(value "$cfg" "$key")
    [[ -n "$v" ]] || fail "$cfg: $key is unset or empty"
  done
  # systemd parses EnvironmentFile itself — it is not a shell. `export`,
  # command substitution and variable expansion all silently do the wrong
  # thing, so reject them rather than discover it in production.
  #
  # Only assignment lines are examined: systemd ignores comments, and prose
  # legitimately contains backticks and the like. (Checking the whole file
  # first meant this test failed on its own explanatory comments.)
  settings=$(grep -E '^[A-Za-z_][A-Za-z0-9_]*=' "$dir/$cfg" || true)
  grep -qE '^\s*export '  <<<"$settings" && fail "$cfg: 'export' — systemd is not a shell here"
  grep -qE '\$\(|`|\$\{'  <<<"$settings" && fail "$cfg: substitution/expansion — systemd is not a shell here"
done

# ── the two must differ everywhere it matters ────────────────────────────────
# REPO_ID above all: Woodpecker serves both repos and both use branch `main`,
# so an unpinned or duplicated repo_id selects whichever pushed most recently.
for key in AUTODEPLOY_NAME REPO_ID MIRROR INSTANCE BASE IMAGE_NAME; do
  a=$(value buzz-autodeploy "$key")
  b=$(value beekeeper-autodeploy "$key")
  [[ "$a" != "$b" ]] || fail "$key is identical in both configs ('$a') — they would share a target"
done

# ── the pairing must be internally consistent ────────────────────────────────
[[ "$(value buzz-autodeploy REPO_ID)" == "1" ]]           || fail "buzz must be Woodpecker repo 1 (agiterra/buzz)"
[[ "$(value beekeeper-autodeploy REPO_ID)" == "2" ]]      || fail "beekeeper must be Woodpecker repo 2 (agiterra/beekeeper)"
grep -q "buzz" <<<"$(value buzz-autodeploy MIRROR)"       || fail "buzz config points at a non-buzz mirror"
grep -q "beekeeper" <<<"$(value beekeeper-autodeploy MIRROR)" || fail "beekeeper config points at a non-beekeeper mirror"

# ── each unit must load its own config, and must not tolerate its absence ────
for name in buzz beekeeper; do
  unit="$units/$name-autodeploy.service"
  [[ -f "$unit" ]] || fail "missing unit: $unit"
  grep -q "^EnvironmentFile=/etc/default/$name-autodeploy$" "$unit" \
    || fail "$name unit must load /etc/default/$name-autodeploy with no '-' prefix (a missing config must fail the unit, not run it unconfigured)"
  grep -q "^ExecStart=/usr/local/sbin/autodeploy$" "$unit" \
    || fail "$name unit must run the shared /usr/local/sbin/autodeploy"
done

echo "autodeploy config contract tests passed"
