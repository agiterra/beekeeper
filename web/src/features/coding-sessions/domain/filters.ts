/**
 * The exact relay filters the observer subscribes with (D2).
 *
 * Every filter carries explicit `kinds` and `#h` — a filter without `kinds`
 * trips the relay's p-gate and comes back 403 — and none narrows by `authors`:
 * open authority here is channel membership, and the read-side trust gate
 * (`trust.ts`) is what decides whose facts count.
 *
 * History filters use `limit: 1000`; the live filter uses `limit: 0` so the
 * relay streams new events without replaying a page first.
 */
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_SYSTEM_MESSAGE,
} from "../../../shared/lib/kinds.ts";

/** The page size every history filter asks for. */
export const CODING_SESSION_HISTORY_LIMIT = 1000;
/** The smaller page the roster reads need. */
export const CODING_SESSION_ROSTER_LIMIT = 500;

/** A NIP-01 filter, narrowed to what these subscriptions actually use. */
export type CodingSessionFilter = {
  kinds: number[];
  "#h": string[];
  limit: number;
  since?: number;
};

function scoped(
  kinds: number[],
  channelId: string,
  limit: number,
): CodingSessionFilter {
  return { kinds, "#h": [channelId], limit };
}

/** The three provider fact streams, as one history page. */
export function codingSessionFactsFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [
      KIND_CODING_SESSION_METADATA,
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      KIND_CODING_SESSION_TRANSCRIPT,
    ],
    channelId,
    CODING_SESSION_HISTORY_LIMIT,
  );
}

/** The same three streams, live. `limit: 0` means "no replay, just new". */
export function codingSessionFactsLiveFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [
      KIND_CODING_SESSION_METADATA,
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      KIND_CODING_SESSION_TRANSCRIPT,
    ],
    channelId,
    0,
  );
}

/**
 * One filter PER KIND for the create-side reads.
 *
 * Deliberately not one filter with three kinds: a relay applies `limit` per
 * filter, so a single combined page can be filled entirely by whichever kind
 * is chattiest and silently drop the other two.
 */
export function codingSessionCreatesFilters(
  channelId: string,
): CodingSessionFilter[] {
  return [
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_GENESIS,
  ].map((kind) => scoped([kind], channelId, CODING_SESSION_HISTORY_LIMIT));
}

export function codingSessionNamesFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [KIND_CODING_SESSION_NAME],
    channelId,
    CODING_SESSION_HISTORY_LIMIT,
  );
}

export function codingSessionGoalsFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [KIND_CODING_SESSION_GOAL],
    channelId,
    CODING_SESSION_HISTORY_LIMIT,
  );
}

export function codingSessionClosuresFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [KIND_CODING_SESSION_CLOSURE],
    channelId,
    CODING_SESSION_HISTORY_LIMIT,
  );
}

/** Leases are ephemeral snapshots — read whole, never paginated. */
export function codingSessionLeasesFilter(
  channelId: string,
): CodingSessionFilter {
  return scoped(
    [KIND_CODING_SESSION_LEASE],
    channelId,
    CODING_SESSION_HISTORY_LIMIT,
  );
}

/** The two roster reads, at the smaller limit. */
export function codingSessionRosterFilters(
  channelId: string,
): CodingSessionFilter[] {
  return [
    scoped(
      [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      channelId,
      CODING_SESSION_ROSTER_LIMIT,
    ),
    scoped([KIND_SYSTEM_MESSAGE], channelId, CODING_SESSION_ROSTER_LIMIT),
  ];
}

/** Every history filter one channel needs, in wire order. */
export function codingSessionHistoryFilters(
  channelId: string,
): CodingSessionFilter[] {
  return [
    codingSessionFactsFilter(channelId),
    ...codingSessionCreatesFilters(channelId),
    codingSessionNamesFilter(channelId),
    codingSessionGoalsFilter(channelId),
    codingSessionClosuresFilter(channelId),
    codingSessionLeasesFilter(channelId),
    ...codingSessionRosterFilters(channelId),
  ];
}

/**
 * A page that came back exactly at its limit is evidence of truncation, not of
 * completeness. Surfaces disclose it rather than implying they showed
 * everything.
 */
export function isTruncatedHistoryPage(
  filter: CodingSessionFilter,
  eventCount: number,
): boolean {
  return filter.limit > 0 && eventCount >= filter.limit;
}
