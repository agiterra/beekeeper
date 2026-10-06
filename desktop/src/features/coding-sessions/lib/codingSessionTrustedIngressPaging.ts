/**
 * Page a coding session's history to completion, newest first (SV-116).
 *
 * History used to be one REQ per kind, capped at the relay's page ceiling and
 * never continued, so any session past 1000 events of a kind opened with its
 * older part missing and nothing said so (ledger 347: tank-loop showed 1000 of
 * 4375 transcript events). The newest page still arrives first and still
 * renders at once; this module walks the older pages behind it and keeps an
 * honest account of whether the history is whole.
 *
 * Paging uses only what NIP-01 offers: `until` (inclusive, whole seconds) and
 * `limit`. The relay orders `created_at DESC, id ASC` and clamps `limit` to
 * NIP-11 `max_limit` (`beekeeper_db::DEFAULT_MAX_PAGE_LIMIT`, 1000), and it does not
 * post-filter these kinds after the SQL limit, so a page shorter than the
 * request is the end of the result. Because `until` is inclusive, each next
 * page re-reads the oldest second of the previous one; the ids already seen at
 * that second are carried so the page can tell new events from repeats. A
 * second holding more than a full page of events cannot be paged past with
 * `until` alone — that is disclosed as a gap, never skipped silently.
 *
 * Trust is untouched: every paged event goes through the same store
 * classifier as the newest page. This module decides only what to fetch.
 */
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";

/** The relay's page ceiling (NIP-11 `max_limit`); a larger ask is clamped. */
export const CODING_SESSION_HISTORY_PAGE_LIMIT = 1000;

/**
 * Older pages one kind may fetch over the life of a backfill: 200 pages is
 * 200,000 events of a kind, far past any measured session (5,838 events in
 * all). It exists so a pathological channel cannot page forever; reaching it
 * is disclosed as `page-budget`, not treated as the end.
 */
export const MAX_CODING_SESSION_HISTORY_BACKFILL_PAGES = 200;

/** Why a history stopped short of its beginning. */
export type CodingSessionHistoryGapReason =
  /** A page request failed; the next successful reload resumes it. */
  | "error"
  /** The page budget ran out before the oldest event. */
  | "page-budget"
  /** More than a page of events share one second, and `until` cannot split it. */
  | "crowded-second"
  /** This scope reads one page by design (multi-channel or one command). */
  | "not-paged";

/**
 * How much of a scope's history this client holds.
 *
 * - `pending` — the newest page has not answered yet; nothing is claimed.
 * - `loading-earlier` — the newest page is shown and older pages are still
 *   arriving.
 * - `complete` — every page answered, back to the oldest event.
 * - `incomplete` — paging stopped short; `reason` says why. The number of
 *   events still missing is not known (the relay is never asked to COUNT), so
 *   no number is invented for it.
 */
export type CodingSessionHistoryCompleteness = {
  state: "pending" | "loading-earlier" | "complete" | "incomplete";
  /** Events received from older pages so far, across every kind. */
  loadedEarlierCount: number;
  reason: CodingSessionHistoryGapReason | null;
  message: string | null;
};

export const PENDING_CODING_SESSION_HISTORY: CodingSessionHistoryCompleteness =
  Object.freeze({
    state: "pending",
    loadedEarlierCount: 0,
    reason: null,
    message: null,
  });

/**
 * One stretch of history still to read: everything at or before `until`, and
 * at or after `since` when the stretch closes a gap above known history.
 */
export type CodingSessionHistorySegment = {
  until: number;
  /** Ids already received whose `created_at` is exactly `until`. */
  seenAtUntil: ReadonlySet<string>;
  since: number | null;
};

export type CodingSessionHistoryPageStep = {
  /** Events in this page not already received for this segment. */
  fresh: RelayEvent[];
  /** The rest of the segment, or `null` when the segment is exhausted. */
  next: CodingSessionHistorySegment | null;
  /** A whole second was skipped because it held more than a page. */
  skippedCrowdedSecond: boolean;
};

