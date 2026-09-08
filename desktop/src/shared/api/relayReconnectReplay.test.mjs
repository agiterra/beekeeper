import assert from "node:assert/strict";
import test from "node:test";

import {
  buildReconnectReplayFilter,
  PAGE_REPLAY_MAX_ATTEMPTS,
  replayLiveSubscriptions,
  shouldPageReconnectReplay,
} from "./relayReconnectReplay.ts";
import { buildChannelFilter } from "./relayChannelFilters.ts";
import { prepareSubscriptionEvent } from "./relayClosedRecovery.ts";

// ── Fake-timer + Date.now setup for gate tests ────────────────────────────────

let fakeNow = 0;
const pendingTimers = new Map();
let nextTimerId = 1;

function fakeSetTimeout(fn, ms) {
  const id = nextTimerId++;
  pendingTimers.set(id, { fn, fireAt: fakeNow + ms });
  return id;
}

function fakeClearTimeout(id) {
  pendingTimers.delete(id);
}

function tickTo(ms) {
  fakeNow = ms;
  for (const [id, { fn, fireAt }] of Array.from(pendingTimers.entries())) {
    if (fireAt <= fakeNow) {
      pendingTimers.delete(id);
      fn();
    }
  }
}

globalThis.window = {
  setTimeout: fakeSetTimeout,
  clearTimeout: fakeClearTimeout,
};

const origDateNow = Date.now;
function setFakeNow(ms) {
  fakeNow = ms;
  Date.now = () => fakeNow;
}

// Import gate module AFTER window shim so it picks up the fake timers.
const { activateRateLimit, resetRateLimitGate } = await import(
  "./relayRateLimitGate.ts"
);

function resetGate(startMs = 0) {
  pendingTimers.clear();
  nextTimerId = 1;
  setFakeNow(startMs);
  resetRateLimitGate();
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function event(id, createdAt) {
  return {
    id,
    pubkey: "pubkey",
    created_at: createdAt,
    kind: 9,
    tags: [],
    content: "",
    sig: "sig",
  };
}

function eventRange(prefix, start, count) {
  return Array.from({ length: count }, (_, index) =>
    event(`${prefix}-${index}`, start + index),
  );
}

function replayFilter(filter, since, until) {
  return buildReconnectReplayFilter(filter, since, until);
}

// ── buildReconnectReplayFilter ────────────────────────────────────────────────

test("reconnect replay preserves small steady-state limits when adding since", () => {
  const filter = {
    kinds: [9, 40002],
    "#h": ["channel-1"],
    limit: 50,
  };

  assert.deepEqual(replayFilter(filter, 123), {
    kinds: [9, 40002],
    "#h": ["channel-1"],
    limit: 50,
    since: 123,
  });
});

test("reconnect replay caps large steady-state limits", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 1000,
  };

  assert.deepEqual(replayFilter(filter, 123), {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 500,
    since: 123,
  });
});

test("reconnect replay preserves the live-only zero-history contract", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 0,
  };

  assert.deepEqual(replayFilter(filter, 123), {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 0,
    since: 123,
  });
});

test("live-only subscriptions do not page reconnect history", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 0,
  };

  assert.equal(shouldPageReconnectReplay(filter), false);
});

test("reconnect replay keeps the stricter existing since window", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 50,
    since: 200,
  };

  assert.deepEqual(replayFilter(filter, 123), {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 50,
    since: 200,
  });
});

test("reconnect replay applies the stricter until window", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 50,
    until: 300,
  };

  assert.deepEqual(replayFilter(filter, 123, 400), {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 50,
    since: 123,
    until: 300,
  });
});

test("initial subscription replay preserves the original filter", () => {
  const filter = {
    kinds: [9],
    "#h": ["channel-1"],
    limit: 50,
  };

  assert.equal(replayFilter(filter, undefined), filter);
});

// ── Pacing through the send budget ───────────────────────────────────────────

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

