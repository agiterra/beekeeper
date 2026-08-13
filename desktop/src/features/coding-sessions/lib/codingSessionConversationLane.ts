/**
 * The umbrella session's conversation lane: ordinary kind:9 chat in the host
 * channel carrying one extra tag, `["cs-session", "<sessionRef>"]`.
 *
 * The relay already permits the tag with zero changes; old clients render
 * lane messages as normal attributable chat (graceful degradation). This
 * module is the pure model side only — building the wire form, projecting the
 * channel's kind:9 stream into a lane, and the suppression predicate the
 * channel timeline uses to keep lane traffic out of ordinary chat.
 */
import { KIND_STREAM_MESSAGE } from "@/shared/constants/kinds";
import {
  isCodingSessionSessionRef,
  isExactProviderAuthorityPubkey,
} from "./codingSessionWireDecode";

/** Tag name that scopes a kind:9 message to an umbrella session. */
export const CODING_SESSION_LANE_TAG = "cs-session" as const;

/** Maximum UTF-8 byte length for a lane message body. */
export const MAX_CODING_SESSION_LANE_MESSAGE_BYTES = 12 * 1024;

/** Unsigned event input for a lane message, ready for the OS keystore. */
export type CodingSessionLaneMessageEventInput = {
  kind: number;
  content: string;
  tags: string[][];
};

/** One verified-enough lane message, projected for rendering. */
export type CodingSessionLaneMessage = {
  eventId: string;
  sessionRef: string;
  /** Host channel from the `h` tag, or null when the event carried none. */
  channelId: string | null;
  authorPubkey: string;
  content: string;
  /** Event `created_at` (seconds) converted to milliseconds. */
  timestampMs: number;
};

/** The minimal event surface the lane reads; a `RelayEvent` satisfies it. */
export type CodingSessionLaneEventLike = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/**
 * Build the unsigned kind:9 event for a message addressed to the Session
 * participant. Tag order is fixed: `h`, `cs-session`, then any `p` mentions.
 */
export function buildCodingSessionLaneMessageEvent(input: {
  channelId: string;
  sessionRef: string;
  content: string;
  mentionPubkeys?: readonly string[];
}): CodingSessionLaneMessageEventInput {
  if (input.channelId.trim().length === 0) {
    throw new Error("channelId must not be empty");
  }
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error("sessionRef must be a canonical lowercase hyphenated UUID");
  }
  if (input.content.trim().length === 0) {
    throw new Error("a session lane message must not be empty");
  }
  if (
    new TextEncoder().encode(input.content).byteLength >
    MAX_CODING_SESSION_LANE_MESSAGE_BYTES
  ) {
    throw new Error(
      `a session lane message exceeds ${MAX_CODING_SESSION_LANE_MESSAGE_BYTES} bytes`,
    );
  }
  const mentions = input.mentionPubkeys ?? [];
  for (const pubkey of mentions) {
    if (!isExactProviderAuthorityPubkey(pubkey)) {
      throw new Error("mention pubkeys must be lowercase 64-hex public keys");
    }
  }
  return {
    kind: KIND_STREAM_MESSAGE,
    content: input.content,
    tags: [
      ["h", input.channelId],
      [CODING_SESSION_LANE_TAG, input.sessionRef],
      ...mentions.map((pubkey) => ["p", pubkey]),
    ],
  };
}

/**
 * The umbrella session a kind:9 event is scoped to, or null when it is
 * ordinary chat.
 *
 * Deliberately strict about ambiguity: an event carrying two *different*
 * `cs-session` values, or a malformed ref, resolves to null — it then renders
 * as plain attributable chat rather than vanishing from both surfaces.
 */
export function codingSessionLaneRef(event: {
  kind: number;
  tags: string[][];
}): string | null {
  if (event.kind !== KIND_STREAM_MESSAGE) return null;
  const refs = new Set<string>();
  for (const tag of event.tags) {
    if (!Array.isArray(tag) || tag[0] !== CODING_SESSION_LANE_TAG) continue;
    if (typeof tag[1] !== "string") return null;
    refs.add(tag[1]);
  }
  if (refs.size !== 1) return null;
  const [ref] = refs;
  return isCodingSessionSessionRef(ref) ? ref : null;
}

/** Whether a kind:9 event belongs to the given umbrella's conversation lane. */
export function isCodingSessionLaneMessage(
  event: { kind: number; tags: string[][] },
  sessionRef: string,
): boolean {
  return (
    isCodingSessionSessionRef(sessionRef) &&
    codingSessionLaneRef(event) === sessionRef
  );
}

