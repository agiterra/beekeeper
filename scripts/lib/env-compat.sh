#!/usr/bin/env bash
# Shell twin of beekeeper_core::env_compat::adopt_legacy_env, for scripts and
# Justfile recipes that read BEEKEEPER_* names themselves (port defaults, relay
# URLs) rather than leaving them to a binary.
#
#   source scripts/lib/env-compat.sh
#   beekeeper_adopt_legacy_env "<who>"
#
# For every BUZZ_<X> variable (set, exported or not) whose BEEKEEPER_<X> is
# unset, export BEEKEEPER_<X> with the same value. The BUZZ_ variable is never
# removed, and a BEEKEEPER_<X> that is already set always wins. Prints one
# stderr line naming the adopted variables (names only, never values) so an
# old .env is visible; `just env-migrate` rewrites it.
beekeeper_adopt_legacy_env() {
  local who="${1:-script}" legacy canonical
  local adopted=()
  while IFS= read -r legacy; do
    [[ "$legacy" =~ ^BUZZ_[A-Za-z0-9_]+$ ]] || continue
    canonical="BEEKEEPER_${legacy#BUZZ_}"
    if [[ -z "${!canonical+x}" ]]; then
      export "${canonical}=${!legacy}"
      adopted+=("$legacy")
    fi
  done < <(compgen -v BUZZ_ || true)
  if [[ ${#adopted[@]} -gt 0 ]]; then
    echo "${who}: read legacy ${adopted[*]} as BEEKEEPER_*; run \`just env-migrate\` to rename them" >&2
  fi
}
