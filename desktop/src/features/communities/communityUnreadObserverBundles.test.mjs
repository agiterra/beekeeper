/**
 * The observer's relay economy: one poll of an inactive community is three
 * bundles (membership; metadata + visibility + read state + mutes; every
 * channel's existence and mention filter), never 5 + 2N frames.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { DM_NOTIFIABLE_EVENT_KINDS } from "@/features/channels/isDmNotifiableKind";
import {
  CHANNEL_MESSAGE_EVENT_KINDS,
  HOME_MENTION_EVENT_KINDS,
  KIND_CHANNEL_MUTES,
  KIND_DM_VISIBILITY,
  KIND_READ_STATE,
} from "@/shared/constants/kinds";
import { fetchCommunityUnread } from "./communityUnreadObserver.ts";
import {
  CHANNEL_ID,
  KIND_NIP29_GROUP_MEMBERS,
  KIND_NIP29_GROUP_METADATA,
  PUBKEY,
  event,
  memberEvent,
  metadataEvent,
  readRelationships,
  relayFor,
} from "./communityUnreadObserverFixtures.mjs";

test("one poll is three bundles, and the unread bundle carries 2N filters", async () => {
  const STREAM = "channel-stream";
  const DM = "channel-dm";
  const relay = relayFor([
    () => [memberEvent([STREAM, DM])],
    () => [metadataEvent(STREAM, "stream"), metadataEvent(DM, "dm")],
    () => [],
    () => [],
    () => [],
    // STREAM: unread, then mention
    () => [
      event({
        id: "stream-unread".padEnd(64, "0"),
        created_at: 20,
        tags: [["h", STREAM]],
      }),
    ],
    () => [],
    // DM: unread, then mention
    () => [],
    () => [
      event({
        id: "dm-mention".padEnd(64, "0"),
        created_at: 30,
        tags: [
          ["h", DM],
          ["p", PUBKEY],
        ],
      }),
    ],
  ]);

  const result = await fetchCommunityUnread({
    client: relay,
    pubkey: PUBKEY,
    nowSeconds: 100,
    decryptReadState: async (value) => value,
    decryptMutes: async (value) => value,
    readThreadRelationships: readRelationships(),
    readForcedUnread: () => ({}),
  });

  assert.deepEqual(result, { hasUnread: true, mentionCount: 1 });

  // Bundle 1: the membership list alone (its ids feed the metadata #d).
  // Bundle 2: metadata, DM visibility, read state, mutes.
  // Bundle 3: every channel's existence + mention filter, 2N in one call.
  assert.equal(relay.batches.length, 3);
  assert.deepEqual(relay.batches[0], [
    { kinds: [KIND_NIP29_GROUP_MEMBERS], "#p": [PUBKEY], limit: 1000 },
  ]);
  assert.deepEqual(relay.batches[1], [
    { kinds: [KIND_NIP29_GROUP_METADATA], "#d": [STREAM, DM], limit: 1000 },
    { kinds: [KIND_DM_VISIBILITY], "#p": [PUBKEY], limit: 1 },
    {
      kinds: [KIND_READ_STATE],
      authors: [PUBKEY],
      "#t": ["read-state"],
      since: 100 - 7 * 24 * 60 * 60,
      limit: 500,
    },
    {
      kinds: [KIND_CHANNEL_MUTES],
      authors: [PUBKEY],
      "#d": ["channel-mutes"],
      limit: 1,
    },
  ]);
  assert.equal(relay.batches[2].length, 4);
  assert.deepEqual(relay.batches[2], [
    {
      kinds: [...CHANNEL_MESSAGE_EVENT_KINDS],
      "#h": [STREAM],
      since: 0,
      limit: 50,
    },
    {
      kinds: [...HOME_MENTION_EVENT_KINDS],
      "#h": [STREAM],
      "#p": [PUBKEY],
      since: 0,
      limit: 100,
    },
    {
      kinds: [...DM_NOTIFIABLE_EVENT_KINDS],
      "#h": [DM],
      since: 0,
      limit: 50,
    },
    {
      kinds: [...HOME_MENTION_EVENT_KINDS],
      "#h": [DM],
      "#p": [PUBKEY],
      since: 0,
      limit: 100,
    },
  ]);
});

test("a forced-unread channel drops every existence filter from the bundle, keeping only mentions", async () => {
  const relay = relayFor([
    () => [memberEvent([CHANNEL_ID])],
    () => [metadataEvent(CHANNEL_ID, "stream")],
    () => [],
    () => [],
    () => [],
    // Only the mention filter is sent for the channel.
    () => [],
  ]);

  const result = await fetchCommunityUnread({
    client: relay,
    pubkey: PUBKEY,
    nowSeconds: 100,
    decryptReadState: async (value) => value,
    decryptMutes: async (value) => value,
    readThreadRelationships: readRelationships(),
    readForcedUnread: () => ({ [CHANNEL_ID]: null }),
  });

  assert.deepEqual(result, { hasUnread: true, mentionCount: 0 });
  assert.equal(relay.batches.length, 3);
  assert.equal(relay.batches[2].length, 1);
  assert.deepEqual(relay.batches[2][0]["#p"], [PUBKEY]);
});
