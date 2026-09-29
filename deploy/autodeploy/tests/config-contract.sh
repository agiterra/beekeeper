#!/usr/bin/env bash
# Contract tests for the per-relay autodeploy config.
#
# There is one relay today (beekeeper on hive; the vanilla buzz relay on
# lightyear was retired 2026-09-29). Woodpecker's pipelines table still holds
# agiterra/buzz history under repo_id 1 on branch `main`, so a config pointing
# at the wrong repo would still select a build — and it would come up healthy,
# because a relay is a relay. These assertions are cheap; that failure is not.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
dir="$repo_root/deploy/autodeploy/etc-default"
units="$repo_root/deploy/autodeploy"

fail() { echo "FAIL: $*" >&2; exit 1; }

value() { grep -m1 "^$2=" "$dir/$1" | cut -d= -f2-; }

cfg=beekeeper-autodeploy

# ── the config sets every required key ───────────────────────────────────────
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

# ── it must point at beekeeper, not at the retired vanilla repo ──────────────
[[ "$(value "$cfg" REPO_ID)" == "2" ]]              || fail "beekeeper must be Woodpecker repo 2 (agiterra/beekeeper); repo 1 is the retired agiterra/buzz"
grep -q "beekeeper" <<<"$(value "$cfg" MIRROR)"     || fail "beekeeper config points at a non-beekeeper mirror"
[[ "$(value "$cfg" INSTANCE)" == "hive" ]]          || fail "beekeeper deploys to the hive instance"

# ── the unit must load its own config, and must not tolerate its absence ─────
unit="$units/$cfg.service"
[[ -f "$unit" ]] || fail "missing unit: $unit"
grep -q "^EnvironmentFile=/etc/default/$cfg$" "$unit" \
  || fail "unit must load /etc/default/$cfg with no '-' prefix (a missing config must fail the unit, not run it unconfigured)"
grep -q "^ExecStart=/usr/local/sbin/autodeploy$" "$unit" \
  || fail "unit must run the shared /usr/local/sbin/autodeploy"

# ── nothing left over from the retired relay ─────────────────────────────────
for f in "$dir/buzz-autodeploy" "$units/buzz-autodeploy.service" "$units/buzz-autodeploy.timer"; do
  [[ ! -e "$f" ]] || fail "$f belongs to the retired vanilla relay; installing it would redeploy lightyear"
done

echo "autodeploy config contract tests passed"
