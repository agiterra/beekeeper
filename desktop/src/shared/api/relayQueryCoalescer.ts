/**
 * One-shot relay reads, bundled.
 *
 * Every one-shot read used to be its own WebSocket REQ — one admission unit
 * each against the per-key burst the paired phone shares. `POST /query`
 * accepts an array of filters for one call against a separate per-minute
 * budget, so reads that happen to land in the same 50 ms tick are merged into
 * one HTTP body and the response is demultiplexed back to each caller with
 * the NIP-01 matcher (`relayFilterMatch.ts`).
 *
 * Contract per input filter:
 * - `limit: 0` resolves `[]` without a request (NIP-01: no stored results).
 * - Results are deduplicated by id, truncated to the newest `limit`, and
 *   returned oldest-first — the same shape the WS history path returns.
 * - A filter the matcher cannot demultiplex (`search`, extension fields) is
 *   never bundled; it takes the WS fallback on its own.
 * - The relay caps aggregate `#h` values per request at
 *   {@link MAX_CHANNEL_VALUES_PER_REQUEST}; bundles are chunked to fit, and a
 *   single filter wider than that is split into pieces whose union is then
 *   truncated back to the filter's own `limit`.
 * - When an HTTP call fails (429 has already armed the shared gate through
 *   `invokeTauri`), every filter in that chunk falls back to the WS history
 *   path individually, through the read lane of the send budget.
 */
