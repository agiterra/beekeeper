/**
 * Fixtures for the community unread observer tests: a batch-aware fake relay
 * and event constructors that carry the kinds and tags the observer's filters
 * really match on. A fixture only reaches its consumer if it would have
 * passed that consumer's filter, because the observer demultiplexes every
 * bundle client-side.
 */
import { KIND_CHANNEL_MUTES } from "@/shared/constants/kinds";

export const PUBKEY = "a".repeat(64);
export const OTHER = "b".repeat(64);
export const CHANNEL_ID = "channel-1";
export const THREAD_ROOT = "c".repeat(64);
export const THREAD_ROOT_2 = "d".repeat(64);

export const EMPTY_RELATIONSHIPS = {
  participatedRootIds: new Set(),
  followedRootIds: new Set(),
  authoredRootIds: new Set(),
  mutedRootIds: new Set(),
};

export function readRelationships(overrides = {}) {
  return () => ({ ...EMPTY_RELATIONSHIPS, ...overrides });
}

export function event(overrides = {}) {
  return {
    id: overrides.id ?? `${Math.random()}`.padEnd(64, "0").slice(0, 64),
    pubkey: overrides.pubkey ?? OTHER,
    created_at: overrides.created_at ?? 100,
    kind: overrides.kind ?? 9,
    tags: overrides.tags ?? [],
    content: overrides.content ?? "",
    sig: overrides.sig ?? "sig",
  };
}

// One handler per filter, consumed in bundle order. Like the read-only
// client, a bundle resolves to the deduplicated union of every filter's
// events, so the observer has to demultiplex it by filter — a fixture only
// reaches its consumer if it really matches that consumer's filter.
export function relayFor(handlers) {
  return {
    requests: [],
    batches: [],
    async fetchEventsBatch(filters) {
      this.batches.push(filters);
      const seen = new Set();
      const union = [];
      for (const filter of filters) {
        this.requests.push(filter);
        for (const event of handlers.shift()?.(filter) ?? []) {
          if (seen.has(event.id)) continue;
          seen.add(event.id);
          union.push(event);
        }
      }
      return union;
    },
  };
}

export const KIND_NIP29_GROUP_METADATA = 39000;
export const KIND_NIP29_GROUP_MEMBERS = 39002;

export function memberEvent(channelIds) {
  return event({
    kind: KIND_NIP29_GROUP_MEMBERS,
    tags: [...channelIds.map((id) => ["d", id]), ["p", PUBKEY]],
  });
}

export function metadataEvent(channelId, channelType) {
  return event({
    kind: KIND_NIP29_GROUP_METADATA,
    tags: [
      ["d", channelId],
      ["t", channelType],
    ],
  });
}

export function mutesEvent(content) {
  return event({
    kind: KIND_CHANNEL_MUTES,
    pubkey: PUBKEY,
    tags: [["d", "channel-mutes"]],
    content,
  });
}

// Helper: encode a mutes payload as JSON (decryptMutes stub returns content as-is)
export function mutesContent(mutedIds) {
  const channels = {};
  for (const id of mutedIds) {
    channels[id] = { muted: true, updatedAt: 1 };
  }
  return JSON.stringify({ version: 1, channels });
}
