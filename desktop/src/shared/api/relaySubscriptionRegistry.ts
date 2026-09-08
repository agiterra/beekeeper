/**
 * Refcounted dedupe registry for live relay subscriptions.
 *
 * Two mounts asking for the *same* filter list (the doubled coding-session
 * catalogs, a presence sub opened by two panels) used to cost two REQs and two
 * relay-side subscriptions. The registry keys subscriptions by the canonical
 * JSON of their filter list — including `since` and `limit`, because consumers
 * classify backlog vs live by the `since` they asked for — opens the REQ once,
 * fans events out to every joiner, and sends CLOSE when the last one leaves.
 *
 * A late joiner is handed what a fresh REQ would have returned: the newest
 * `limit` events already delivered on the entry (bounded by
 * {@link REPLAY_RING_MAX}), then live events from the join onward.
 */
import type {
  LiveSubscriptionReadiness,
  RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";

/** Upper bound on the events kept for late joiners, whatever the filter's limit. */
export const REPLAY_RING_MAX = 1_000;

type Listener = (event: RelayEvent) => void;
type ReadyListener = (readiness: LiveSubscriptionReadiness) => void;
type Unsubscribe = () => Promise<void>;

/**
 * Opens the underlying subscription for the first joiner. `onEvent` is the
 * registry's fan-out; `onReady` records readiness for every joiner.
 */
export type OpenSubscription = (
  filters: RelaySubscriptionFilter[],
  onEvent: Listener,
  onReady: ReadyListener,
) => Promise<Unsubscribe>;

/** One join. A token per join lets the same handler function join twice. */
type Member = { listener: Listener };

type Entry = {
  filters: RelaySubscriptionFilter[];
  members: Set<Member>;
  opening: Promise<Unsubscribe>;
  readiness: LiveSubscriptionReadiness | null;
  readyWaiters: ReadyListener[];
  ring: RelayEvent[];
  ringMax: number;
};

function canonicalFilter(filter: RelaySubscriptionFilter): string {
  const keys = Object.keys(filter).sort();
  const parts = keys.map((key) => {
    const value = (filter as Record<string, unknown>)[key];
    const canonical = Array.isArray(value)
      ? [...value].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))
      : value;
    return `${JSON.stringify(key)}:${JSON.stringify(canonical)}`;
  });
  return `{${parts.join(",")}}`;
}

/**
 * Canonical identity of a filter list: key order and value order inside a
 * clause do not matter, every field (including `since`/`limit`) does.
 */
export function subscriptionKey(filters: RelaySubscriptionFilter[]): string {
  return `[${filters.map(canonicalFilter).sort().join(",")}]`;
}

export class RelaySubscriptionRegistry {
  private entries = new Map<string, Entry>();

  /** Number of distinct underlying subscriptions currently open. */
  size(): number {
    return this.entries.size;
  }

  /** Joiners currently attached to the subscription for `filters` (0 = none). */
  refCount(filters: RelaySubscriptionFilter[]): number {
    return this.entries.get(subscriptionKey(filters))?.members.size ?? 0;
  }

  /**
   * Attach `listener` to the subscription for `filters`, opening it through
   * `open` when nobody holds it yet. Resolves once readiness is known (as the
   * single-subscriber path always did) with the leave function.
   */
  async join(
    filters: RelaySubscriptionFilter[],
    listener: Listener,
    onReady: ReadyListener | undefined,
    open: OpenSubscription,
  ): Promise<Unsubscribe> {
    const key = subscriptionKey(filters);
    const member: Member = { listener };
    const joined = this.entries.get(key);
    if (joined) {
      joined.members.add(member);
      await this.attach(joined, member, onReady);
      return () => this.leave(key, joined, member);
    }

    const ringMax = Math.min(
      REPLAY_RING_MAX,
      Math.max(0, ...filters.map((filter) => filter.limit)),
    );
    const created: Entry = {
      filters,
      members: new Set([member]),
      opening: Promise.resolve(async () => {}),
      readiness: null,
      readyWaiters: onReady ? [onReady] : [],
      ring: [],
      ringMax,
    };
    this.entries.set(key, created);
    created.opening = open(
      filters,
      (event) => this.fanOut(created, event),
      (readiness) => {
        if (created.readiness !== null) return;
        created.readiness = readiness;
        const waiters = created.readyWaiters;
        created.readyWaiters = [];
        for (const waiter of waiters) waiter(readiness);
      },
    );
    try {
      await created.opening;
    } catch (error) {
      if (this.entries.get(key) === created) this.entries.delete(key);
      throw error;
    }
    return () => this.leave(key, created, member);
  }

  /** Drop every entry without sending CLOSE (the socket is gone anyway). */
  clear(): void {
    this.entries.clear();
  }

  private async attach(
    entry: Entry,
    member: Member,
    onReady: ReadyListener | undefined,
  ) {
    // A fresh REQ would return the newest `limit` stored events first.
    for (const event of entry.ring) member.listener(event);
    if (entry.readiness !== null) {
      onReady?.(entry.readiness);
    } else if (onReady) {
      entry.readyWaiters.push(onReady);
    }
    try {
      await entry.opening;
    } catch (error) {
      entry.members.delete(member);
      throw error;
    }
  }

  private fanOut(entry: Entry, event: RelayEvent) {
    if (entry.ringMax > 0) {
      entry.ring.push(event);
      if (entry.ring.length > entry.ringMax) {
        entry.ring.splice(0, entry.ring.length - entry.ringMax);
      }
    }
    for (const member of entry.members) {
      try {
        member.listener(event);
      } catch (error) {
        console.error("Relay subscription listener failed", error);
      }
    }
  }

  private async leave(key: string, entry: Entry, member: Member) {
    if (!entry.members.delete(member)) return;
    if (entry.members.size > 0) return;
    if (this.entries.get(key) === entry) this.entries.delete(key);
    let unsubscribe: Unsubscribe;
    try {
      unsubscribe = await entry.opening;
    } catch {
      return;
    }
    await unsubscribe();
  }
}
