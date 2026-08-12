#!/usr/bin/env bash
# Cargo runner for `tauri dev` that disables the system-keyring cargo feature
# (secrets then fall back to 0600 files — see docs/local-desktop-instances.md).
#
# Why a runner wrapper: tauri-cli re-adds the crate's default features as an
# explicit `--no-default-features --features system-keyring` regardless of
# `build.features` in the config or extra runner args (verified empirically
# with `-r /bin/echo`), so the only reliable off-switch is rewriting the
# cargo invocation itself. Everything except the system-keyring feature is
# passed through untouched.
set -euo pipefail

strip_feature() { # $1 = feature-list value; prints the list minus system-keyring
  printf '%s' "$1" | tr ', ' '\n\n' | grep -vx 'system-keyring' | paste -sd, - || true
}

args=()
expect_features=false
for a in "$@"; do
  if $expect_features; then
    expect_features=false
    filtered="$(strip_feature "$a")"
    [[ -n "$filtered" ]] && args+=(--features "$filtered")
    continue
  fi
  case "$a" in
    --features) expect_features=true ;;
    --features=*)
      filtered="$(strip_feature "${a#--features=}")"
      [[ -n "$filtered" ]] && args+=("--features=$filtered")
      ;;
    *) args+=("$a") ;;
  esac
done
exec cargo ${args[@]+"${args[@]}"}
