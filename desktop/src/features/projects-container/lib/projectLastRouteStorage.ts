/**
 * Where you were last inside each project, so a hotkey can put you back.
 *
 * ⌥2 is meant to mean "that project", and a project is mostly the session you
 * had open in it — landing on the project's home page every time would make
 * the chord a detour rather than a jump. Per device and persistent across
 * restarts, keyed by (pubkey, relay) like the other project view preferences.
 *
 * A reserved key also remembers the last direct message, which is what ⌥M
 * returns to.
 */

import { getStorageItem, setStorageItem } from "@/shared/lib/safeStorage";

const STORAGE_PREFIX = "buzz.projects.lastRoute.v1";

/** Reserved id for the direct-messages surface, which is not a project. */
export const DM_LAST_ROUTE_KEY = "@dms";

/**
 * Cap on remembered routes. Bounded because the map is keyed by project id and
 * a long-lived install churns through projects; the cap keeps one preference
 * blob from growing without limit.
 */
export const MAX_REMEMBERED_ROUTES = 100;

export type ProjectLastRouteEntry = { href: string; at: number };
export type ProjectLastRouteState = Record<string, ProjectLastRouteEntry>;

export function projectLastRouteStorageKey(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): string {
  return `${STORAGE_PREFIX}:${pubkey ?? "anon"}:${relayUrl ?? "unknown"}`;
}

export function parseProjectLastRouteState(
  raw: string | null,
): ProjectLastRouteState {
  try {
    const parsed: unknown = JSON.parse(raw ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const state: ProjectLastRouteState = {};
    for (const [id, value] of Object.entries(parsed)) {
      if (!value || typeof value !== "object" || Array.isArray(value)) continue;
      const entry = value as Record<string, unknown>;
      if (typeof entry.href !== "string" || entry.href.length === 0) continue;
      state[id] = {
        href: entry.href,
        at:
          typeof entry.at === "number" && Number.isFinite(entry.at)
            ? entry.at
            : 0,
      };
    }
    return state;
  } catch {
    return {};
  }
}

/** Drop the least recently visited entries once the map exceeds the cap. */
export function boundProjectLastRouteState(
  state: ProjectLastRouteState,
): ProjectLastRouteState {
  const entries = Object.entries(state);
  if (entries.length <= MAX_REMEMBERED_ROUTES) return state;
  entries.sort((left, right) => right[1].at - left[1].at);
  return Object.fromEntries(entries.slice(0, MAX_REMEMBERED_ROUTES));
}

export function readProjectLastRouteState(
  pubkey: string | undefined,
  relayUrl: string | undefined,
): ProjectLastRouteState {
  return parseProjectLastRouteState(
    getStorageItem(projectLastRouteStorageKey(pubkey, relayUrl)),
  );
}

export function writeProjectLastRouteState(
  pubkey: string | undefined,
  relayUrl: string | undefined,
  state: ProjectLastRouteState,
): boolean {
  return setStorageItem(
    projectLastRouteStorageKey(pubkey, relayUrl),
    JSON.stringify(boundProjectLastRouteState(state)),
  );
}

/**
 * Record `href` as the current location of `id`.
 *
 * Returns the same reference when nothing changed, so a caller can skip a
 * write on every route tick within the same page.
 */
export function withProjectLastRoute(
  state: ProjectLastRouteState,
  id: string,
  href: string,
  at: number,
): ProjectLastRouteState {
  if (state[id]?.href === href) return state;
  return boundProjectLastRouteState({ ...state, [id]: { href, at } });
}
