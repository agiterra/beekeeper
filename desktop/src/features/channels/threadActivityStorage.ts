import { normalizeRelayUrl } from "@/features/profile/lib/selfProfileStorage";

export type ThreadActivityItem = {
  id: string;
  kind: number;
  pubkey: string;
  content: string;
  createdAt: number;
  channelId: string;
  channelName: string;
  tags: string[][];
};

export const ACTIVITY_STORAGE_PREFIX = "buzz-thread-activity.v1";
/**
 * Tombstones: ids the relay has positively confirmed it does not hold.
 *
 * Kept in a separate, tiny key rather than inside the item blob so the write
 * path can consult it without re-parsing the (up to ~600 KB) item payload on
 * every coalesced write.
 */
export const ABSENT_ACTIVITY_STORAGE_PREFIX = "buzz-thread-activity-absent.v1";
const MAX_ACTIVITY_ITEMS = 100;
/**
 * Tombstone ring size. The item store is capped at 100, so 200 covers the
 * live set twice over: every id that could still be in the buffer stays
 * censored even after a full turnover of items.
 */
const MAX_ABSENT_IDS = 200;

// Scoped to relay+pubkey. The legacy pubkey-only key is intentionally not read
// — rows from an unknown relay cannot be safely attributed.
export function activityStorageKey(pubkey: string, relayUrl: string): string {
  return `${ACTIVITY_STORAGE_PREFIX}:${normalizeRelayUrl(relayUrl)}:${pubkey}`;
}

/** Companion key holding the confirmed-absent ids for the same scope. */
export function absentActivityStorageKey(
  pubkey: string,
  relayUrl: string,
): string {
  return `${ABSENT_ACTIVITY_STORAGE_PREFIX}:${normalizeRelayUrl(relayUrl)}:${pubkey}`;
}

/**
 * Stable identity string for the in-memory thread-activity buffer.
 * A single definition avoids the `${pubkey}:${relay}` expression being
 * repeated at reset, both writers, and the render fence.
 * Returns `""` when either value is absent — the empty string never matches a
 * valid loaded scope, so the fence returns `[]` until the buffer is seeded.
 */
export function activityScopeKey(
  pubkey: string | null,
  relayUrl: string,
): string {
  if (!pubkey || !relayUrl) return "";
  return `${pubkey}:${normalizeRelayUrl(relayUrl)}`;
}

/**
 * Pure projection helper used at the hook return and in tests.
 *
 * Returns `items` only when both scopes are non-empty and identical.
 * An empty `currentScope` (absent pubkey or relay) is always rejected —
 * `activityScopeKey()` returns `""` in that case, and `""` must never
 * compare-equal to a valid loaded scope even if the ref was initialised to
 * `""` before the first reset effect commits.
 */
export function projectActivityForScope(
  loadedScope: string,
  currentScope: string,
  items: ThreadActivityItem[],
): ThreadActivityItem[] {
  if (!currentScope || loadedScope !== currentScope) return [];
  return items;
}

// ─── Persisted payload shapes ────────────────────────────────────────────────
//
// The blob is `{ updatedAt, items }`, NOT a bare array. That matters beyond
// tidiness: localStorageSweep reads `updatedAt` off the JSON *root* to decide
// staleness, and its parser returns null for an array — so while this key held
// a bare array it was structurally un-sweepable no matter what TTL rule was
// registered for the prefix. Pre-envelope blobs are migrated on first read.

type ParsedActivityPayload = {
  /** Root `updatedAt`, or null for the legacy array form. */
  updatedAt: number | null;
  items: ThreadActivityItem[];
  legacy: boolean;
};

function coerceActivityItems(value: unknown[]): ThreadActivityItem[] {
  return value.filter(
    (item): item is ThreadActivityItem =>
      typeof item === "object" &&
      item !== null &&
      typeof (item as ThreadActivityItem).id === "string",
  );
}

