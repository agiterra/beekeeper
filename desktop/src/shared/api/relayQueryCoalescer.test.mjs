import assert from "node:assert/strict";
import test from "node:test";

import {
  chunkFiltersForRequest,
  demuxForFilter,
  executeQueryBatch,
  RelayQueryCoalescer,
  splitOversizedChannelFilter,
  unionQueryResults,
} from "./relayQueryCoalescer.ts";

function event(id, createdAt, kind, channel, extraTags = []) {
  return {
    id,
    pubkey: "pubkey",
    created_at: createdAt,
    kind,
    tags: [...(channel ? [["h", channel]] : []), ...extraTags],
    content: "",
    sig: "sig",
  };
}

const channels = (n, prefix = "ch") =>
  Array.from({ length: n }, (_, i) => `${prefix}-${i}`);

// ── Chunking ──────────────────────────────────────────────────────────────────

test("chunkFiltersForRequest keeps aggregate #h at or under the cap", () => {
  const items = [
    { filter: { kinds: [9], "#h": channels(100), limit: 10 } },
    { filter: { kinds: [9], "#h": channels(20, "b"), limit: 10 } },
    { filter: { kinds: [9], "#h": channels(10, "c"), limit: 10 } },
    { filter: { kinds: [9], limit: 10 } },
  ];
  const chunks = chunkFiltersForRequest(items, { maxChannelValues: 128 });
  assert.deepEqual(
    chunks.map((chunk) => chunk.map((item) => item.filter["#h"]?.length ?? 0)),
    [
      [100, 20],
      [10, 0],
    ],
  );
});

test("chunkFiltersForRequest honours a per-request filter cap", () => {
  const items = Array.from({ length: 23 }, () => ({
    filter: { kinds: [9], limit: 1 },
  }));
  const chunks = chunkFiltersForRequest(items, { maxFilters: 10 });
  assert.deepEqual(
    chunks.map((chunk) => chunk.length),
    [10, 10, 3],
  );
});

test("splitOversizedChannelFilter splits 130 channels into 128 + 2", () => {
  const pieces = splitOversizedChannelFilter({
    kinds: [9],
    "#h": channels(130),
    limit: 50,
  });
  assert.deepEqual(
    pieces.map((piece) => piece["#h"].length),
    [128, 2],
  );
  assert.equal(pieces[0].limit, 50);
});

// ── Demux ─────────────────────────────────────────────────────────────────────

test("demuxForFilter keeps matches only, dedupes, truncates to the newest limit, oldest-first", () => {
  const events = [
    event("a", 100, 9, "ch-1"),
    event("b", 300, 9, "ch-1"),
    event("b", 300, 9, "ch-1"),
    event("c", 200, 9, "ch-1"),
    event("d", 400, 9, "ch-2"),
    event("e", 500, 1, "ch-1"),
  ];
  assert.deepEqual(
    demuxForFilter(events, { kinds: [9], "#h": ["ch-1"], limit: 2 }).map(
      (e) => e.id,
    ),
    ["c", "b"],
  );
  assert.deepEqual(
    demuxForFilter(events, { kinds: [9], "#h": ["ch-1"], limit: 100 }).map(
      (e) => e.id,
    ),
    ["a", "c", "b"],
  );
});

test("demuxForFilter delivers an h-less reaction to a #h filter (relay channel_id fallback)", () => {
  const reaction = event("r", 100, 7, null, [["e", "target"]]);
  assert.deepEqual(
    demuxForFilter([reaction], { kinds: [7], "#h": ["ch-1"], limit: 10 }),
    [reaction],
  );
});

// ── executeQueryBatch ─────────────────────────────────────────────────────────

test("three callers in one batch share one query body and each gets only its matches", async () => {
  const bodies = [];
  const results = await executeQueryBatch(
    [
      { kinds: [9], "#h": ["ch-1"], limit: 10 },
      { kinds: [9], "#h": ["ch-2"], limit: 10 },
      { kinds: [40002], "#p": ["me"], limit: 10 },
    ],
    {
      query: async (filters) => {
        bodies.push(filters);
        return [
          event("one", 1, 9, "ch-1"),
          event("two", 2, 9, "ch-2"),
          event("three", 3, 40002, "ch-3", [["p", "me"]]),
        ];
      },
      fallback: async () => {
        throw new Error("fallback must not run");
      },
    },
  );
  assert.equal(bodies.length, 1, "one POST /query");
  assert.equal(bodies[0].length, 3, "three filters in the body");
  assert.deepEqual(
    results.map((r) => r.events.map((e) => e.id)),
    [["one"], ["two"], ["three"]],
  );
});

test("130 aggregate #h across filters produce two query calls", async () => {
  const bodies = [];
  await executeQueryBatch(
    [
      { kinds: [9], "#h": channels(100), limit: 10 },
      { kinds: [9], "#h": channels(30, "b"), limit: 10 },
    ],
    {
      query: async (filters) => {
        bodies.push(filters.length);
        return [];
      },
      fallback: async () => [],
    },
  );
  assert.deepEqual(bodies, [1, 1]);
});

test("a single filter wider than the cap is split, and its union is re-limited", async () => {
  const bodies = [];
  const results = await executeQueryBatch(
    [{ kinds: [9], "#h": channels(130), limit: 2 }],
    {
      query: async (filters) => {
        bodies.push(filters[0]["#h"].length);
        return filters[0]["#h"].length === 128
          ? [event("x1", 10, 9, "ch-0"), event("x2", 20, 9, "ch-1")]
          : [event("y1", 30, 9, "ch-128")];
      },
      fallback: async () => [],
    },
  );
  assert.deepEqual(bodies, [128, 2]);
  assert.deepEqual(
    results[0].events.map((e) => e.id),
    ["x2", "y1"],
    "newest two of the union",
  );
});

