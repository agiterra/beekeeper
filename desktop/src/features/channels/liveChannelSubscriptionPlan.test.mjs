import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_CHANNEL_VALUES_PER_REQUEST,
  MAX_FILTERS_PER_REQ,
} from "@/shared/api/relayClientShared";
import {
  CHANNEL_EVENT_KINDS,
  HOME_MENTION_EVENT_KINDS,
} from "@/shared/constants/kinds";
import { matchesFilter } from "@/shared/api/relayFilterMatch";
import {
  chunkChannelIds,
  liveRequestKey,
  mentionMatchFilter,
  planLiveChannelFilters,
} from "./liveChannelSubscriptionPlan.ts";

const ME = "a".repeat(64);
const NOW = 1_800_000_000;

function ids(count) {
  return Array.from(
    { length: count },
    (_, i) => `channel-${String(i).padStart(5, "0")}`,
  );
}

function filterCount(plan) {
  return plan.reduce((sum, group) => sum + group.length, 0);
}

test("25 channels plan as one live filter on one REQ, with mentions matched client-side", () => {
  const plan = planLiveChannelFilters(ids(25), ME, NOW);

  assert.equal(plan.live.length, 1);
  assert.deepEqual(plan.live[0], [
    {
      kinds: [...CHANNEL_EVENT_KINDS],
      "#h": ids(25),
      limit: 0,
      since: NOW,
    },
  ]);
  // The relay-shaped `#p` clause, applied to the live stream: a second REQ
  // carrying it would deliver a strict subset of the first.
  assert.deepEqual(plan.mention, {
    kinds: [...HOME_MENTION_EVENT_KINDS],
    "#p": [ME],
    limit: 0,
  });
});

test("the mention matcher agrees with the relay's kind + #p clause", () => {
  const { mention } = planLiveChannelFilters(ids(1), ME, NOW);
  const base = {
    id: "e".repeat(64),
    pubkey: "b".repeat(64),
    created_at: NOW,
    kind: 9,
    content: "",
    sig: "sig",
  };

  assert.equal(
    matchesFilter(
      {
        ...base,
        tags: [
          ["h", "channel-00000"],
          ["p", ME],
        ],
      },
      mention,
    ),
    true,
  );
  assert.equal(
    matchesFilter({ ...base, tags: [["h", "channel-00000"]] }, mention),
    false,
  );
  assert.equal(
    matchesFilter(
      {
        ...base,
        kind: 7,
        tags: [
          ["h", "channel-00000"],
          ["p", ME],
        ],
      },
      mention,
    ),
    false,
  );
});

test("300 channels chunk at 128 into three filters", () => {
  const plan = planLiveChannelFilters(ids(300), ME, NOW);

  assert.equal(filterCount(plan.live), 3);
  assert.deepEqual(
    plan.live.flat().map((filter) => filter["#h"].length),
    [128, 128, 44],
  );
});

test("the 128 cap is aggregate per REQ, so every full chunk is its own REQ group", () => {
  // The relay bounds the *sum* of `#h` values across every filter in one
  // REQ (MAX_EXPLICIT_CHANNEL_VALUES in handlers/req.rs), not the count per
  // filter. Ten 128-channel filters therefore cannot share a REQ; 1300
  // channels are 11 chunks and 11 REQs, and any group that does hold
  // several filters stays under both caps.
  const plan = planLiveChannelFilters(ids(1300), ME, NOW);

  assert.equal(filterCount(plan.live), 11);
  assert.equal(plan.live.length, 11);
  for (const group of plan.live) {
    assert.ok(group.length >= 1 && group.length <= MAX_FILTERS_PER_REQ);
    const aggregate = group.reduce(
      (sum, filter) => sum + filter["#h"].length,
      0,
    );
    assert.ok(aggregate <= MAX_CHANNEL_VALUES_PER_REQUEST);
  }
});

test("ids are deduplicated and sorted so the same membership always plans the same chunks", () => {
  const plan = planLiveChannelFilters(["b", "a", "b", "c"], ME, NOW);

  assert.deepEqual(plan.live[0][0]["#h"], ["a", "b", "c"]);
  assert.deepEqual(chunkChannelIds(["a", "b", "c"], 2), [["a", "b"], ["c"]]);
});

test("without a pubkey there is no mention matcher, and no channels means no REQs at all", () => {
  assert.equal(planLiveChannelFilters(ids(3), "", NOW).mention, null);
  assert.equal(planLiveChannelFilters(ids(3), "   ", NOW).mention, null);
  assert.equal(mentionMatchFilter(""), null);
  assert.deepEqual(planLiveChannelFilters([], ME, NOW).live, []);
});

test("the pubkey is normalised into #p the way the hook compares authors", () => {
  const plan = planLiveChannelFilters(ids(1), ` ${ME.toUpperCase()} `, NOW);
  assert.deepEqual(plan.mention["#p"], [ME]);
});

test("a REQ group's key ignores since, so an unchanged bundle is not reopened", () => {
  const earlier = planLiveChannelFilters(ids(2), ME, NOW);
  const later = planLiveChannelFilters(ids(2), ME, NOW + 60);
  const grown = planLiveChannelFilters(ids(3), ME, NOW + 60);

  assert.equal(liveRequestKey(earlier.live[0]), liveRequestKey(later.live[0]));
  assert.notEqual(
    liveRequestKey(earlier.live[0]),
    liveRequestKey(grown.live[0]),
  );
});
