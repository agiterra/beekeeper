import { normalizeRelayUrl } from "@/shared/lib/normalizeRelayUrl";

const STORAGE_KEY_PREFIX = "buzz-project-order.v1";

/**
 * Upper bound on remembered positions. Matches the `limit: 200` the container
 * fetch uses (`fetchProjectContainers`), so the blob can name every project a
 * client can see and no more.
 */
export const MAX_PROJECT_ORDER_ENTRIES = 200;

/**
 * A user's chosen project order: project ids (`<owner-hex>:<dtag>`), most
 * significant first. Projects absent from `order` are not "unordered" — they
 * fall back to the alphabetical base order behind the named ones.
 */
export type ProjectOrderStore = {
  version: 1;
  order: string[];
};

export const DEFAULT_STORE: ProjectOrderStore = Object.freeze({
  version: 1,
  order: [],
}) as ProjectOrderStore;

/**
 * Returns the localStorage key for the project order.
 *
 * Scoped to (pubkey, relay) — normalized via the same `normalizeRelayUrl` used
 * by every relay-scoped local store — because project ids are only meaningful
 * within one community.
 */
export function storageKey(pubkey: string, relayUrl?: string): string {
  if (!relayUrl) return `${STORAGE_KEY_PREFIX}:${pubkey}`;
  const normalized = normalizeRelayUrl(relayUrl);
  // Encode the normalized relay so it can't contain the `:` delimiter.
  return `${STORAGE_KEY_PREFIX}:${pubkey}:${encodeURIComponent(normalized)}`;
}

/**
 * Drops duplicates (first occurrence wins — it is the more significant
 * position) and truncates to `MAX_PROJECT_ORDER_ENTRIES`, keeping the *head*
 * of the list: entries dragged to the top are the ones the user cares about,
 * and the tail is where the blob and the alphabetical fallback differ least.
 */
export function boundProjectOrderStore(
  store: ProjectOrderStore,
): ProjectOrderStore {
  const seen = new Set<string>();
  const order: string[] = [];
  for (const id of store.order) {
    if (seen.has(id)) continue;
    seen.add(id);
    order.push(id);
    if (order.length === MAX_PROJECT_ORDER_ENTRIES) break;
  }
  if (order.length === store.order.length) return store;
  return { ...store, order };
}

export function parseProjectOrderPayload(
  json: unknown,
): ProjectOrderStore | null {
  if (typeof json !== "object" || json === null) return null;
  const obj = json as Record<string, unknown>;
  if (obj.version !== 1) return null;
  if (!Array.isArray(obj.order)) return null;
  const order = obj.order.filter(
    (entry): entry is string => typeof entry === "string" && entry.length > 0,
  );
  return boundProjectOrderStore({ version: 1, order });
}

export function readProjectOrderStore(
  pubkey: string,
  relayUrl?: string,
): ProjectOrderStore {
  try {
    const raw = window.localStorage.getItem(storageKey(pubkey, relayUrl));
    if (!raw) return DEFAULT_STORE;
    return parseProjectOrderPayload(JSON.parse(raw)) ?? DEFAULT_STORE;
  } catch {
    return DEFAULT_STORE;
  }
}

export function writeProjectOrderStore(
  pubkey: string,
  store: ProjectOrderStore,
  relayUrl?: string,
): boolean {
  try {
    window.localStorage.setItem(
      storageKey(pubkey, relayUrl),
      JSON.stringify(boundProjectOrderStore(store)),
    );
    return true;
  } catch {
    return false;
  }
}

/**
 * Permutes `projects` into the user's chosen order.
 *
 * - Projects named in `order` come first, in that order.
 * - Ids in `order` that no longer resolve to a project are skipped. They stay
 *   in the blob rather than being pruned, so a transient fetch that missed a
 *   project does not cost that project its position.
 * - Projects the order never named — created since the last drag, or on
 *   another device — are appended in their incoming order, which is the
 *   alphabetical base order from `sortProjectContainers`.
 *
 * Pure and side-effect-free so it can be unit-tested without a DOM or React.
 */
export function applyProjectOrder<T extends { id: string }>(
  projects: T[],
  order: readonly string[],
): T[] {
  if (order.length === 0) return projects;
  const byId = new Map(projects.map((project) => [project.id, project]));
  const seen = new Set<string>();
  const ordered: T[] = [];

  for (const id of order) {
    const project = byId.get(id);
    if (project && !seen.has(id)) {
      ordered.push(project);
      seen.add(id);
    }
  }
  for (const project of projects) {
    if (!seen.has(project.id)) ordered.push(project);
  }
  return ordered;
}
