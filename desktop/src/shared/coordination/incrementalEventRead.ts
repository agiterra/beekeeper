/**
 * Incremental re-reads for the coordination polls (Agent Progress, Pulse).
 *
 * Those surfaces re-read up to ~1,000 durable session facts per channel chunk
 * every 60 s and on every live fan-out, and the webview re-processed every row
 * each time — the periodic CPU bursts measured in ledger 310. The durable
 * kinds (442xx, 44240) are regular, append-only events, so a re-read only has
 * to ask for what is new: this module keeps the rows a read returned and asks
 * the next read for `since` the newest of them, minus the relay's ingest
 * window, then merges, deduplicates by id and applies the same cap.
 *
 * What an append-only delta cannot see is a deletion (a kind:5 tombstone on a
 * deleted session carries no `#h`, so no channel-scoped query finds it). Two
 * fences bound that: every held read is replaced by a full read after
 * {@link INCREMENTAL_FULL_REREAD_MS}, and {@link forgetHeldEventReads} drops
 * everything when this client deletes a session itself. A snapshot read — the
 * kind 24223 lease keys — is never incremental: a released lease must vanish.
 */
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";

/**
 * The relay rejects an event stamped more than 900 s from its clock
 * (`MAX_TIMESTAMP_DRIFT_SECS` in `beekeeper-relay` ingest), so a row stored after a
 * read began is never stamped earlier than this before it. The extra minute
 * absorbs skew between this machine's clock and the relay's.
 */
export const INCREMENTAL_READ_OVERLAP_SECS = 900 + 60;
/** A held read older than this is replaced by a full read (deletions, skew). */
export const INCREMENTAL_FULL_REREAD_MS = 5 * 60_000;
/** Distinct held reads kept at once; the oldest is evicted past this. */
export const HELD_EVENT_READ_LIMIT = 64;

/** One source read's rows, kept to make the next read a delta. */
export type HeldEventRead = {
  /** Newest first, at most the read's `limit`. */
  events: RelayEvent[];
  /** This machine's clock, in seconds, when the read that produced it began. */
  readStartedAtSec: number;
  /** When the last *full* read of this source completed, in ms. */
  fullReadAtMs: number;
};

/** Where held reads live; a `Map` in production, injectable for tests. */
export type HeldEventReads = Map<string, HeldEventRead>;

const heldEventReads: HeldEventReads = new Map();

/** The app's shared store of held reads. Community-scoped: see the reset. */
export function sharedHeldEventReads(): HeldEventReads {
  return heldEventReads;
}

/**
 * Drop every held read, so each source's next read is a full one. Wired into
 * `resetCommunityState()` and called after this client deletes a session.
 */
export function forgetHeldEventReads(): void {
  heldEventReads.clear();
}

/** Newest first; ties broken by id so the cap is deterministic. */
function newestFirst(left: RelayEvent, right: RelayEvent): number {
  if (left.created_at !== right.created_at) {
    return right.created_at - left.created_at;
  }
  return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
}

/**
 * The `since` a delta read uses: the earlier of the newest held row and the
 * held read's own start, minus the overlap. Anchoring on both keeps a
 * future-stamped row or a fast local clock from pushing `since` past rows the
 * relay stored after the previous read.
 */
export function incrementalSince(held: HeldEventRead): number {
  const newest = held.events[0]?.created_at ?? held.readStartedAtSec;
  const anchor = Math.min(newest, held.readStartedAtSec);
  return Math.max(0, anchor - INCREMENTAL_READ_OVERLAP_SECS);
}

/**
 * Merge a delta into held rows: dedupe by id, newest first, capped at `limit`.
 *
 * `reachedLimit` is computed over the distinct union *before* the cap, which
 * is exactly when a full read would have come back with `limit` rows — so a
 * merged read reports truncation precisely when a full read would have.
 */
