import assert from "node:assert/strict";
import test from "node:test";

import { DM_NOTIFIABLE_EVENT_KINDS } from "./isDmNotifiableKind.ts";
import {
  buildCatchUpFilters,
  catchUpSince,
  groupEventsByChannel,
} from "./channelCatchUpFilters.ts";
import { CATCH_UP_LIMIT } from "./unreadCatchUpScan.ts";
import { CHANNEL_MESSAGE_EVENT_KINDS } from "@/shared/constants/kinds";

function event(id, tags) {
  return {
    id,
    pubkey: "b".repeat(64),
    created_at: 1,
    kind: 9,
    tags,
    content: "",
    sig: "sig",
  };
}

test("each channel keeps its own since and limit, with kinds by channel type", () => {
  const filters = buildCatchUpFilters([
    { channelId: "stream-1", channelType: "stream", readAt: 100 },
    { channelId: "dm-1", channelType: "dm", readAt: null },
    { channelId: "unknown-1", channelType: undefined, readAt: 7 },
  ]);

  assert.deepEqual(filters, [
    {
      kinds: [...CHANNEL_MESSAGE_EVENT_KINDS],
      "#h": ["stream-1"],
      since: 101,
      limit: CATCH_UP_LIMIT,
    },
    {
      kinds: [...DM_NOTIFIABLE_EVENT_KINDS],
      "#h": ["dm-1"],
      since: 0,
      limit: CATCH_UP_LIMIT,
    },
    {
      kinds: [...CHANNEL_MESSAGE_EVENT_KINDS],
      "#h": ["unknown-1"],
      since: 8,
      limit: CATCH_UP_LIMIT,
    },
  ]);
});

test("since is strict-newer than the read marker, and 0 when there is none", () => {
  assert.equal(catchUpSince(null), 0);
  assert.equal(catchUpSince(0), 1);
  assert.equal(catchUpSince(1_700_000_000), 1_700_000_001);
});

test("a batch union is grouped by h, in order, and an h-less event is dropped", () => {
  const a1 = event("a1", [["h", "chan-a"]]);
  const b1 = event("b1", [["h", "chan-b"]]);
  const a2 = event("a2", [
    ["e", "root"],
    ["h", "chan-a"],
  ]);
  const orphan = event("orphan", [["e", "root"]]);

  const grouped = groupEventsByChannel([a1, b1, orphan, a2]);

  assert.deepEqual([...grouped.keys()], ["chan-a", "chan-b"]);
  assert.deepEqual(grouped.get("chan-a"), [a1, a2]);
  assert.deepEqual(grouped.get("chan-b"), [b1]);
  assert.equal(grouped.has("orphan"), false);
});

test("an empty union groups to an empty map", () => {
  assert.equal(groupEventsByChannel([]).size, 0);
});
