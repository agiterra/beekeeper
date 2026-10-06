/**
 * Client-side NIP-01 filter matching.
 *
 * Mirrors `crates/beekeeper-core/src/filter.rs` (`filter_match_one`): fields inside
 * one filter are AND-ed, values inside a field are OR-ed, `since`/`until` are
 * inclusive, `ids` are prefix matches, tag clauses (`#x`) match any tag whose
 * first element is `x` and whose value is listed.
 *
 * Used to demultiplex one `POST /query` response back to the callers whose
 * filters were bundled into it (`relayQueryCoalescer.ts`), so it must agree
 * with the relay. The one place it cannot: the relay resolves `#h` for an
 * event that carries **no** `h` tag at all (a reaction or deletion that
 * inherits its channel from its target) through the stored `channel_id`,
 * which a wire event does not expose. `channelFallback: "permissive"` lets
 * such an event through the `#h` clause the way the relay may have; `"strict"`
 * (the default, pure NIP-01) rejects it.
 */
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";

export type FilterMatchOptions = {
  channelFallback?: "strict" | "permissive";
};

const KNOWN_KEYS = new Set([
  "ids",
  "kinds",
  "limit",
  "authors",
  "since",
  "until",
]);

/**
 * Whether every key of `filter` is one this matcher understands. Filters with
 * `search` or the relay's JSON extension fields (`before_id`, `depth_limit`,
 * `feed_types`, …) cannot be demultiplexed client-side and must travel alone.
 */
export function isMatchableFilter(filter: RelaySubscriptionFilter): boolean {
  for (const key of Object.keys(filter)) {
    if (KNOWN_KEYS.has(key)) continue;
    // Any `#name` key: the relay indexes single-letter tags and the
    // coding-session anchors (`#cstx-genesis`, `#csat-genesis`) alike, and
    // the desktop already queries by both over the WebSocket.
    if (key.length >= 2 && key.startsWith("#")) continue;
    return false;
  }
  return true;
}

function tagValues(event: RelayEvent, letter: string): string[] | null {
  let values: string[] | null = null;
  for (const tag of event.tags) {
    if (tag[0] !== letter) continue;
    values ??= [];
    if (tag[1] !== undefined) values.push(tag[1]);
  }
  return values;
}

/** `true` when `event` satisfies every clause of `filter`. */
export function matchesFilter(
  event: RelayEvent,
  filter: RelaySubscriptionFilter,
  options: FilterMatchOptions = {},
): boolean {
  if (filter.kinds !== undefined && !filter.kinds.includes(event.kind)) {
    return false;
  }
  if (filter.authors !== undefined && !filter.authors.includes(event.pubkey)) {
    return false;
  }
  if (filter.since !== undefined && event.created_at < filter.since) {
    return false;
  }
  if (filter.until !== undefined && event.created_at > filter.until) {
    return false;
  }
  if (
    filter.ids !== undefined &&
    !filter.ids.some((prefix) => event.id.startsWith(prefix))
  ) {
    return false;
  }
  for (const [key, wanted] of Object.entries(filter)) {
    if (key.length < 2 || !key.startsWith("#") || !Array.isArray(wanted)) {
      continue;
    }
    const letter = key.slice(1);
    const present = tagValues(event, letter);
    if (present === null) {
      if (letter === "h" && options.channelFallback === "permissive") continue;
      return false;
    }
    const values = wanted as string[];
    if (!values.some((value) => present.includes(value))) return false;
  }
  return true;
}

/** `true` when `event` satisfies at least one of `filters` (NIP-01 OR). */
export function matchesAnyFilter(
  event: RelayEvent,
  filters: readonly RelaySubscriptionFilter[],
  options: FilterMatchOptions = {},
): boolean {
  return filters.some((filter) => matchesFilter(event, filter, options));
}
