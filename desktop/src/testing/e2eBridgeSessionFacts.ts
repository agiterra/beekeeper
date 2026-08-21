import type { RelayEvent } from "@/shared/api/types";
import {
  CODING_SESSION_EVENT_KINDS,
  KIND_CODING_SESSION_LEASE,
} from "@/shared/constants/kinds";

export type MockFilter = {
  "#a"?: string[];
  "#d"?: string[];
  "#e"?: string[];
  "#h"?: string[];
  "#p"?: string[];
  authors?: string[];
  ids?: string[];
  kinds?: number[];
  limit?: number;
  since?: number;
  until?: number;
};

/** Every durable coding-session fact kind served by the mock relay. */
const MOCK_SESSION_FACT_KINDS: ReadonlySet<number> = new Set(
  CODING_SESSION_EVENT_KINDS.filter(
    (kind) => kind !== KIND_CODING_SESSION_LEASE,
  ),
);

/**
 * Serve one durable coding-session query spanning several `#h` channels.
 *
 * The ordinary channel branch serves only `#h[0]`. Agent Progress reads every
 * channel it can see in one filter, so this responder must aggregate all of
 * them without weakening an optional author allowlist. Returning `true` means
 * the request matched this seam, including deliberately hung requests.
 */
export function respondToMockMultiChannelSessionFacts(
  filter: MockFilter,
  subId: string,
  getEvents: (channelId: string) => readonly RelayEvent[],
  send: (message: unknown[]) => void,
): boolean {
  const channels = filter["#h"] ?? [];
  const kinds = filter.kinds ?? [];
  if (
    channels.length <= 1 ||
    kinds.length === 0 ||
    !kinds.every((kind) => MOCK_SESSION_FACT_KINDS.has(kind))
  ) {
    return false;
  }

  const hungKinds = window.__BUZZ_E2E_HANG_PROJECT_QUERY_KINDS__ ?? [];
  if (kinds.some((kind) => hungKinds.includes(kind))) return true;

  const rejectedKinds = window.__BUZZ_E2E_REJECT_PROJECT_QUERY_KINDS__ ?? [];
  if (kinds.some((kind) => rejectedKinds.includes(kind))) {
    send(["CLOSED", subId, "mock session fact query failure"]);
    return true;
  }

  const authors = filter.authors?.map((author) => author.toLowerCase());
  for (const channel of channels) {
    for (const event of getEvents(channel)) {
      if (!kinds.includes(event.kind)) continue;
      if (authors && !authors.includes(event.pubkey.toLowerCase())) continue;
      send(["EVENT", subId, event]);
    }
  }
  send(["EOSE", subId]);
  return true;
}