function oldestCreatedAt(page: readonly RelayEvent[]): number {
  let oldest = Number.POSITIVE_INFINITY;
  for (const event of page) {
    if (event.created_at < oldest) oldest = event.created_at;
  }
  return oldest;
}

function newestCreatedAt(page: readonly RelayEvent[]): number {
  let newest = Number.NEGATIVE_INFINITY;
  for (const event of page) {
    if (event.created_at > newest) newest = event.created_at;
  }
  return newest;
}

/**
 * The segment that continues below a full page — the cursor for the page
 * after it. `previous` carries the ids already seen at the same second when
 * the page's oldest second is the one it was asked from.
 */
function segmentBelow(
  page: readonly RelayEvent[],
  since: number | null,
  previous: CodingSessionHistorySegment | null,
): CodingSessionHistorySegment {
  const until = oldestCreatedAt(page);
  const seenAtUntil = new Set<string>(
    previous && previous.until === until ? previous.seenAtUntil : [],
  );
  for (const event of page) {
    if (event.created_at === until) seenAtUntil.add(event.id);
  }
  return { until, seenAtUntil, since };
}

/**
 * Advance one segment by the page it just returned. Pure, so the stopping
 * rules are testable without a relay:
 *
 * - a page shorter than `limit` is the end of the segment;
 * - a full page continues from its oldest second, inclusive, remembering the
 *   ids it saw there;
 * - a full page with nothing new means every event in it shares `until` —
 *   more than a page in one second. The cursor steps one second down and the
 *   step reports the skipped second, so the caller can disclose it.
 */
export function advanceCodingSessionHistorySegment(
  segment: CodingSessionHistorySegment,
  page: readonly RelayEvent[],
  limit: number,
): CodingSessionHistoryPageStep {
  const fresh: RelayEvent[] = [];
  const inPage = new Set<string>();
  for (const event of page) {
    if (inPage.has(event.id)) continue;
    inPage.add(event.id);
    if (
      event.created_at === segment.until &&
      segment.seenAtUntil.has(event.id)
    ) {
      continue;
    }
    fresh.push(event);
  }
  if (page.length < limit) {
    return { fresh, next: null, skippedCrowdedSecond: false };
  }
  if (fresh.length === 0) {
    const until = segment.until - 1;
    const exhausted =
      until < 0 || (segment.since !== null && until < segment.since);
    return {
      fresh,
      next: exhausted
        ? null
        : { until, seenAtUntil: new Set(), since: segment.since },
      skippedCrowdedSecond: true,
    };
  }
  const next = segmentBelow(page, segment.since, segment);
  if (segment.since !== null && next.until < segment.since) {
    return { fresh, next: null, skippedCrowdedSecond: false };
  }
  return { fresh, next, skippedCrowdedSecond: false };
}

/** The filter for one segment's next page. */
export function buildCodingSessionHistoryPageFilter(
  base: RelaySubscriptionFilter,
  segment: CodingSessionHistorySegment,
  limit: number,
): RelaySubscriptionFilter {
  const filter: RelaySubscriptionFilter = {
    ...base,
    limit,
    until: segment.until,
  };
  if (segment.since !== null) filter.since = segment.since;
  else delete filter.since;
  return filter;
}

type KindPaging = {
  base: RelaySubscriptionFilter;
  /** Stretches still to read, newest first. */
  segments: CodingSessionHistorySegment[];
  /** Newest `created_at` any newest-page read of this kind has returned. */
  newestSeen: number | null;
  pagesFetched: number;
  error: string | null;
  budgetExhausted: boolean;
  crowdedSecond: boolean;
  /** A one-page scope saw a full page: older history exists, unread. */
  truncatedUnpaged: boolean;
};