test("replay paces live REQs through the send budget: no 5 s window exceeds the read lane", async () => {
  resetGate(0);
  const { RelaySendBudget, LOCAL_BURST_CAPACITY, WRITE_RESERVE } = await import(
    "./relaySendBudget.ts"
  );
  const budget = new RelaySendBudget({
    now: () => fakeNow,
    setTimeoutFn: fakeSetTimeout,
    clearTimeoutFn: fakeClearTimeout,
  });
  const subCount = 60;
  const sentAtMs = [];
  const subscriptions = new Map(
    Array.from({ length: subCount }, (_, i) => [
      `sub-${i}`,
      {
        mode: "live",
        filters: [{ kinds: [9], "#h": [`ch-${i}`], limit: 0 }],
        onEvent: () => {},
      },
    ]),
  );

  const replayPromise = replayLiveSubscriptions({
    subscriptions,
    // The session's sendRaw charges the read lane before every REQ frame.
    sendRaw: async () => {
      await budget.acquire("read");
      sentAtMs.push(fakeNow);
    },
    requestHistoryBatch: async () => [],
  });

  // Drive the fake clock until every REQ is out; the budget wakes waiters on
  // fake timers as the window slides.
  for (let step = 0; sentAtMs.length < subCount && step < 10_000; step++) {
    await settle();
    tickTo(fakeNow + 100);
  }
  await replayPromise;

  assert.equal(sentAtMs.length, subCount, "every subscription was replayed");
  const readCap = LOCAL_BURST_CAPACITY - WRITE_RESERVE;
  for (const start of sentAtMs) {
    const inWindow = sentAtMs.filter(
      (ms) => ms >= start && ms < start + 5_000,
    ).length;
    assert.ok(
      inWindow <= readCap,
      `${inWindow} REQs in the 5 s window from ${start} ms exceeds the read lane cap ${readCap}`,
    );
  }
  assert.ok(
    sentAtMs[sentAtMs.length - 1] >= 5_000,
    "60 REQs cannot fit in one window; the tail must have waited",
  );
});

test("replay sends one REQ per subscription carrying all its filters", async () => {
  resetGate();
  const sent = [];
  const filters = [
    { kinds: [9], "#h": ["ch-a", "ch-b"], limit: 0 },
    { kinds: [7], "#p": ["me"], limit: 0 },
    { kinds: [40002], limit: 0 },
  ];
  const subscriptions = new Map([
    ["sub-many", { mode: "live", filters, onEvent: () => {} }],
  ]);

  await replayLiveSubscriptions({
    subscriptions,
    sendRaw: async (payload) => {
      sent.push(payload);
    },
    requestHistoryBatch: async () => [],
  });

  assert.deepEqual(sent, [["REQ", "sub-many", ...filters]]);
});

// ── Visible-channel priority ──────────────────────────────────────────────────

test("visible channel subscription is sent first", async () => {
  resetGate();
  const sentOrder = [];

  const subscriptions = new Map([
    [
      "other-1",
      {
        mode: "live",
        filters: [{ kinds: [9], "#h": ["ch-other"], limit: 50 }],
        onEvent: () => {},
        lastSeenCreatedAt: undefined,
      },
    ],
    [
      "visible-sub",
      {
        mode: "live",
        filters: [{ kinds: [9], "#h": ["ch-visible"], limit: 50 }],
        onEvent: () => {},
        lastSeenCreatedAt: undefined,
      },
    ],
    [
      "other-2",
      {
        mode: "live",
        filters: [{ kinds: [9], "#h": ["ch-other2"], limit: 50 }],
        onEvent: () => {},
        lastSeenCreatedAt: undefined,
      },
    ],
  ]);

  await replayLiveSubscriptions({
    subscriptions,
    sendRaw: async (payload) => {
      sentOrder.push(payload[1]);
    },
    requestHistoryBatch: async () => [],
    visibleChannelId: "ch-visible",
  });

  assert.equal(sentOrder[0], "visible-sub", "visible sub sent first");
  assert.equal(sentOrder.length, 3);
});

// ── Rate-limit gate deferral ──────────────────────────────────────────────────

test("replay waits for rate-limit gate before sending REQs", async () => {
  resetGate(0);
  activateRateLimit(5); // gate active for 5 seconds

  const sentIds = [];

  const replayPromise = replayLiveSubscriptions({
    subscriptions: new Map([
      [
        "sub-1",
        {
          mode: "live",
          filters: [{ kinds: [9], "#h": ["ch-1"], limit: 50 }],
          onEvent: () => {},
          lastSeenCreatedAt: undefined,
        },
      ],
    ]),
    sendRaw: async (payload) => {
      sentIds.push(payload[1]);
    },
    requestHistoryBatch: async () => [],
    setTimeoutFn: (fn, _ms) => {
      fn();
      return 0;
    },
  });

  // Gate expires — replay should proceed now.
  tickTo(5_001);

  await replayPromise;

  assert.equal(sentIds.length, 1, "REQ sent after gate expired");
});

