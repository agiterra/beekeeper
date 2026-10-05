/**
 * The channel observer: relay reads in, one snapshot out.
 *
 * Deliberately free of React so the read rules can be tested against a mock
 * socket rather than a rendered tree. The engine owns three relay reads:
 *
 * - one history page per D2 filter (limit 1000), closed at EOSE;
 * - one long-lived live subscription (limit 0) that survives reconnect and
 *   replays `since = lastSeen - 5s`;
 * - a lease re-read every 60s, because a 24223 lease is ephemeral and its
 *   150s TTL is the only proof anybody is actually answering (D8);
 * - after history, a back-fill read for any generated title whose standing
 *   the pages could not prove (SV-31, `titleStanding.ts`).
 *
 * It never publishes anything. The browser observer has no write path.
 */
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
} from "../../../shared/lib/kinds.ts";
import {
  type NostrEvent,
  type SubscribeEventsOptions,
  subscribeEvents,
} from "../../../shared/lib/nostr-client.ts";
import {
  CODING_SESSION_HISTORY_LIMIT,
  type CodingSessionFilter,
  CodingSessionObserverStore,
  codingSessionClosuresFilter,
  codingSessionCreatesFilters,
  codingSessionFactsLiveFilter,
  codingSessionGeneratedTitlesFilter,
  codingSessionGoalsFilter,
  codingSessionHistoryFilters,
  codingSessionLeasesFilter,
  codingSessionNamesFilter,
  codingSessionTitleStandingFilters,
  codingSessionTitleStandingGaps,
  type CodingSessionObserverFacts,
  isTitleStandingPageTruncated,
  isTruncatedHistoryPage,
  MAX_TITLE_STANDING_GAPS_PER_READ,
  MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
  type ObservedEvent,
} from "../domain/index.ts";
import {
  type ChannelSessionObserverSnapshot,
  buildChannelSessionSnapshot,
  type CodingSessionObserverConnection,
  createEmptyChannelSessionSnapshot,
} from "./observerSnapshot.ts";

/** The transport the engine reads through; injected whole in tests. */
export type SubscribeEventsFn = (
  wsUrl: string,
  filters: readonly CodingSessionFilter[],
  onEvent: (event: NostrEvent, filterIndex: number) => void,
  options?: SubscribeEventsOptions,
) => () => void;

/** Cancels a repeating job. */
export type CancelSchedule = () => void;

export type ObserverEngineOptions = {
  wsUrl: string;
  channelId: string;
  /** Defaults to the shared relay client. */
  subscribe?: SubscribeEventsFn;
  /** Wall clock, injected so age lines are testable. */
  now?: () => number;
  /** How often leases are re-read while mounted. */
  leaseRefreshMs?: number;
  /** Coalescing window for snapshot rebuilds during a burst. */
  rebuildDelayMs?: number;
  maxRetainedRawEventsPerGeneration?: number;
  /** Repeating scheduler, injected in tests. */
  scheduleRepeating?: (run: () => void, everyMs: number) => CancelSchedule;
};

/** How often a mounted observer re-reads the ephemeral leases (D8/D10). */
export const LEASE_REFRESH_MS = 60_000;

/**
 * The live filter set.
 *
 * Every filter carries `limit: 0` — "nothing stored, only what is new" — so
 * the live subscription never re-pages history the initial read already has.
 * Lifecycle commands (44221) and genesis (44226) are here as well as in the
 * history read: a session created while a reader is watching would otherwise
 * have no readable create until the next refresh and would render as
 * "authority unverified" (D5) despite being perfectly governed. Lifecycle
 * receipts are not repeated here — the facts filter already carries 44224.
 */
export function codingSessionLiveFilters(
  channelId: string,
): CodingSessionFilter[] {
  const live = (filter: CodingSessionFilter): CodingSessionFilter => ({
    ...filter,
    limit: 0,
  });
  return [
    codingSessionFactsLiveFilter(channelId),
    ...codingSessionCreatesFilters(channelId)
      .filter(
        (filter) =>
          !filter.kinds.includes(KIND_CODING_SESSION_LIFECYCLE_RECEIPT),
      )
      .map(live),
    live(codingSessionNamesFilter(channelId)),
    live(codingSessionGeneratedTitlesFilter(channelId)),
    live(codingSessionGoalsFilter(channelId)),
    live(codingSessionClosuresFilter(channelId)),
    live(codingSessionLeasesFilter(channelId)),
  ];
}