function parseActivityPayload(
  raw: string | null,
): ParsedActivityPayload | null {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (Array.isArray(parsed)) {
    return {
      updatedAt: null,
      items: coerceActivityItems(parsed),
      legacy: true,
    };
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const record = parsed as { updatedAt?: unknown; items?: unknown };
  if (!Array.isArray(record.items)) return null;
  return {
    updatedAt:
      typeof record.updatedAt === "number" && Number.isFinite(record.updatedAt)
        ? record.updatedAt
        : null,
    items: coerceActivityItems(record.items),
    legacy: false,
  };
}

/**
 * Age to stamp on a migrated legacy blob: the newest item's own creation time,
 * never later than now. Stamping `Date.now()` instead would reset the TTL clock
 * on exactly the abandoned blobs the sweep exists to collect — a store whose
 * newest row is from July must present as a July-old entry, not a fresh one.
 */
function deriveLegacyUpdatedAt(
  items: ThreadActivityItem[],
  now: number,
): number {
  let newest = 0;
  for (const item of items) {
    if (typeof item.createdAt === "number" && Number.isFinite(item.createdAt)) {
      newest = Math.max(newest, item.createdAt * 1_000);
    }
  }
  return newest > 0 ? Math.min(newest, now) : now;
}

function writeActivityPayload(
  key: string,
  items: ThreadActivityItem[],
  updatedAt: number,
): void {
  const capped =
    items.length > MAX_ACTIVITY_ITEMS
      ? items.slice(items.length - MAX_ACTIVITY_ITEMS)
      : items;
  window.localStorage.setItem(
    key,
    JSON.stringify({ updatedAt, items: capped }),
  );
}

/**
 * Read the persisted rows for a scope, minus any id the relay has positively
 * confirmed it does not hold.
 *
 * The tombstone filter is applied here, at hydration, rather than only in the
 * live projection: a row whose event no longer exists must never be handed to
 * the UI at all, not even for the seconds before an async reconcile completes.
 */
export function readActivityFromStorage(
  pubkey: string,
  relayUrl: string,
): ThreadActivityItem[] {
  try {
    const key = activityStorageKey(pubkey, relayUrl);
    const parsed = parseActivityPayload(window.localStorage.getItem(key));
    if (!parsed) return [];

    const absentIds = readAbsentThreadActivityIds(pubkey, relayUrl);
    const visible =
      absentIds.size === 0
        ? parsed.items
        : parsed.items.filter((item) => !absentIds.has(item.id));

    if (parsed.legacy) {
      // One-time in-place upgrade so the sweep rule can see this key's age.
      try {
        writeActivityPayload(
          key,
          visible,
          deriveLegacyUpdatedAt(parsed.items, Date.now()),
        );
      } catch {
        // Migration is best-effort; the rows are still returned.
      }
    }

    return visible;
  } catch {
    return [];
  }
}

export function writeActivityToStorage(
  pubkey: string,
  relayUrl: string,
  items: ThreadActivityItem[],
): void {
  try {
    // The in-memory buffer is never pruned in place (it belongs to
    // useUnreadChannels), so without this filter the next coalesced write would
    // resurrect every tombstoned row straight back into storage.
    const absentIds = readAbsentThreadActivityIds(pubkey, relayUrl);
    const visible =
      absentIds.size === 0
        ? items
        : items.filter((item) => !absentIds.has(item.id));
    writeActivityPayload(
      activityStorageKey(pubkey, relayUrl),
      visible,
      Date.now(),
    );
  } catch {
    // Ignore storage errors.
  }
}

export function addThreadActivityItems(
  existing: ThreadActivityItem[],
  items: ThreadActivityItem[],
) {
  if (items.length === 0) {
    return { didAdd: false, items: existing };
  }

  const existingIds = new Set(existing.map((item) => item.id));
  const newItems = items.filter((item) => !existingIds.has(item.id));
  if (newItems.length === 0) {
    return { didAdd: false, items: existing };
  }

  const merged = [...existing, ...newItems].sort(
    (left, right) => left.createdAt - right.createdAt,
  );
  const capped =
    merged.length > MAX_ACTIVITY_ITEMS
      ? merged.slice(merged.length - MAX_ACTIVITY_ITEMS)
      : merged;

  return { didAdd: true, items: capped };
}

/**
 * One-time removal of the orphaned pre-relay-scoping key
 * `buzz-thread-activity.v1:<pubkey>`. The legacy pubkey-only writer was dropped
 * when the key became relay-scoped, but the old ~400 KB blob is never read and
 * is absent from the quota-sweep whitelist, so it lingers as dead weight.
 * removeItem on an absent key is a no-op, so running this on every identity
 * load is idempotent and self-terminating.
 */
export function removeLegacyThreadActivityKey(pubkey: string): void {
  try {
    window.localStorage.removeItem(`${ACTIVITY_STORAGE_PREFIX}:${pubkey}`);
  } catch {
    // Ignore storage errors.
  }
}

/**
 * Drop every thread-activity key (rows and tombstones) belonging to one relay
 * URL, across all local identities.
 *
 * Used by the relay-identity guard: this store is keyed by relay *URL*, so a
 * relay reinstalled at a familiar address would otherwise inherit the previous
 * instance's rows and present them as live Inbox items forever.
 */
export function removeThreadActivityForRelay(relayUrl: string): void {
  try {
    const normalized = normalizeRelayUrl(relayUrl);
    const prefixes = [
      `${ACTIVITY_STORAGE_PREFIX}:${normalized}:`,
      `${ABSENT_ACTIVITY_STORAGE_PREFIX}:${normalized}:`,
    ];
    const doomed: string[] = [];
    for (let i = 0; i < window.localStorage.length; i++) {
      const key = window.localStorage.key(i);
      if (key && prefixes.some((prefix) => key.startsWith(prefix))) {
        doomed.push(key);
      }
    }
    // Collect before mutating: localStorage indexes shift on removal.
    for (const key of doomed) {
      window.localStorage.removeItem(key);
    }
  } catch {
    // Ignore storage errors.
  }
}

// ─── Confirmed-absent tombstones ─────────────────────────────────────────────

function parseAbsentIds(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== "object" || parsed === null) return [];
    const ids = (parsed as { ids?: unknown }).ids;
    if (!Array.isArray(ids)) return [];
    return ids.filter((id): id is string => typeof id === "string");
  } catch {
    return [];
  }
}