// ── Connection-generation guard ───────────────────────────────────────────────

test("stale replay sends no REQs when generation advances while gate was active", async () => {
  resetGate(0);
  activateRateLimit(5); // gate active for 5 seconds

  let generationActive = true; // true = current, false = stale
  const sentIds = [];

  const replayPromise = replayLiveSubscriptions({
    subscriptions: new Map([
      [
        "sub-1",
        {
          mode: "live",
          filters: [{ kinds: [9], "#h": ["ch-1"], limit: 50 }],
          onEvent: () => {},
          lastSeenCreatedAt: undefined,
        },
      ],
    ]),
    sendRaw: async (payload) => {
      sentIds.push(payload[1]);
    },
    requestHistoryBatch: async () => [],
    isActive: () => generationActive,
  });

  // Advance the generation (simulate new connection) before the gate resolves.
  generationActive = false;

  // Fire the gate timer.
  tickTo(5_001);

  await replayPromise;

  assert.equal(sentIds.length, 0, "no REQs sent for a stale replay");
});

// ── Paged replay (existing behaviour) ────────────────────────────────────────

test("channel reconnect replay pages the missed window until a short page", async () => {
  resetGate();
  const delivered = [];
  const historyFilters = [];
  const sentPayloads = [];
  const pages = [
    eventRange("newest", 1501, 500),
    eventRange("middle", 1002, 500),
    eventRange("oldest", 995, 8),
  ];
  const filter = buildChannelFilter("channel-1", 50);
  const subscriptions = new Map([
    [
      "live-1",
      {
        mode: "live",
        filters: [filter],
        onEvent: (event) => delivered.push(event),
        lastSeenCreatedAt: 1000,
      },
    ],
  ]);

  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async (payload) => {
      sentPayloads.push(payload);
    },
    requestHistoryBatch: async ([filter]) => {
      historyFilters.push(filter);
      return pages.shift() ?? [];
    },
  });

  assert.deepEqual(sentPayloads, [
    [
      "REQ",
      "live-1",
      {
        kinds: filter.kinds,
        "#h": ["channel-1"],
        limit: 50,
      },
    ],
  ]);
  assert.deepEqual(historyFilters, [
    {
      kinds: filter.kinds,
      "#h": ["channel-1"],
      limit: 500,
      since: 995,
      until: 2000,
    },
    {
      kinds: filter.kinds,
      "#h": ["channel-1"],
      limit: 500,
      since: 995,
      until: 1501,
    },
    {
      kinds: filter.kinds,
      "#h": ["channel-1"],
      limit: 500,
      since: 995,
      until: 1002,
    },
  ]);
  assert.equal(delivered.length, 1008);
});

test("reconnect replay starts live REQs in parallel and preserves per-sub page order", async () => {
  resetGate();
  const sentPayloads = [];
  const sendResolvers = [];
  const historyFiltersByChannel = {
    "channel-1": [],
    "channel-2": [],
  };
  const pagesByChannel = {
    "channel-1": [
      eventRange("c1-full", 1501, 500),
      eventRange("c1-short", 1490, 2),
    ],
    "channel-2": [
      eventRange("c2-full", 1701, 500),
      eventRange("c2-short", 1690, 2),
    ],
  };
  const subscriptions = new Map([
    [
      "live-1",
      {
        mode: "live",
        filters: [buildChannelFilter("channel-1", 50)],
        onEvent: () => {},
        lastSeenCreatedAt: 1000,
      },
    ],
    [
      "live-2",
      {
        mode: "live",
        filters: [buildChannelFilter("channel-2", 50)],
        onEvent: () => {},
        lastSeenCreatedAt: 1000,
      },
    ],
  ]);

  const replayPromise = replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    pageReplayConcurrency: 2,
    sendRaw: (payload) => {
      sentPayloads.push(payload);
      return new Promise((resolve) => {
        sendResolvers.push(resolve);
      });
    },
    requestHistoryBatch: async ([filter]) => {
      const channelId = filter["#h"]?.[0];
      historyFiltersByChannel[channelId].push(filter.until);
      return pagesByChannel[channelId].shift() ?? [];
    },
  });

  await Promise.resolve();

  assert.deepEqual(
    sentPayloads.map((payload) => payload[1]),
    ["live-1", "live-2"],
  );
  assert.equal(sendResolvers.length, 2);
  assert.deepEqual(historyFiltersByChannel, {
    "channel-1": [],
    "channel-2": [],
  });

  for (const resolve of sendResolvers) {
    resolve();
  }
  await replayPromise;

  assert.deepEqual(historyFiltersByChannel, {
    "channel-1": [2000, 1501],
    "channel-2": [2000, 1701],
  });
});