function defaultScheduleRepeating(
  run: () => void,
  everyMs: number,
): CancelSchedule {
  const handle = setInterval(run, everyMs);
  return () => clearInterval(handle);
}

/** Reads one channel's coding sessions and publishes snapshots of them. */
export class CodingSessionObserverEngine {
  private readonly wsUrl: string;
  private readonly channelId: string;
  private readonly subscribe: SubscribeEventsFn;
  private readonly now: () => number;
  private readonly leaseRefreshMs: number;
  private readonly rebuildDelayMs: number;
  private readonly scheduleRepeating: (
    run: () => void,
    everyMs: number,
  ) => CancelSchedule;
  private readonly store: CodingSessionObserverStore;
  private readonly listeners = new Set<() => void>();

  private started = false;
  private snapshot: ChannelSessionObserverSnapshot;
  private connection: CodingSessionObserverConnection = "idle";
  private lastError: string | null = null;
  private truncatedAt1000 = false;
  private historyRead = false;
  private leasesRead = false;

  private stopHistory: (() => void) | null = null;
  private stopLive: (() => void) | null = null;
  private stopLeaseRead: (() => void) | null = null;
  private stopStandingRead: (() => void) | null = null;
  /** Titles a back-fill already asked about; cleared by a history re-read. */
  private readonly standingAsked = new Set<string>();
  private cancelLeaseSchedule: CancelSchedule | null = null;
  private rebuildTimer: ReturnType<typeof setTimeout> | null = null;
  private historyWaiters: {
    resolve: () => void;
    reject: (error: Error) => void;
  }[] = [];

  constructor(options: ObserverEngineOptions) {
    this.wsUrl = options.wsUrl;
    this.channelId = options.channelId;
    this.subscribe = options.subscribe ?? subscribeEvents;
    this.now = options.now ?? (() => Date.now());
    this.leaseRefreshMs = options.leaseRefreshMs ?? LEASE_REFRESH_MS;
    this.rebuildDelayMs = options.rebuildDelayMs ?? 0;
    this.scheduleRepeating =
      options.scheduleRepeating ?? defaultScheduleRepeating;
    this.store = new CodingSessionObserverStore(
      options.maxRetainedRawEventsPerGeneration ??
        MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
    );
    this.snapshot = createEmptyChannelSessionSnapshot();
  }

  /** Open the live subscription and read history. Idempotent. */
  start(): void {
    if (this.started) return;
    this.started = true;
    this.connection = "connecting";
    this.openLive();
    this.readHistory();
    this.cancelLeaseSchedule = this.scheduleRepeating(
      () => this.readLeases(),
      this.leaseRefreshMs,
    );
    this.rebuildNow();
  }

  /** Close every relay read. The snapshot stays readable. */
  stop(): void {
    this.started = false;
    this.stopHistory?.();
    this.stopHistory = null;
    this.stopLive?.();
    this.stopLive = null;
    this.stopLeaseRead?.();
    this.stopLeaseRead = null;
    this.stopStandingRead?.();
    this.stopStandingRead = null;
    this.cancelLeaseSchedule?.();
    this.cancelLeaseSchedule = null;
    if (this.rebuildTimer !== null) {
      clearTimeout(this.rebuildTimer);
      this.rebuildTimer = null;
    }
    this.connection = "idle";
    this.rebuildNow();
  }

  /** Re-read history and leases. Resolves when the history page completes. */
  refresh(): Promise<void> {
    if (!this.started) this.start();
    else this.readHistory();
    this.readLeases();
    return this.whenHistoryRead();
  }

  /**
   * Resolves when a history read completes, rejects when one fails.
   *
   * Waiters outlive an individual read on purpose: a remount stops and
   * restarts the engine, and a caller that was waiting should be answered by
   * whichever read finishes, not left holding a superseded promise.
   */
  whenHistoryRead(): Promise<void> {
    if (this.historyRead) return Promise.resolve();
    return new Promise<void>((resolve, reject) => {
      this.historyWaiters.push({ resolve, reject });
    });
  }