import {
  MAX_CHANNEL_VALUES_PER_REQUEST,
  filterChannelValueCount,
  sortEvents,
  type RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import {
  isMatchableFilter,
  matchesFilter,
} from "@/shared/api/relayFilterMatch";
import type { RelayEvent } from "@/shared/api/types";

/** Coalescing window: reads issued within it share one `POST /query`. */
export const QUERY_COALESCE_WINDOW_MS = 50;

export type QueryExecutor = (
  filters: RelaySubscriptionFilter[],
) => Promise<RelayEvent[]>;

export type FilterFallback = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

export type QuerySettled =
  | { ok: true; events: RelayEvent[] }
  | { ok: false; error: Error };

/**
 * Split one filter whose `#h` list exceeds the per-request cap into pieces
 * that each fit. Anything else is returned as-is.
 */
export function splitOversizedChannelFilter(
  filter: RelaySubscriptionFilter,
  maxChannelValues = MAX_CHANNEL_VALUES_PER_REQUEST,
): RelaySubscriptionFilter[] {
  const channels = filter["#h"];
  if (!channels || channels.length <= maxChannelValues) return [filter];
  const pieces: RelaySubscriptionFilter[] = [];
  for (let i = 0; i < channels.length; i += maxChannelValues) {
    pieces.push({ ...filter, "#h": channels.slice(i, i + maxChannelValues) });
  }
  return pieces;
}

/**
 * Greedily group filters into request-sized chunks: at most `maxFilters` per
 * chunk and at most `maxChannelValues` aggregate `#h` values per chunk. Input
 * order is preserved. Filters wider than the channel cap must be split first
 * ({@link splitOversizedChannelFilter}); one is placed alone here.
 */
export function chunkFiltersForRequest<
  T extends { filter: RelaySubscriptionFilter },
>(
  items: T[],
  {
    maxFilters = Number.POSITIVE_INFINITY,
    maxChannelValues = MAX_CHANNEL_VALUES_PER_REQUEST,
  }: { maxFilters?: number; maxChannelValues?: number } = {},
): T[][] {
  const chunks: T[][] = [];
  let current: T[] = [];
  let currentChannels = 0;
  for (const item of items) {
    const channels = filterChannelValueCount(item.filter);
    const overflows =
      current.length >= maxFilters ||
      (current.length > 0 && currentChannels + channels > maxChannelValues);
    if (overflows) {
      chunks.push(current);
      current = [];
      currentChannels = 0;
    }
    current.push(item);
    currentChannels += channels;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

/**
 * Reduce a union response to what one filter would have returned on its own:
 * matching events only, deduplicated, newest `limit`, oldest-first.
 */
export function demuxForFilter(
  events: readonly RelayEvent[],
  filter: RelaySubscriptionFilter,
  channelFallback: "permissive" | "strict" = "permissive",
): RelayEvent[] {
  const seen = new Set<string>();
  const matched: RelayEvent[] = [];
  for (const event of events) {
    if (seen.has(event.id)) continue;
    if (!matchesFilter(event, filter, { channelFallback })) {
      continue;
    }
    seen.add(event.id);
    matched.push(event);
  }
  const ordered = sortEvents(matched);
  if (filter.limit > 0 && ordered.length > filter.limit) {
    return ordered.slice(ordered.length - filter.limit);
  }
  return ordered;
}

function toError(error: unknown, fallback: string): Error {
  return error instanceof Error ? error : new Error(fallback);
}

/**
 * Run a set of one-shot filters as few HTTP calls as the relay allows, and
 * settle each filter independently. See the module doc for the contract.
 */
export async function executeQueryBatch(
  filters: readonly RelaySubscriptionFilter[],
  {
    query,
    fallback,
    maxChannelValues = MAX_CHANNEL_VALUES_PER_REQUEST,
  }: {
    query: QueryExecutor;
    fallback: FilterFallback;
    maxChannelValues?: number;
  },
): Promise<QuerySettled[]> {
  const results: QuerySettled[] = new Array(filters.length);
  const pieces: Array<{ index: number; filter: RelaySubscriptionFilter }> = [];
  const direct: Promise<void>[] = [];

  filters.forEach((filter, index) => {
    if (filter.limit === 0) {
      results[index] = { ok: true, events: [] };
      return;
    }
    if (!isMatchableFilter(filter)) {
      direct.push(
        fallback(filter).then(
          (events) => {
            results[index] = { ok: true, events };
          },
          (error) => {
            results[index] = {
              ok: false,
              error: toError(error, "Relay history request failed."),
            };
          },
        ),
      );
      return;
    }
    for (const piece of splitOversizedChannelFilter(filter, maxChannelValues)) {
      pieces.push({ index, filter: piece });
    }
  });

  const collected = new Map<number, RelayEvent[]>();
  const failures = new Map<number, Error>();
  // A chunk whose filters name different channel sets cannot let an h-less
  // event (a reaction, a deletion — the relay resolves those to one channel
  // from a column the wire does not carry) satisfy every `#h` clause: that
  // would hand one caller another caller's events. Such chunks demux
  // strictly, and an h-less event is dropped rather than leaked.
  const strictIndexes = new Set<number>();
  const chunks = chunkFiltersForRequest(pieces, { maxChannelValues });
  for (const chunk of chunks) {
    const scopes = new Set(chunk.map((piece) => channelScope(piece.filter)));
    if (scopes.size > 1) {
      for (const { index } of chunk) strictIndexes.add(index);
    }
  }
  await Promise.all([
    ...direct,
    ...chunks.map(async (chunk) => {
      try {
        const events = await query(chunk.map((piece) => piece.filter));
        for (const { index } of chunk) {
          const bucket = collected.get(index) ?? [];
          bucket.push(...events);
          collected.set(index, bucket);
        }
      } catch (error) {
        console.warn(
          "[relay query] POST /query failed; falling back to WS history",
          error,
        );
        await Promise.all(
          chunk.map(async ({ index, filter }) => {
            try {
              const events = await fallback(filter);
              const bucket = collected.get(index) ?? [];
              bucket.push(...events);
              collected.set(index, bucket);
            } catch (fallbackError) {
              failures.set(
                index,
                toError(fallbackError, "Relay history request failed."),
              );
            }
          }),
        );
      }
    }),
  ]);

  filters.forEach((filter, index) => {
    if (results[index] !== undefined) return;
    const error = failures.get(index);
    if (error) {
      results[index] = { ok: false, error };
      return;
    }
    results[index] = {
      ok: true,
      events: demuxForFilter(
        collected.get(index) ?? [],
        filter,
        strictIndexes.has(index) ? "strict" : "permissive",
      ),
    };
  });
  return results;
}

/** The `#h` set of a filter as a stable key; "" when it names no channel. */
function channelScope(filter: RelaySubscriptionFilter): string {
  const channels = (filter as { "#h"?: string[] })["#h"];
  return channels ? [...channels].sort().join("\u0000") : "";
}

/** Union of every settled result; throws the first failure when one exists. */
export function unionQueryResults(results: QuerySettled[]): RelayEvent[] {
  const seen = new Set<string>();
  const union: RelayEvent[] = [];
  for (const result of results) {
    if (!result.ok) throw result.error;
    for (const event of result.events) {
      if (seen.has(event.id)) continue;
      seen.add(event.id);
      union.push(event);
    }
  }
  return sortEvents(union);
}

type Pending = {
  filter: RelaySubscriptionFilter;
  resolve: (events: RelayEvent[]) => void;
  reject: (error: Error) => void;
};

/**
 * Collects one-shot reads for {@link QUERY_COALESCE_WINDOW_MS} and hands
 * them to `execute` as one batch. Owned by a relay session; `reset()` on
 * disconnect so a pending read cannot resolve with another community's data.
 */
export class RelayQueryCoalescer {
  private pending: Pending[] = [];
  private timer: number | null = null;
  private readonly execute: (
    filters: RelaySubscriptionFilter[],
  ) => Promise<QuerySettled[]>;
  private readonly windowMs: number;
  private readonly setTimeoutFn: (fn: () => void, ms: number) => number;
  private readonly clearTimeoutFn: (id: number) => void;

  constructor({
    execute,
    windowMs = QUERY_COALESCE_WINDOW_MS,
    setTimeoutFn = (fn, ms) => window.setTimeout(fn, ms) as unknown as number,
    clearTimeoutFn = (id) => window.clearTimeout(id),
  }: {
    execute: (filters: RelaySubscriptionFilter[]) => Promise<QuerySettled[]>;
    windowMs?: number;
    setTimeoutFn?: (fn: () => void, ms: number) => number;
    clearTimeoutFn?: (id: number) => void;
  }) {
    this.execute = execute;
    this.windowMs = windowMs;
    this.setTimeoutFn = setTimeoutFn;
    this.clearTimeoutFn = clearTimeoutFn;
  }

  /** Queue one read; resolves with that filter's own results. */
  enqueue(filter: RelaySubscriptionFilter): Promise<RelayEvent[]> {
    return new Promise<RelayEvent[]>((resolve, reject) => {
      this.pending.push({ filter, resolve, reject });
      this.timer ??= this.setTimeoutFn(() => this.flush(), this.windowMs);
    });
  }

  /** Number of reads waiting for the window to close. */
  pendingCount(): number {
    return this.pending.length;
  }

  /** Close the window now and run the batch. */
  flush(): void {
    if (this.timer !== null) {
      this.clearTimeoutFn(this.timer);
      this.timer = null;
    }
    const batch = this.pending;
    this.pending = [];
    if (batch.length === 0) return;
    void this.execute(batch.map((entry) => entry.filter)).then(
      (results) => {
        batch.forEach((entry, index) => {
          const result = results[index];
          if (result === undefined) {
            entry.reject(new Error("Relay query batch returned no result."));
          } else if (result.ok) {
            entry.resolve(result.events);
          } else {
            entry.reject(result.error);
          }
        });
      },
      (error) => {
        const failure = toError(error, "Relay query batch failed.");
        for (const entry of batch) entry.reject(failure);
      },
    );
  }

  /** Reject every queued read (community switch) and drop the timer. */
  reset(error: Error): void {
    if (this.timer !== null) {
      this.clearTimeoutFn(this.timer);
      this.timer = null;
    }
    const batch = this.pending;
    this.pending = [];
    for (const entry of batch) entry.reject(error);
  }
}