/** Ids the relay has positively confirmed absent for this scope. */
export function readAbsentThreadActivityIds(
  pubkey: string,
  relayUrl: string,
): ReadonlySet<string> {
  try {
    return new Set(
      parseAbsentIds(
        window.localStorage.getItem(absentActivityStorageKey(pubkey, relayUrl)),
      ),
    );
  } catch {
    return new Set<string>();
  }
}

/**
 * Record ids the relay has positively confirmed it does not hold, and prune
 * them out of the persisted rows.
 *
 * Callers MUST only pass ids for which a relay query completed successfully and
 * returned no matching event. An unreachable, erroring, or rate-limited relay
 * proves nothing and must never reach this function.
 */
export function recordAbsentThreadActivityIds(
  pubkey: string,
  relayUrl: string,
  ids: readonly string[],
): void {
  if (ids.length === 0) return;
  try {
    const key = absentActivityStorageKey(pubkey, relayUrl);
    const incoming = new Set(ids);
    const merged = [
      ...parseAbsentIds(window.localStorage.getItem(key)).filter(
        (id) => !incoming.has(id),
      ),
      ...incoming,
    ];
    const capped =
      merged.length > MAX_ABSENT_IDS
        ? merged.slice(merged.length - MAX_ABSENT_IDS)
        : merged;
    window.localStorage.setItem(
      key,
      JSON.stringify({ updatedAt: Date.now(), ids: capped }),
    );

    // Reclaim the bytes now. Correctness does not depend on this succeeding —
    // the read and write paths both filter on the tombstones above.
    const itemsKey = activityStorageKey(pubkey, relayUrl);
    const parsed = parseActivityPayload(window.localStorage.getItem(itemsKey));
    if (!parsed) return;
    const censored = new Set(capped);
    const kept = parsed.items.filter((item) => !censored.has(item.id));
    if (kept.length === parsed.items.length) return;
    // Preserve the existing age: pruning dead rows is not fresh activity, so it
    // must not reset the TTL clock on an otherwise abandoned blob.
    writeActivityPayload(itemsKey, kept, parsed.updatedAt ?? Date.now());
  } catch {
    // Ignore storage errors.
  }
}

// ─── Relay reconciliation ────────────────────────────────────────────────────

/**
 * Ask the relay which of `ids` it actually holds. Resolves with the ids the
 * relay returned; rejects (or throws) on anything else. The distinction is the
 * whole safety property — see {@link findAbsentThreadActivityIds}.
 */
export type ThreadActivityExistenceProbe = (input: {
  ids: string[];
  kinds: number[];
}) => Promise<readonly { id: string }[]>;

/** Ids per REQ. Keeps a full 100-row store to four bounded queries. */
export const RECONCILE_CHUNK_SIZE = 25;

export type ThreadActivityReconcileResult = {
  /** Ids the relay completed a query for and did not return. */
  absentIds: string[];
  /** False when a probe failed and some ids were left unjudged. */
  complete: boolean;
};

/**
 * Partition persisted rows into "still on the relay" and "confirmed gone".
 *
 * **Fails closed by construction.** An id is judged absent only when a probe
 * *resolves* — i.e. the relay accepted the filter and completed the result set
 * — and did not include it. Every other outcome (rejection, timeout, CLOSED,
 * rate-limit, malformed response) aborts the pass and leaves the remaining ids
 * untouched, so a flaky network can only ever under-evict, never over-evict.
 */