// ── Per-REQ gate re-check ─────────────────────────────────────────────────────

test("a refusal mid-replay arms the gate: the remaining REQs wait until it expires", async () => {
  // The gate is inactive when replay starts. The third REQ (simulating the
  // relay's admission control) arms it; every later REQ must wait until the
  // gate clears before it is sent.
  resetGate(0);
  const sentAtMs = [];
  let armGate;
  const gateArmed = new Promise((resolve) => {
    armGate = resolve;
  });
  const subscriptions = new Map(
    Array.from({ length: 6 }, (_, i) => [
      `sub-${i}`,
      {
        mode: "live",
        filters: [{ kinds: [9], "#h": [`ch-${i}`], limit: 50 }],
        onEvent: () => {},
      },
    ]),
  );

  const replayPromise = replayLiveSubscriptions({
    subscriptions,
    sendConcurrency: 1,
    sendRaw: async () => {
      sentAtMs.push(fakeNow);
      if (sentAtMs.length === 3) {
        activateRateLimit(5);
        armGate();
      }
    },
    requestHistoryBatch: async () => [],
  });

  await gateArmed;
  tickTo(5_001);
  await replayPromise;

  assert.equal(
    sentAtMs.filter((ms) => ms < 5_001).length,
    3,
    "three REQs went out before the refusal",
  );
  assert.equal(
    sentAtMs.filter((ms) => ms >= 5_001).length,
    3,
    "the rest waited for the gate to expire",
  );
});

// ── Per-channel cursors and batched backfill ─────────────────────────────────

test("multi-#h subscription replays from the earliest channel cursor with limit 0 and backfills each channel in one batch", async () => {
  resetGate(0);
  const delivered = [];
  const batches = [];
  const filter = {
    kinds: [...buildChannelFilter("x", 0).kinds],
    "#h": ["ch-1", "ch-2", "ch-3"],
    limit: 0,
    since: 900,
  };
  const subscription = {
    mode: "live",
    filters: [filter],
    onEvent: (event) => delivered.push(event.id),
    lastSeenCreatedAt: 1500,
  };
  // ch-1 and ch-2 saw events at different times; ch-3 only inherits the
  // subscription-wide cursor.
  prepareSubscriptionEvent(subscription, {
    ...event("seen-1", 1200),
    tags: [["h", "ch-1"]],
  });
  prepareSubscriptionEvent(subscription, {
    ...event("seen-2", 1500),
    tags: [["h", "ch-2"]],
  });
  const sent = [];
  const subscriptions = new Map([["live-many", subscription]]);

  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async (payload) => {
      sent.push(payload);
    },
    requestHistoryBatch: async (filters) => {
      batches.push(filters);
      return [
        { ...event("missed-1", 1300), tags: [["h", "ch-1"]] },
        { ...event("missed-2", 1600), tags: [["h", "ch-2"]] },
      ];
    },
  });

  assert.deepEqual(sent, [
    ["REQ", "live-many", { ...filter, since: 1195, limit: 0 }],
  ]);
  assert.equal(batches.length, 1, "one batched read for all channels");
  assert.deepEqual(
    batches[0].map((f) => [f["#h"][0], f.since, f.until, f.limit]),
    [
      ["ch-1", 1195, 2000, 500],
      ["ch-2", 1495, 2000, 500],
      ["ch-3", 1495, 2000, 500],
    ],
  );
  assert.deepEqual(delivered, ["missed-1", "missed-2"]);
  assert.equal(
    subscription.pendingReplaySince,
    undefined,
    "a completed batch backfill clears the pinned floor",
  );
});