export type CodingSessionHistoryBackfillRunOptions = {
  fetchPage: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>;
  /** Ingest one page's new events into the store (classifier and all). */
  ingest: (events: readonly RelayEvent[]) => void;
};

/**
 * The paging state of one retained ingress store, shared by every hook that
 * mounts that store, so two catalogs on the same scope page once between them
 * and a remount resumes from where paging stopped rather than starting over.
 */
export class CodingSessionHistoryBackfill {
  private readonly kinds = new Map<number, KindPaging>();
  private readonly listeners = new Set<() => void>();
  private attached = 0;
  private running = false;
  private loadedEarlierCount = 0;
  private sawNewestPage = false;
  private cached: CodingSessionHistoryCompleteness =
    PENDING_CODING_SESSION_HISTORY;

  readonly paged: boolean;
  private readonly limit: number;
  private readonly maxPages: number;

  constructor(
    paged: boolean,
    limit: number = CODING_SESSION_HISTORY_PAGE_LIMIT,
    maxPages: number = MAX_CODING_SESSION_HISTORY_BACKFILL_PAGES,
  ) {
    this.paged = paged;
    this.limit = limit;
    this.maxPages = maxPages;
  }

  /**
   * Record what one kind's newest page returned. A full page means older
   * history exists: the first time, everything below it is queued; on a
   * reload, a full page that no longer reaches the newest event already known
   * means more than a page arrived while this client was away, and the stretch
   * between is queued too (bounded below by `since`), so a reconnect cannot
   * open a hole in the middle of a transcript.
   */
  noteNewestPage(
    kind: number,
    base: RelaySubscriptionFilter,
    page: readonly RelayEvent[],
  ): void {
    let paging = this.kinds.get(kind);
    if (!paging) {
      paging = {
        base: { ...base },
        segments: [],
        newestSeen: null,
        pagesFetched: 0,
        error: null,
        budgetExhausted: false,
        crowdedSecond: false,
        truncatedUnpaged: false,
      };
      this.kinds.set(kind, paging);
    }
    const full = page.length >= this.limit;
    if (full) {
      if (!this.paged) {
        paging.truncatedUnpaged = true;
      } else if (paging.newestSeen === null) {
        paging.segments.push(segmentBelow(page, null, null));
      } else if (oldestCreatedAt(page) > paging.newestSeen) {
        // Newest first: the gap is above every stretch already queued.
        paging.segments.unshift(segmentBelow(page, paging.newestSeen, null));
      }
    }
    const newest = page.length > 0 ? newestCreatedAt(page) : 0;
    paging.newestSeen =
      paging.newestSeen === null ? newest : Math.max(paging.newestSeen, newest);
    this.sawNewestPage = true;
    this.changed();
  }

  /** Whether any kind still has older pages to read. */
  hasPendingPages(): boolean {
    for (const paging of this.kinds.values()) {
      if (paging.segments.length > 0 && !paging.budgetExhausted) return true;
    }
    return false;
  }