/** The host channel an event claims through its `h` tag, or null. */
export function codingSessionLaneEventChannelId(event: {
  tags: string[][];
}): string | null {
  return readChannelTag(event.tags);
}

/**
 * Admission predicate for one umbrella's lane: the event must be a lane
 * message for exactly this `sessionRef` **and** carry this channel's `h` tag.
 *
 * Both halves are load-bearing. A relay that answers `#cs-session` loosely, or
 * a member who reuses a ref from another channel, would otherwise place
 * foreign chat in the lane; the lane's own subscription is `#h`-scoped, so
 * re-checking the tag here makes the client's guarantee independent of the
 * relay's filtering.
 */
export function isCodingSessionLaneEventForChannel(
  event: { kind: number; tags: string[][] },
  channelId: string,
  sessionRef: string,
): boolean {
  return (
    isCodingSessionLaneMessage(event, sessionRef) &&
    codingSessionLaneEventChannelId(event) === channelId
  );
}

/**
 * Whether a session ref names a lane this client can actually open.
 *
 * Either the resolved set of refs for the host channel, or a predicate over
 * one ref. The lane library never learns this by itself — the caller owns the
 * catalog knowledge and passes it in, so this module stays pure.
 */
export type CodingSessionLaneRefResolver =
  | ReadonlySet<string>
  | ((sessionRef: string) => boolean);

function resolvesLaneRef(
  resolver: CodingSessionLaneRefResolver,
  sessionRef: string,
): boolean {
  return typeof resolver === "function"
    ? resolver(sessionRef)
    : resolver.has(sessionRef);
}

/**
 * Suppression predicate for the channel timeline: a lane message is hidden
 * from ordinary chat only when it is renderable in a lane the user can open —
 * i.e. `renderableSessionRefs` resolves its `cs-session` ref for the host
 * channel. Everything else stays visible:
 *
 * - a malformed or ambiguous lane tag (this client cannot scope it),
 * - a ref this client has never resolved to a lane-bearing umbrella. That
 *   covers lane chat that arrives before the umbrella's second execution is
 *   ingested, single-execution sessions (which render no lane), and the
 *   hidden-message injection any member would otherwise get for free by
 *   tagging a channel message with an arbitrary valid UUID.
 *
 * Suppressing therefore always means "shown somewhere else", never "shown
 * nowhere". The caller must pass the same resolver to the unread/notification
 * paths, so a message that stays in the timeline still counts normally.
 */
export function shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
  event: {
    kind: number;
    tags: string[][];
  },
  renderableSessionRefs: CodingSessionLaneRefResolver,
): boolean {
  const sessionRef = codingSessionLaneRef(event);
  if (sessionRef === null) return false;
  return resolvesLaneRef(renderableSessionRefs, sessionRef);
}

/**
 * Nostr REQ filter for a dedicated lane subscription. Always carries explicit
 * `kinds` (the relay p-gates open-ended queries) and stays scoped to the host
 * channel, preserving the community boundary.
 */
export function buildCodingSessionLaneFilter(
  channelId: string,
  sessionRef: string,
): {
  kinds: number[];
  "#h": string[];
  "#cs-session": string[];
} {
  return {
    kinds: [KIND_STREAM_MESSAGE],
    "#h": [channelId],
    "#cs-session": [sessionRef],
  };
}

/**
 * Project the channel's kind:9 stream into one umbrella's conversation lane,
 * ordered by `created_at` (tie-break: event id) so the ordering never depends
 * on arrival.
 */
export function projectCodingSessionLaneMessages(
  events: readonly CodingSessionLaneEventLike[],
  sessionRef: string,
): CodingSessionLaneMessage[] {
  if (!isCodingSessionSessionRef(sessionRef)) return [];
  return events
    .filter((event) => codingSessionLaneRef(event) === sessionRef)
    .map((event) => ({
      eventId: event.id,
      sessionRef,
      channelId: readChannelTag(event.tags),
      authorPubkey: event.pubkey,
      content: event.content,
      timestampMs: event.created_at * 1000,
    }))
    .sort(
      (left, right) =>
        left.timestampMs - right.timestampMs ||
        left.eventId.localeCompare(right.eventId),
    );
}

function readChannelTag(tags: string[][]): string | null {
  for (const tag of tags) {
    if (Array.isArray(tag) && tag[0] === "h" && typeof tag[1] === "string") {
      return tag[1];
    }
  }
  return null;
}