test("multi-#h backfill keeps paging only the channels that filled their page", async () => {
  resetGate(0);
  const batches = [];
  const filter = {
    kinds: [...buildChannelFilter("x", 0).kinds],
    "#h": ["ch-full", "ch-short"],
    limit: 0,
  };
  const subscription = {
    mode: "live",
    filters: [filter],
    onEvent: () => {},
    lastSeenCreatedAt: 1000,
  };
  const subscriptions = new Map([["live-many", subscription]]);
  const fullPage = eventRange("full", 1501, 500).map((e) => ({
    ...e,
    tags: [["h", "ch-full"]],
  }));

  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    requestHistoryBatch: async (filters) => {
      batches.push(filters.map((f) => [f["#h"][0], f.since, f.until]));
      if (batches.length === 1) {
        return [
          ...fullPage,
          { ...event("short", 1400), tags: [["h", "ch-short"]] },
        ];
      }
      return [{ ...event("older", 1100), tags: [["h", "ch-full"]] }];
    },
  });

  assert.deepEqual(batches, [
    [
      ["ch-full", 995, 2000],
      ["ch-short", 995, 2000],
    ],
    [["ch-full", 995, 1501]],
  ]);
});

// ── Backfill failure containment ─────────────────────────────────────────────

test("history backfill rejection never escapes replayLiveSubscriptions", async () => {
  resetGate(0);
  const filter = buildChannelFilter("channel-1", 50);
  const subscriptions = new Map([
    [
      "live-1",
      {
        mode: "live",
        filters: [filter],
        onEvent: () => {},
        lastSeenCreatedAt: 1000,
      },
    ],
  ]);

  let historyCalls = 0;
  // Must resolve — a rejection here is the socket-killing flap regression.
  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    requestHistoryBatch: async () => {
      historyCalls++;
      throw new Error("rate-limited: quota exceeded; retry in 4s");
    },
  });

  assert.equal(
    historyCalls,
    PAGE_REPLAY_MAX_ATTEMPTS,
    "backfill must retry a bounded number of times, then degrade",
  );
});

test("backfill retry waits out the armed gate, then succeeds", async () => {
  resetGate(0);
  const delivered = [];
  const filter = buildChannelFilter("channel-1", 50);
  const subscriptions = new Map([
    [
      "live-1",
      {
        mode: "live",
        filters: [filter],
        onEvent: (event) => delivered.push(event),
        lastSeenCreatedAt: 1000,
      },
    ],
  ]);

  const attemptAtMs = [];
  let armGate;
  const gateArmed = new Promise((resolve) => {
    armGate = resolve;
  });
  const replayPromise = replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    requestHistoryBatch: async () => {
      attemptAtMs.push(fakeNow);
      if (attemptAtMs.length === 1) {
        // Mirror relayClosedRecovery: the CLOSED handler arms the gate
        // before rejecting the history promise.
        activateRateLimit(4);
        armGate();
        throw new Error("rate-limited: quota exceeded; retry in 4s");
      }
      return [event("recovered", 1500)];
    },
  });

  // Wait until the gate is actually armed, then expire it. The retry loop is
  // (or will be) suspended in waitForRateLimit; expiring the gate releases it.
  await gateArmed;
  tickTo(4_001);
  await replayPromise;

  assert.equal(attemptAtMs.length, 2, "one failure, one retry");
  assert.ok(
    attemptAtMs[1] >= 4_001,
    "retry must not fire before the rate-limit gate expires",
  );
  assert.deepEqual(
    delivered.map((e) => e.id),
    ["recovered"],
    "the retried backfill must deliver its events",
  );
});

test("backfill retry aborts when the subscription was replaced", async () => {
  resetGate(0);
  const filter = buildChannelFilter("channel-1", 50);
  const subscription = {
    mode: "live",
    filters: [filter],
    onEvent: () => {},
    lastSeenCreatedAt: 1000,
  };
  const subscriptions = new Map([["live-1", subscription]]);

  let historyCalls = 0;
  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    requestHistoryBatch: async () => {
      historyCalls++;
      // Simulate the subscription being torn down while the REQ is in flight.
      subscriptions.delete("live-1");
      throw new Error("rate-limited: quota exceeded; retry in 4s");
    },
  });

  assert.equal(
    historyCalls,
    1,
    "no retry may target a subscription that no longer exists",
  );
});

