import assert from "node:assert/strict";
import test from "node:test";

import {
  HELD_EVENT_READ_LIMIT,
  INCREMENTAL_FULL_REREAD_MS,
  INCREMENTAL_READ_OVERLAP_SECS,
  heldReadKey,
  incrementalSince,
  mergeEventReads,
  readBundleIncrementally,
} from "./incrementalEventRead.ts";

function event(id, created_at, kind = 44223) {
  return { id, created_at, kind, pubkey: "aa", tags: [], content: "", sig: "" };
}

const DURABLE = { kinds: [44223], "#h": ["c1"], limit: 3 };
const LEASES = { kinds: [24223], "#h": ["c1"], limit: 3 };

function reads() {
  return [
    { filter: DURABLE, heldKey: heldReadKey("test", DURABLE) },
    { filter: LEASES, heldKey: null },
  ];
}

/** A fetcher that answers each filter from `rows`, honouring `since`/`limit`. */
function fakeRelay(rows) {
  const calls = [];
  const fetch = async (filters) => {
    calls.push(filters);
    return filters.flatMap((filter) =>
      rows
        .filter((row) => filter.kinds.includes(row.kind))
        .filter(
          (row) => filter.since === undefined || row.created_at >= filter.since,
        )
        .sort((a, b) => b.created_at - a.created_at)
        .slice(0, filter.limit),
    );
  };
  return { fetch, calls };
}

test("mergeEventReads dedupes by id, orders newest first and caps", () => {
  const merged = mergeEventReads(
    [event("b", 20), event("a", 10)],
    [event("b", 20), event("c", 30)],
    5,
  );
  assert.deepEqual(
    merged.events.map((row) => row.id),
    ["c", "b", "a"],
  );
  assert.equal(merged.reachedLimit, false);

  const capped = mergeEventReads(
    [event("b", 20), event("a", 10)],
    [event("c", 30)],
    2,
  );
  assert.deepEqual(
    capped.events.map((row) => row.id),
    ["c", "b"],
  );
  assert.equal(capped.reachedLimit, true);
});

test("incrementalSince anchors on the earlier of newest row and read start", () => {
  const overlap = INCREMENTAL_READ_OVERLAP_SECS;
  assert.equal(
    incrementalSince({
      events: [event("a", 5_000)],
      readStartedAtSec: 10_000,
      fullReadAtMs: 0,
    }),
    5_000 - overlap,
  );
  // A future-stamped row cannot push `since` past the read's own start.
  assert.equal(
    incrementalSince({
      events: [event("a", 20_000)],
      readStartedAtSec: 10_000,
      fullReadAtMs: 0,
    }),
    10_000 - overlap,
  );
  assert.equal(
    incrementalSince({ events: [], readStartedAtSec: 10, fullReadAtMs: 0 }),
    0,
  );
});

test("a second read is a delta for durable rows and whole for leases", async () => {
  const store = new Map();
  const rows = [event("a", 100_000), event("l", 100_000, 24223)];
  const relay = fakeRelay(rows);
  const nowMs = 100_010_000;

  const first = await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs,
  });
  assert.equal(relay.calls[0][0].since, undefined);
  assert.deepEqual(
    first[0].events.map((row) => row.id),
    ["a"],
  );

  rows.push(event("b", 100_020));
  const second = await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: nowMs + 60_000,
  });
  assert.equal(
    relay.calls[1][0].since,
    100_000 - INCREMENTAL_READ_OVERLAP_SECS,
  );
  assert.equal(relay.calls[1][1].since, undefined);
  assert.deepEqual(
    second[0].events.map((row) => row.id),
    ["b", "a"],
  );
  assert.deepEqual(
    second[1].events.map((row) => row.id),
    ["l"],
  );
  assert.equal(second[0].reachedLimit, false);
});

test("held rows outside the delta window still count toward the limit", async () => {
  const store = new Map();
  const rows = [event("a", 1_000), event("b", 2_000)];
  const relay = fakeRelay(rows);
  await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 10_000_000,
  });
  rows.push(event("c", 9_990));
  const second = await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 10_060_000,
  });
  // Row "a" (t=1000) is older than the delta window; it survives from the hold.
  assert.equal(relay.calls[1][0].since, 2_000 - INCREMENTAL_READ_OVERLAP_SECS);
  assert.deepEqual(
    second[0].events.map((row) => row.id),
    ["c", "b", "a"],
  );
  // Three distinct rows at limit 3: a full read would also have been truncated.
  assert.equal(second[0].reachedLimit, true);
});

test("a delta that fills its limit is re-read in full", async () => {
  const store = new Map();
  const rows = [event("a", 9_000)];
  const relay = fakeRelay(rows);
  await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 10_000_000,
  });
  rows.push(event("b", 9_500), event("c", 9_600), event("d", 9_700));
  const second = await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 10_060_000,
  });
  assert.equal(relay.calls.length, 3);
  assert.deepEqual(relay.calls[2], [DURABLE]);
  assert.deepEqual(
    second[0].events.map((row) => row.id),
    ["d", "c", "b"],
  );
  assert.equal(second[0].reachedLimit, true);
});

test("a held read past the full re-read interval is read whole", async () => {
  const store = new Map();
  const relay = fakeRelay([event("a", 9_000)]);
  await readBundleIncrementally(reads(), relay.fetch, { store, nowMs: 1_000 });
  await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 1_000 + INCREMENTAL_FULL_REREAD_MS - 1,
  });
  assert.notEqual(relay.calls[1][0].since, undefined);
  await readBundleIncrementally(reads(), relay.fetch, {
    store,
    nowMs: 1_000 + INCREMENTAL_FULL_REREAD_MS,
  });
  assert.equal(relay.calls[2][0].since, undefined);
});

test("without a store every read is full", async () => {
  const relay = fakeRelay([event("a", 9_000)]);
  await readBundleIncrementally(reads(), relay.fetch, { nowMs: 1_000 });
  await readBundleIncrementally(reads(), relay.fetch, { nowMs: 2_000 });
  assert.equal(relay.calls[1][0].since, undefined);
});

test("a failed fetch throws and leaves held reads untouched", async () => {
  const store = new Map();
  const relay = fakeRelay([event("a", 9_000)]);
  await readBundleIncrementally(reads(), relay.fetch, { store, nowMs: 1_000 });
  const before = store.get(heldReadKey("test", DURABLE));
  await assert.rejects(
    readBundleIncrementally(
      reads(),
      async () => {
        throw new Error("offline");
      },
      { store, nowMs: 2_000 },
    ),
    /offline/,
  );
  assert.equal(store.get(heldReadKey("test", DURABLE)), before);
});

test("the held store evicts its oldest read past the cap", async () => {
  const store = new Map();
  const relay = fakeRelay([]);
  for (let index = 0; index <= HELD_EVENT_READ_LIMIT; index += 1) {
    const filter = { kinds: [44223], "#h": [`c${index}`], limit: 3 };
    await readBundleIncrementally(
      [{ filter, heldKey: heldReadKey("test", filter) }],
      relay.fetch,
      { store, nowMs: 1_000 },
    );
  }
  assert.equal(store.size, HELD_EVENT_READ_LIMIT);
  assert.equal(
    store.has(heldReadKey("test", { kinds: [44223], "#h": ["c0"], limit: 3 })),
    false,
  );
});