  /** The current snapshot. Reference-stable until something changes. */
  getSnapshot = (): ChannelSessionObserverSnapshot => this.snapshot;

  /** Subscribe to snapshot changes; returns an unsubscribe. */
  subscribeSnapshot = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Rebuild immediately rather than on the coalescing timer. */
  flush(): void {
    if (this.rebuildTimer !== null) {
      clearTimeout(this.rebuildTimer);
      this.rebuildTimer = null;
    }
    this.rebuildNow();
  }

  /** Raw retained events for one generation stream — proves D10 eviction. */
  retainedRawEvents(targetKey: string, signerPubkey: string): ObservedEvent[] {
    return this.store.retainedRawEvents(
      this.channelId,
      targetKey,
      signerPubkey,
    );
  }

  private openLive(): void {
    const filters = codingSessionLiveFilters(this.channelId);
    this.stopLive = this.subscribe(
      this.wsUrl,
      filters,
      (event) => this.ingest(event),
      {
        // A live filter's `limit: 0` would make a replay `since` return
        // nothing, so a reconnect borrows a real page size for the gap.
        replayLimit: CODING_SESSION_HISTORY_LIMIT,
        onStateChange: (state) => {
          if (state === "open") this.setConnection("open");
          else if (state === "connecting") this.setConnection("connecting");
          else if (state === "error") this.setConnection("error");
          // Without this the view would keep claiming "open" after the
          // transport stopped retrying — a lie about the relay.
          else if (state === "closed") this.setConnection("closed");
        },
        onClosed: (reason, index) => {
          this.lastError = reason;
          // A refused filter takes part of the live stream away. The transport
          // retries it, but until it comes back the subscription is degraded,
          // and reporting a frozen transcript as "live" is exactly the lie
          // this surface exists to avoid.
          if (index !== null && this.connection === "open") {
            this.connection = "error";
          }
          this.rebuildNow();
        },
      },
    );
  }

  private readHistory(): void {
    this.stopHistory?.();
    this.stopHistory = null;
    this.historyRead = false;
    // A re-read is a retry: every title may be asked about again.
    this.stopStandingRead?.();
    this.stopStandingRead = null;
    this.standingAsked.clear();

    const filters = codingSessionHistoryFilters(this.channelId);
    const counts = filters.map(() => 0);
    const seenEose = filters.map(() => false);
    const leaseFilterIndex = filters.findIndex((filter) =>
      filter.kinds.includes(KIND_CODING_SESSION_LEASE),
    );

    this.stopHistory = this.subscribe(
      this.wsUrl,
      filters,
      (event, index) => {
        counts[index] += 1;
        this.ingest(event);
      },
      {
        closeOnEose: true,
        onEose: (index) => {
          seenEose[index] = true;
          // A page that came back exactly at its limit is evidence of
          // truncation, not of completeness (D10).
          if (isTruncatedHistoryPage(filters[index], counts[index])) {
            this.truncatedAt1000 = true;
          }
          if (index === leaseFilterIndex) this.leasesRead = true;
          if (seenEose.every((value) => value)) {
            this.historyRead = true;
            this.stopHistory = null;
            this.settleHistoryWaiters(null);
            this.flush();
          }
        },
        onClosed: (reason, index) => {
          this.lastError = reason;
          if (index !== null) {
            // One refused subscription: the rest of the page still counts,
            // but this read can never complete, so it is settled honestly.
            seenEose[index] = true;
            if (seenEose.every((value) => value)) {
              this.historyRead = true;
              this.settleHistoryWaiters(null);
            }
            this.rebuildNow();
            return;
          }
          // Socket-level failure. Stop this read outright rather than let the
          // transport retry underneath a caller that is also retrying.
          this.stopHistory?.();
          this.stopHistory = null;
          this.settleHistoryWaiters(new Error(reason));
          this.setConnection("error");
        },
      },
    );
  }

