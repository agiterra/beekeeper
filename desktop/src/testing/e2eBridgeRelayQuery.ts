import type { RelayEvent } from "@/shared/api/types";
import type { MockFilter } from "./e2eBridgeSessionFacts";

/** Reuse the mock relay's ordinary REQ responder for a batched HTTP read. */
export async function queryMockRelayFilters(
  filters: readonly MockFilter[],
  respond: (
    filter: MockFilter,
    subId: string,
    send: (frame: unknown[]) => void,
  ) => unknown,
): Promise<RelayEvent[]> {
  const pages = await Promise.all(
    filters.map(
      (filter, index) =>
        new Promise<RelayEvent[]>((resolve, reject) => {
          const subId = `history-http-${index}`;
          const events: RelayEvent[] = [];
          const send = (frame: unknown[]) => {
            if (frame[1] !== subId) return;
            if (frame[0] === "EVENT") events.push(frame[2] as RelayEvent);
            else if (frame[0] === "EOSE") resolve(events);
            else if (frame[0] === "CLOSED")
              reject(new Error(String(frame[2] ?? "Mock relay query refused")));
          };
          try {
            Promise.resolve(respond(filter, subId, send)).catch(reject);
          } catch (error) {
            reject(error);
          }
        }),
    ),
  );
  return [...new Map(pages.flat().map((event) => [event.id, event])).values()];
}

/** The shared mock channel history ordering and inclusive timestamp page. */
export function mockChannelHistoryPage(
  events: readonly RelayEvent[],
  filter: MockFilter,
): RelayEvent[] {
  return (
    events
      .filter((event) => {
        if (filter.kinds && !filter.kinds.includes(event.kind)) {
          return false;
        }
        if (filter.since !== undefined && event.created_at < filter.since) {
          return false;
        }
        if (filter.until !== undefined && event.created_at > filter.until) {
          return false;
        }
        return true;
      })
      // Relay order is `created_at DESC, id ASC` — match it (both the WS history
      // page and the `get_channel_messages_before` keyset are backed by that one
      // order in production, so the mock must be self-consistent too, else a
      // same-second slice returned here won't line up with the keyset's tiebreak
      // and the dense-second escape hatch can't prove completeness). Bare `until`
      // still can't advance past a second denser than one page; the composite
      // keyset is the escape hatch.
      .sort(
        (left, right) =>
          right.created_at - left.created_at || left.id.localeCompare(right.id),
      )
      .slice(0, filter.limit ?? 50)
      .sort(
        (left, right) =>
          left.created_at - right.created_at || left.id.localeCompare(right.id),
      )
  );
}