  completeness(): CodingSessionHistoryCompleteness {
    return this.cached;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /**
   * A mounted reader. Paging continues while at least one is attached and
   * pauses (keeping its cursors) when the last one leaves.
   */
  attach(): () => void {
    this.attached += 1;
    let detached = false;
    return () => {
      if (detached) return;
      detached = true;
      this.attached -= 1;
    };
  }

  /**
   * Read every queued older page, kinds in parallel, pages within a kind in
   * order. A second call while one is running is a no-op: the running pass
   * already reads everything queued, including stretches queued during it.
   * An errored kind is retried by the next call (the hook calls this after
   * every successful newest-page load, which includes each reconnect).
   */
  async run(options: CodingSessionHistoryBackfillRunOptions): Promise<void> {
    if (!this.paged || this.running) return;
    this.running = true;
    for (const paging of this.kinds.values()) paging.error = null;
    this.changed();
    try {
      await Promise.all(
        [...this.kinds.values()].map((paging) => this.runKind(paging, options)),
      );
    } finally {
      this.running = false;
      this.changed();
    }
  }

  private async runKind(
    paging: KindPaging,
    { fetchPage, ingest }: CodingSessionHistoryBackfillRunOptions,
  ): Promise<void> {
    while (this.attached > 0 && paging.segments.length > 0) {
      if (paging.pagesFetched >= this.maxPages) {
        paging.budgetExhausted = true;
        return;
      }
      const segment = paging.segments[0];
      let page: RelayEvent[];
      try {
        page = await fetchPage(
          buildCodingSessionHistoryPageFilter(paging.base, segment, this.limit),
        );
      } catch (error) {
        paging.error =
          error instanceof Error
            ? error.message
            : "Failed to load earlier coding-session events.";
        return;
      }
      paging.pagesFetched += 1;
      const step = advanceCodingSessionHistorySegment(
        segment,
        page,
        this.limit,
      );
      if (step.skippedCrowdedSecond) paging.crowdedSecond = true;
      // The segment may have been reordered by a reload while the page was in
      // flight; replace exactly the one this page answered.
      const index = paging.segments.indexOf(segment);
      if (index >= 0) {
        if (step.next) paging.segments[index] = step.next;
        else paging.segments.splice(index, 1);
      }
      if (step.fresh.length > 0) {
        this.loadedEarlierCount += step.fresh.length;
        ingest(step.fresh);
      }
      this.changed();
    }
  }

  private changed(): void {
    const next = this.computeCompleteness();
    const previous = this.cached;
    if (
      previous.state === next.state &&
      previous.loadedEarlierCount === next.loadedEarlierCount &&
      previous.reason === next.reason &&
      previous.message === next.message
    ) {
      return;
    }
    this.cached = next;
    for (const listener of this.listeners) listener();
  }

  private computeCompleteness(): CodingSessionHistoryCompleteness {
    if (!this.sawNewestPage) return PENDING_CODING_SESSION_HISTORY;
    const loadedEarlierCount = this.loadedEarlierCount;
    let reason: CodingSessionHistoryGapReason | null = null;
    let message: string | null = null;
    let stillPaging = false;
    for (const paging of this.kinds.values()) {
      const pending = paging.segments.length > 0;
      if (paging.error !== null) {
        reason ??= "error";
        message ??= paging.error;
      } else if (paging.budgetExhausted) {
        reason ??= "page-budget";
      } else if (pending) {
        stillPaging = true;
      }
      if (paging.crowdedSecond) reason ??= "crowded-second";
      if (paging.truncatedUnpaged) reason ??= "not-paged";
    }
    if (stillPaging && (this.running || reason === null)) {
      return {
        state: "loading-earlier",
        loadedEarlierCount,
        reason: null,
        message: null,
      };
    }
    if (reason !== null) {
      return { state: "incomplete", loadedEarlierCount, reason, message };
    }
    return {
      state: "complete",
      loadedEarlierCount,
      reason: null,
      message: null,
    };
  }
}

const backfills = new WeakMap<object, CodingSessionHistoryBackfill>();

/**
 * The backfill that belongs to `store`, created on first use. Keyed by the
 * store object, so it lives exactly as long as the retained store does and
 * needs no reset of its own: `resetCodingSessionIngressStores()` dropping the
 * store drops its paging state with it.
 */
export function acquireCodingSessionHistoryBackfill(
  store: object,
  paged: boolean,
): CodingSessionHistoryBackfill {
  const existing = backfills.get(store);
  if (existing && existing.paged === paged) return existing;
  const backfill = new CodingSessionHistoryBackfill(paged);
  backfills.set(store, backfill);
  return backfill;
}

/** The backfill for `store` if one exists, without creating one (render-safe). */
export function peekCodingSessionHistoryBackfill(
  store: object,
): CodingSessionHistoryBackfill | null {
  return backfills.get(store) ?? null;
}
