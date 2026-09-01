/**
 * The live per-project "where you were" map.
 *
 * Module-level for the same reason the hotkey bindings are: the writer is a
 * route-watching effect in the shell and the readers are the sidebar (which
 * binds each project's `activate`) and the hotkey dispatcher. Threading one
 * small lookup through both prop chains would touch a dozen components to move
 * a string.
 *
 * Community-scoped — cleared by `resetCommunityState()`.
 */

import * as React from "react";

import {
  readProjectLastRouteState,
  withProjectLastRoute,
  writeProjectLastRouteState,
  type ProjectLastRouteState,
} from "./projectLastRouteStorage";

let state: ProjectLastRouteState = {};
let identity: { pubkey?: string; relayUrl?: string } | null = null;

/** The remembered href for a project (or `DM_LAST_ROUTE_KEY`), if any. */
export function getRememberedRoute(id: string): string | null {
  return state[id]?.href ?? null;
}

/**
 * Record `href` as the current location of `id`. Writes only when the href
 * actually moved, so a re-render storm on one page costs no storage traffic.
 */
export function rememberRoute(id: string, href: string, at: number): void {
  const next = withProjectLastRoute(state, id, href, at);
  if (next === state) return;
  state = next;
  if (identity) {
    writeProjectLastRouteState(identity.pubkey, identity.relayUrl, next);
  }
}

export function resetProjectRouteMemory(): void {
  state = {};
  identity = null;
}

/** Load the remembered routes for an identity. Mount once, in the shell. */
export function useProjectRouteMemorySync(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): void {
  React.useEffect(() => {
    identity = { pubkey, relayUrl };
    state = readProjectLastRouteState(pubkey, relayUrl);
  }, [pubkey, relayUrl]);
}