export async function findAbsentThreadActivityIds(
  items: readonly ThreadActivityItem[],
  probe: ThreadActivityExistenceProbe,
  chunkSize: number = RECONCILE_CHUNK_SIZE,
): Promise<ThreadActivityReconcileResult> {
  const byId = new Map<string, ThreadActivityItem>();
  for (const item of items) {
    if (typeof item.id === "string" && item.id !== "") byId.set(item.id, item);
  }
  const ids = [...byId.keys()];
  const absentIds: string[] = [];
  if (ids.length === 0) return { absentIds, complete: true };

  const size = Math.max(1, chunkSize);
  for (let start = 0; start < ids.length; start += size) {
    const chunk = ids.slice(start, start + size);
    const kinds = [
      ...new Set(
        chunk.flatMap((id) => {
          const kind = byId.get(id)?.kind;
          return typeof kind === "number" ? [kind] : [];
        }),
      ),
    ];
    // A chunk with no usable kinds cannot be queried: the relay's p-gate
    // rejects kind-less filters, so an empty `kinds` would produce a spurious
    // "not found". Skip it rather than judge it.
    if (kinds.length === 0) return { absentIds, complete: false };

    let present: Set<string>;
    try {
      const events = await probe({ ids: chunk, kinds });
      if (!Array.isArray(events)) return { absentIds, complete: false };
      present = new Set(
        events.flatMap((event) =>
          typeof event?.id === "string" ? [event.id] : [],
        ),
      );
    } catch {
      // Ambiguous: the relay never answered. Judge nothing further.
      return { absentIds, complete: false };
    }

    for (const id of chunk) {
      if (!present.has(id)) absentIds.push(id);
    }
  }

  return { absentIds, complete: true };
}

// ─── Coalesced persistence writer (first-writer-wins + flush) ─────────────────
//
// The full item list is one ~600 KB JSON.stringify+setItem. Writing it on every
// incoming reply drives the renderer stall this coalescing exists to fix, so the
// scheduler collapses a burst of writes into one setItem ~1 s later. The live
// buffer (itemsRef) stays the source of truth between flushes; the timer reads
// it at fire time, so the persisted blob is always the burst's final state.

const WRITE_DEBOUNCE_MS = 1_000;

export type ThreadActivityRefs = {
  itemsRef: { current: ThreadActivityItem[] };
  scopeLoadedRef: { current: string };
  timerRef: { current: ReturnType<typeof setTimeout> | null };
};

/**
 * Inverse of activityScopeKey: split "pubkey:normalizedRelayUrl" back into its
 * parts. The pubkey is colon-free, so the first colon is the boundary. Returns
 * null for an empty or malformed scope.
 */
function decomposeScope(
  scope: string,
): { pubkey: string; relayUrl: string } | null {
  if (!scope) return null;
  const colonIdx = scope.indexOf(":");
  if (colonIdx === -1) return null;
  const pubkey = scope.slice(0, colonIdx);
  const relayUrl = scope.slice(colonIdx + 1);
  return pubkey && relayUrl ? { pubkey, relayUrl } : null;
}

/**
 * Arm a trailing-edge write for `scope`, first-writer-wins: a pending timer is
 * NOT reset, so a burst of N writes still fires once. The timer reads itemsRef
 * at fire time and re-checks the loaded scope, so a write that outlives a scope
 * switch can neither land under the new key nor persist the wrong buffer.
 */
export function scheduleThreadActivityWrite(
  scope: string,
  refs: ThreadActivityRefs,
): void {
  if (!scope || refs.scopeLoadedRef.current !== scope) return;
  if (refs.timerRef.current !== null) return;

  const parts = decomposeScope(scope);
  if (!parts) return;
  const { pubkey, relayUrl } = parts;

  refs.timerRef.current = setTimeout(() => {
    refs.timerRef.current = null;
    if (refs.scopeLoadedRef.current !== scope) return;
    writeActivityToStorage(pubkey, relayUrl, refs.itemsRef.current);
  }, WRITE_DEBOUNCE_MS);
}

/**
 * Synchronously persist a pending write and cancel the timer. A no-op when no
 * write is pending (the buffer is already durable), so flushing on hidden or
 * pagehide costs a setItem only when there is unsaved state. Callers MUST flush
 * before reseeding on a scope switch so the old scope's buffer lands under the
 * old key.
 */
export function flushThreadActivityWrite(refs: ThreadActivityRefs): void {
  if (refs.timerRef.current === null) return;
  clearTimeout(refs.timerRef.current);
  refs.timerRef.current = null;
  const parts = decomposeScope(refs.scopeLoadedRef.current);
  if (!parts) return;
  writeActivityToStorage(parts.pubkey, parts.relayUrl, refs.itemsRef.current);
}