export function mergeEventReads(
  held: readonly RelayEvent[],
  fresh: readonly RelayEvent[],
  limit: number,
): { events: RelayEvent[]; reachedLimit: boolean } {
  const byId = new Map<string, RelayEvent>();
  for (const event of held) byId.set(event.id, event);
  for (const event of fresh) byId.set(event.id, event);
  const merged = [...byId.values()].sort(newestFirst);
  return {
    events: merged.slice(0, limit),
    reachedLimit: merged.length >= limit,
  };
}

/** One filter in a bundle. `heldKey: null` marks a snapshot: always full. */
export type IncrementalSourceRead = {
  filter: RelaySubscriptionFilter & { kinds: number[]; limit: number };
  heldKey: string | null;
};

/** One read's outcome: the rows to fold, and whether it hit its limit. */
export type IncrementalSourceResult = {
  events: RelayEvent[];
  reachedLimit: boolean;
};

function rowsFor(
  bundle: readonly RelayEvent[],
  filter: { kinds: number[] },
): RelayEvent[] {
  return bundle.filter((event) => filter.kinds.includes(event.kind));
}

function remember(
  store: HeldEventReads,
  key: string,
  read: HeldEventRead,
): void {
  store.delete(key);
  if (store.size >= HELD_EVENT_READ_LIMIT) {
    const oldest = store.keys().next();
    if (!oldest.done) store.delete(oldest.value);
  }
  store.set(key, read);
}

/**
 * Run one bundle of reads, as deltas where a fresh held read allows it.
 *
 * Rows are attributed back to each read by kind, so the reads in one bundle
 * must name disjoint kinds (both callers already rely on that). A delta that
 * itself came back with `limit` rows may have skipped rows between it and the
 * held set, so that source is re-read in full before anything is merged.
 * A failed fetch throws and leaves every held read untouched.
 */
export async function readBundleIncrementally(
  reads: readonly IncrementalSourceRead[],
  fetchEventsBatch: (
    filters: RelaySubscriptionFilter[],
  ) => Promise<RelayEvent[]>,
  options: { store?: HeldEventReads; nowMs?: number } = {},
): Promise<IncrementalSourceResult[]> {
  const { store } = options;
  const nowMs = options.nowMs ?? Date.now();
  const readStartedAtSec = Math.floor(nowMs / 1_000);
  const held = reads.map((read) => {
    if (!store || read.heldKey === null) return null;
    const entry = store.get(read.heldKey);
    if (!entry || nowMs - entry.fullReadAtMs >= INCREMENTAL_FULL_REREAD_MS) {
      return null;
    }
    return entry;
  });

  const bundle = await fetchEventsBatch(
    reads.map((read, index) => {
      const entry = held[index];
      return entry
        ? { ...read.filter, since: incrementalSince(entry) }
        : read.filter;
    }),
  );
  const rows = reads.map((read) => rowsFor(bundle, read.filter));

  const overflowed = reads
    .map((_, index) => index)
    .filter(
      (index) =>
        held[index] !== null && rows[index].length >= reads[index].filter.limit,
    );
  if (overflowed.length > 0) {
    const refetched = await fetchEventsBatch(
      overflowed.map((index) => reads[index].filter),
    );
    for (const index of overflowed) {
      held[index] = null;
      rows[index] = rowsFor(refetched, reads[index].filter);
    }
  }

  return reads.map((read, index) => {
    const entry = held[index];
    const limit = read.filter.limit;
    const result = entry
      ? mergeEventReads(entry.events, rows[index], limit)
      : {
          events: rows[index],
          reachedLimit: rows[index].length >= limit,
        };
    if (store && read.heldKey !== null) {
      remember(store, read.heldKey, {
        events: entry
          ? result.events
          : mergeEventReads([], rows[index], limit).events,
        readStartedAtSec,
        fullReadAtMs: entry ? entry.fullReadAtMs : nowMs,
      });
    }
    return result;
  });
}

/** A stable held-read key for one filter on one surface. */
export function heldReadKey(
  surface: string,
  filter: RelaySubscriptionFilter,
): string {
  return `${surface}:${JSON.stringify(filter)}`;
}