  /**
   * Re-read the ephemeral leases.
   *
   * Leases are the only proof of reachability and they expire in 150s, so a
   * mounted observer re-reads them on a cadence well inside that window. The
   * rebuild also refreshes the "last reported N ago" age with the new clock.
   */
  private readLeases(): void {
    this.stopLeaseRead?.();
    const filters = [codingSessionLeasesFilter(this.channelId)];
    this.stopLeaseRead = this.subscribe(
      this.wsUrl,
      filters,
      (event) => this.ingest(event),
      {
        closeOnEose: true,
        onEose: () => {
          this.leasesRead = true;
          this.stopLeaseRead = null;
          this.flush();
        },
        onClosed: (reason) => {
          this.lastError = reason;
          this.rebuildNow();
        },
      },
    );
    this.rebuildNow();
  }

  private ingest(event: NostrEvent): void {
    this.store.ingest([event as ObservedEvent], [this.channelId]);
    this.scheduleRebuild();
  }

  private setConnection(next: CodingSessionObserverConnection): void {
    if (this.connection === next) return;
    this.connection = next;
    if (next !== "error" && next !== "closed") this.lastError = null;
    this.rebuildNow();
  }

  private scheduleRebuild(): void {
    if (this.rebuildTimer !== null) return;
    this.rebuildTimer = setTimeout(() => {
      this.rebuildTimer = null;
      this.rebuildNow();
    }, this.rebuildDelayMs);
  }

  /**
   * Ask the relay for the proof behind generated titles the history pages
   * could not stand up (SV-31): the create by id, and the signer's metadata
   * and receipts around the title's own time. One read at a time; a read's
   * end rebuilds, which asks about the next batch. A title is asked about
   * once per history read, so an unprovable one stays honestly foreign
   * instead of looping.
   */
  private readTitleStanding(facts: CodingSessionObserverFacts): void {
    if (!this.started || !this.historyRead || this.stopStandingRead !== null) {
      return;
    }
    const gaps = codingSessionTitleStandingGaps(facts)
      .filter((gap) => !this.standingAsked.has(gap.titleEventId))
      .slice(0, MAX_TITLE_STANDING_GAPS_PER_READ);
    if (gaps.length === 0) return;
    for (const gap of gaps) this.standingAsked.add(gap.titleEventId);
    const filters = codingSessionTitleStandingFilters(gaps);
    const counts = filters.map(() => 0);
    const settled = filters.map(() => false);
    const settle = (index: number) => {
      settled[index] = true;
      if (settled.every((value) => value)) {
        this.stopStandingRead = null;
        this.flush();
      }
    };
    let finished = false;
    const stop = this.subscribe(
      this.wsUrl,
      filters,
      (event, index) => {
        counts[index] += 1;
        this.ingest(event);
      },
      {
        closeOnEose: true,
        onEose: (index) => {
          if (isTitleStandingPageTruncated(filters[index], counts[index])) {
            this.truncatedAt1000 = true;
          }
          settle(index);
        },
        onClosed: (reason, index) => {
          this.lastError = reason;
          if (index !== null) {
            settle(index);
            return;
          }
          finished = true;
          this.stopStandingRead?.();
          this.stopStandingRead = null;
          this.scheduleRebuild();
        },
      },
    );
    // A transport that settled every filter synchronously already cleared
    // the slot; never re-arm it with a finished read.
    if (!finished && settled.some((value) => !value)) {
      this.stopStandingRead = stop;
    }
  }

  private rebuildNow(): void {
    const facts = this.store.facts([this.channelId]);
    this.readTitleStanding(facts);
    this.snapshot = buildChannelSessionSnapshot(facts, {
      nowMs: this.now(),
      leasesRead: this.leasesRead,
      historyRead: this.historyRead,
      truncatedAt1000: this.truncatedAt1000,
      connection: this.connection,
      lastError: this.lastError,
    });
    for (const listener of this.listeners) listener();
  }

  private settleHistoryWaiters(error: Error | null): void {
    const waiters = this.historyWaiters;
    this.historyWaiters = [];
    for (const waiter of waiters) {
      if (error === null) waiter.resolve();
      else waiter.reject(error);
    }
  }
}