test("a search filter bypasses the query body and takes the fallback alone", async () => {
  const bodies = [];
  const fallbacks = [];
  const results = await executeQueryBatch(
    [
      { kinds: [9], limit: 5, search: "hello" },
      { kinds: [9], "#h": ["ch-1"], limit: 5 },
    ],
    {
      query: async (filters) => {
        bodies.push(filters);
        return [];
      },
      fallback: async (filter) => {
        fallbacks.push(filter);
        return [event("hit", 1, 9, "ch-1")];
      },
    },
  );
  assert.equal(bodies.length, 1);
  assert.equal(bodies[0].length, 1, "only the matchable filter was bundled");
  assert.deepEqual(fallbacks, [{ kinds: [9], limit: 5, search: "hello" }]);
  assert.deepEqual(
    results[0].events.map((e) => e.id),
    ["hit"],
  );
  assert.deepEqual(results[1].events, []);
});

test("limit 0 resolves empty without any request", async () => {
  let queries = 0;
  const results = await executeQueryBatch([{ kinds: [9], limit: 0 }], {
    query: async () => {
      queries++;
      return [];
    },
    fallback: async () => {
      queries++;
      return [];
    },
  });
  assert.equal(queries, 0);
  assert.deepEqual(results, [{ ok: true, events: [] }]);
});

test("an HTTP failure (429) falls back to one WS read per filter in the chunk", async () => {
  const fallbacks = [];
  const results = await executeQueryBatch(
    [
      { kinds: [9], "#h": ["ch-1"], limit: 5 },
      { kinds: [9], "#h": ["ch-2"], limit: 5 },
    ],
    {
      query: async () => {
        throw new Error("relay rate-limited: retry in 4s");
      },
      fallback: async (filter) => {
        fallbacks.push(filter["#h"][0]);
        if (filter["#h"][0] === "ch-2") throw new Error("ws failed too");
        return [event("f1", 1, 9, "ch-1")];
      },
    },
  );
  assert.deepEqual(fallbacks, ["ch-1", "ch-2"]);
  assert.deepEqual(results[0], {
    ok: true,
    events: [event("f1", 1, 9, "ch-1")],
  });
  assert.equal(results[1].ok, false);
  assert.match(results[1].error.message, /ws failed too/);
  assert.throws(() => unionQueryResults(results), /ws failed too/);
  assert.deepEqual(
    unionQueryResults([results[0], results[0]]).map((e) => e.id),
    ["f1"],
    "the union dedupes",
  );
});

// ── Coalescer window ──────────────────────────────────────────────────────────

test("reads enqueued inside one window run as one batch; the next window is separate", async () => {
  const timers = [];
  const batches = [];
  const coalescer = new RelayQueryCoalescer({
    windowMs: 50,
    setTimeoutFn: (fn, ms) => {
      timers.push({ fn, ms });
      return timers.length;
    },
    clearTimeoutFn: () => {},
    execute: async (filters) => {
      batches.push(filters);
      return filters.map((filter) => ({
        ok: true,
        events: [event(filter["#h"][0], 1, 9, filter["#h"][0])],
      }));
    },
  });

  const a = coalescer.enqueue({ kinds: [9], "#h": ["a"], limit: 1 });
  const b = coalescer.enqueue({ kinds: [9], "#h": ["b"], limit: 1 });
  assert.equal(timers.length, 1, "one window timer");
  assert.equal(timers[0].ms, 50);
  assert.equal(coalescer.pendingCount(), 2);
  timers[0].fn();
  assert.deepEqual(
    (await a).map((e) => e.id),
    ["a"],
  );
  assert.deepEqual(
    (await b).map((e) => e.id),
    ["b"],
  );

  const c = coalescer.enqueue({ kinds: [9], "#h": ["c"], limit: 1 });
  assert.equal(timers.length, 2, "a new window after the flush");
  timers[1].fn();
  assert.deepEqual(
    (await c).map((e) => e.id),
    ["c"],
  );
  assert.equal(batches.length, 2);
});

test("reset rejects every queued read", async () => {
  const coalescer = new RelayQueryCoalescer({
    setTimeoutFn: () => 1,
    clearTimeoutFn: () => {},
    execute: async () => [],
  });
  const pending = coalescer.enqueue({ kinds: [9], limit: 1 });
  coalescer.reset(new Error("community switch"));
  await assert.rejects(pending, /community switch/);
  assert.equal(coalescer.pendingCount(), 0);
});

test("an h-less event never leaks across callers with different channels", async () => {
  // The relay resolves a reaction with no `h` tag to its stored channel and
  // would return it to exactly one of these filters; the client cannot tell
  // which, so a mixed-scope chunk demuxes strictly and drops it.
  const reaction = {
    id: "r1",
    kind: 7,
    pubkey: "p",
    created_at: 5,
    tags: [],
    content: "+",
  };
  const results = await executeQueryBatch(
    [
      { kinds: [7], "#h": ["a"], limit: 10 },
      { kinds: [7], "#h": ["b"], limit: 10 },
    ],
    { query: async () => [reaction], fallback: async () => [] },
  );
  assert.deepEqual(
    results.map((r) => r.ok && r.events.length),
    [0, 0],
  );
  // The same filters over one channel set keep the permissive read.
  const same = await executeQueryBatch(
    [
      { kinds: [7], "#h": ["a"], limit: 10 },
      { kinds: [9], "#h": ["a"], limit: 10 },
    ],
    { query: async () => [reaction], fallback: async () => [] },
  );
  assert.deepEqual(
    same.map((r) => r.ok && r.events.length),
    [1, 0],
  );
});