test("exhausted backfill pins the floor: next replay still requests the original window after live events advance the cursor", async () => {
  // The blocking review scenario on PR #4990: cursor=1000, all backfill
  // attempts fail, a live event at 2100 then advances lastSeenCreatedAt via
  // prepareSubscriptionEvent. Without the pinned floor, the next reconnect
  // would start near 2095 and silently skip 1001..1999.
  resetGate(0);
  const filter = buildChannelFilter("channel-1", 50);
  const subscription = {
    mode: "live",
    filters: [filter],
    onEvent: () => {},
    lastSeenCreatedAt: 1000,
  };
  const subscriptions = new Map([["live-1", subscription]]);

  // Reconnect 1: every backfill attempt is rate-limited.
  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    requestHistoryBatch: async () => {
      throw new Error("rate-limited: quota exceeded; retry in 4s");
    },
  });
  assert.equal(
    subscription.pendingReplaySince,
    995,
    "exhausted backfill must pin the unresolved window's lower bound",
  );

  // A live event arrives through the normal cursor path.
  prepareSubscriptionEvent(subscription, event("live-newer", 2100));
  assert.equal(subscription.lastSeenCreatedAt, 2100);

  // Reconnect 2: backfill now succeeds. It must request the ORIGINAL window.
  const historyFilters = [];
  await replayLiveSubscriptions({
    subscriptions,
    now: 2200,
    sendRaw: async () => {},
    requestHistoryBatch: async ([filter]) => {
      historyFilters.push(filter);
      return [];
    },
  });

  assert.equal(historyFilters.length, 1);
  assert.equal(
    historyFilters[0].since,
    995,
    "replay must start from the pinned floor, not the advanced cursor",
  );
  assert.equal(
    subscription.pendingReplaySince,
    undefined,
    "a completed backfill must clear the pinned floor",
  );

  // Reconnect 3: with the floor cleared, replay returns to the cursor.
  const laterFilters = [];
  await replayLiveSubscriptions({
    subscriptions,
    now: 2300,
    sendRaw: async () => {},
    requestHistoryBatch: async ([filter]) => {
      laterFilters.push(filter);
      return [];
    },
  });
  assert.equal(
    laterFilters[0].since,
    2095,
    "after recovery the cursor governs again",
  );
});

test("in-flight stale abort keeps the pinned floor for the superseding connection", async () => {
  // Race from re-review of b70a6716d/c493d378b: production supersession bumps
  // the connection GENERATION while the same subscription key and object
  // survive in the map. The identity guard alone stays true, so only the
  // combined guard (outer isActive && identity) aborts the stale pass. That
  // abort must NOT count as completion — the pinned floor belongs to the
  // superseding connection's replay.
  resetGate(0);
  const filter = buildChannelFilter("channel-1", 50);
  const subscription = {
    mode: "live",
    filters: [filter],
    onEvent: () => {},
    lastSeenCreatedAt: 1000,
  };
  const subscriptions = new Map([["live-1", subscription]]);

  let generationActive = true;
  let historyCalls = 0;
  await replayLiveSubscriptions({
    subscriptions,
    now: 2000,
    sendRaw: async () => {},
    isActive: () => generationActive,
    requestHistoryBatch: async () => {
      historyCalls++;
      // Connection A is superseded while the REQ is in flight: the generation
      // advances, but the subscription keeps its key AND object identity —
      // exactly what production supersession does.
      generationActive = false;
      // A full page would otherwise continue paging — the post-await
      // combined guard must abort instead.
      return eventRange("full", 1001, 500);
    },
  });

  assert.equal(historyCalls, 1, "stale generation must stop paging");
  assert.equal(
    subscriptions.get("live-1"),
    subscription,
    "precondition: key and object survive supersession untouched",
  );
  assert.equal(
    subscription.pendingReplaySince,
    995,
    "a stale-generation abort must not clear the floor the new connection needs",
  );
});

// ── Teardown ──────────────────────────────────────────────────────────────────

test("teardown — restore Date.now", () => {
  Date.now = origDateNow;
  assert.ok(true);
});
