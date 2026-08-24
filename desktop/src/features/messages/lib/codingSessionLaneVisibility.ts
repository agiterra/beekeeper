/**
 * Which coding-session conversation lanes this client can actually open.
 *
 * The channel timeline and the unread/notification paths both need one answer
 * to the same question: "is this `cs-session`-tagged kind:9 renderable in a
 * lane the user can open?" Only if it is may the timeline hide it — and only
 * then may unread skip it. Anything else degrades to ordinary attributable
 * chat that counts normally, which is what keeps a member from hiding a
 * message from every new client by tagging it with an arbitrary UUID.
 *
 * This module is the small mutable registry behind that answer. It is
 * deliberately *not* in the coding-session lane library: that library stays
 * pure and takes the resolved refs as an argument. Population is owned by
 * `useCodingSessionLaneVisibility`, which only spins up a catalog subscription
 * for channels where lane-tagged chat has actually been observed.
 */
import {
  codingSessionLaneRef,
  shouldSuppressCodingSessionLaneMessageFromChannelTimeline,
} from "@/features/coding-sessions/lib/codingSessionConversationLane";

const EMPTY_REFS: ReadonlySet<string> = new Set<string>();

type ChannelLaneVisibility = {
  /** Refs seen on this channel's chat stream — the catalog-subscribe trigger. */
  observed: Set<string>;
  /** Refs resolved to an umbrella whose lane this client can open. */
  renderable: ReadonlySet<string>;
};

const byChannel = new Map<string, ChannelLaneVisibility>();
const listeners = new Set<() => void>();

function channelEntry(channelId: string): ChannelLaneVisibility {
  const existing = byChannel.get(channelId);
  if (existing) return existing;
  const created: ChannelLaneVisibility = {
    observed: new Set<string>(),
    renderable: EMPTY_REFS,
  };
  byChannel.set(channelId, created);
  return created;
}

function bump() {
  for (const listener of listeners) listener();
}

function sameRefs(left: ReadonlySet<string>, right: ReadonlySet<string>) {
  if (left === right) return true;
  if (left.size !== right.size) return false;
  for (const ref of left) {
    if (!right.has(ref)) return false;
  }
  return true;
}

/**
 * Record the lane refs carried by events arriving on a channel's chat stream.
 *
 * Observation is only a hint that a channel is worth resolving: it never makes
 * a ref renderable on its own (a forged ref would then hide its own message).
 * Returns whether anything new was observed.
 */
export function observeCodingSessionLaneRefs(
  channelId: string | null,
  events: readonly { kind: number; tags: string[][] }[],
): boolean {
  if (!channelId) return false;
  let didObserve = false;
  for (const event of events) {
    const sessionRef = codingSessionLaneRef(event);
    if (sessionRef === null) continue;
    const entry = channelEntry(channelId);
    if (entry.observed.has(sessionRef)) continue;
    entry.observed.add(sessionRef);
    didObserve = true;
  }
  if (didObserve) bump();
  return didObserve;
}

/** Channels where lane-tagged chat has been seen, newest registration last. */
export function codingSessionLaneObservedChannelIds(): string[] {
  const channelIds: string[] = [];
  for (const [channelId, entry] of byChannel) {
    if (entry.observed.size > 0) channelIds.push(channelId);
  }
  return channelIds;
}

/** The refs whose lanes this client can open in the given channel. */
export function codingSessionLaneRenderableRefs(
  channelId: string | null,
): ReadonlySet<string> {
  if (!channelId) return EMPTY_REFS;
  return byChannel.get(channelId)?.renderable ?? EMPTY_REFS;
}

/**
 * Replace a channel's renderable refs. Returns whether the set changed, so the
 * publisher can re-project timelines that were built while a ref was unknown.
 */
export function publishCodingSessionLaneRenderableRefs(
  channelId: string,
  refs: ReadonlySet<string>,
): boolean {
  const entry = channelEntry(channelId);
  if (sameRefs(entry.renderable, refs)) return false;
  entry.renderable = new Set(refs);
  bump();
  return true;
}

/**
 * The lane refs of umbrellas that render a conversation lane.
 *
 * Mirrors the two conditions the surface itself uses: an umbrella has a lane
 * only when it claimed a `sessionRef` (`codingSessionUmbrellaModel`'s "Session"
 * participant) and it renders the umbrella workspace at all
 * (`umbrellaHasCollapsedHistory`: more than one execution, OR one execution
 * carrying prior generations — a resumed session). An umbrella with nothing
 * collapsed shows no lane, so its tagged chat must stay in the channel
 * timeline. The predicate is restated structurally here rather than imported
 * so this module stays free of the coding-sessions model graph.
 */
export function codingSessionLaneRenderableRefsFromUmbrellas(
  umbrellas: readonly {
    sessionRef: string | null;
    executions: readonly { priorGenerations: readonly unknown[] }[];
  }[],
): Set<string> {
  const refs = new Set<string>();
  for (const umbrella of umbrellas) {
    if (umbrella.sessionRef === null) continue;
    const rendersUmbrella =
      umbrella.executions.length > 1 ||
      umbrella.executions.some((e) => e.priorGenerations.length > 0);
    if (!rendersUmbrella) continue;
    refs.add(umbrella.sessionRef);
  }
  return refs;
}

/**
 * The single resolution rule shared by the channel timeline, the unread
 * trigger set, and the notification paths: hidden here means renderable in a
 * lane the user can open, and never anything else.
 */
export function isCodingSessionLaneMessageHiddenFromChannel(
  channelId: string | null,
  event: { kind: number; tags: string[][] },
): boolean {
  return shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
    event,
    codingSessionLaneRenderableRefs(channelId),
  );
}

/** React `useSyncExternalStore` seam for the publisher hook. */
export function subscribeToCodingSessionLaneVisibility(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Drop every channel's lane knowledge — community-scoped state, so it must be
 * reset on a relay boundary change alongside the other module-level caches.
 */
export function resetCodingSessionLaneVisibility() {
  if (byChannel.size === 0) return;
  byChannel.clear();
  bump();
}
