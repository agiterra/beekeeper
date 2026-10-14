#!/usr/bin/env bash
# Headless crates must not depend on Tauri.
#
# The desktop is one client of Beekeeper, not its executor: every crate under
# `crates/` (host, session provider, agent management, CLI, relay) has to build
# and run on a server with no GUI toolkit anywhere. The plan that moved agent
# execution out of the desktop (BEEKEEPER_HEADLESS_EXTENSIBILITY_PLAN § 6)
# requires a dependency check proving that, and a doc comment is not a check.
#
# Reads the fully resolved workspace graph (the desktop crate is excluded from
# the workspace, so it is not in it) and fails on any resolved package whose
# name starts with `tauri`. Needs no network when Cargo.lock is current.
set -euo pipefail
cd "$(dirname "$0")/.."
metadata() {
  cargo metadata --format-version 1 --locked --offline 2>/dev/null \
    || cargo metadata --format-version 1 --locked
}
metadata | python3 -I -c '
import json, sys
meta = json.load(sys.stdin)
hits = sorted({p["name"] for p in meta["packages"] if p["name"].startswith("tauri")})
if hits:
    print("no-tauri-check: workspace resolves Tauri packages: " + ", ".join(hits), file=sys.stderr)
    sys.exit(1)
print("no-tauri-check: %d workspace packages, no tauri dependency" % len(meta["workspace_members"]))
'
