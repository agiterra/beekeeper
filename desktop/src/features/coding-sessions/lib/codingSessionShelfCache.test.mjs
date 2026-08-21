import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_CODING_SESSION_SHELF_CACHE_EVENTS,
  codingSessionShelfCacheKey,
  filterShelfCacheEventsToChannels,
  readCodingSessionShelfCache,
  writeCodingSessionShelfCache,
} from "./codingSessionShelfCache.ts";

function withMemoryLocalStorage(run) {
  const store = new Map();
  const original = globalThis.localStorage;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key) => store.get(key) ?? null,
      setItem: (key, value) => store.set(key, String(value)),
      removeItem: (key) => store.delete(key),
    },
  });
  try {
    return run(store);
  } finally {
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: original,
    });
  }
}

function fakeEvent(id, channelId) {
  return {
    id,
    sig: "f".repeat(128),
    kind: 44223,
    pubkey: "a".repeat(64),
    created_at: 1_800_000_000,
    content: "{}",
    tags: [["h", channelId]],
  };
}

test("the key requires both relay and viewer identity", () => {
  assert.equal(codingSessionShelfCacheKey(undefined, "abc"), undefined);
  assert.equal(codingSessionShelfCacheKey("wss://r", undefined), undefined);
  assert.equal(
    codingSessionShelfCacheKey("wss://r", "abc"),
    "buzz.codingSessions.shelf.v1:wss://r:abc",
  );
});

test("events round-trip with a sweepable updatedAt and a hard cap", () => {
  withMemoryLocalStorage((store) => {
    const key = codingSessionShelfCacheKey("wss://r", "abc");
    const events = Array.from(
      { length: MAX_CODING_SESSION_SHELF_CACHE_EVENTS + 10 },
      (_, index) => fakeEvent(`id-${index}`, "channel-1"),
    );
    writeCodingSessionShelfCache(key, events);
    const raw = JSON.parse(store.get(key));
    assert.equal(typeof raw.updatedAt, "number");
    const read = readCodingSessionShelfCache(key);
    assert.equal(read.length, MAX_CODING_SESSION_SHELF_CACHE_EVENTS);
    // The cap keeps the newest slice (retainedShelfEvents orders oldest
    // first, so trimming from the front drops the oldest).
    assert.equal(read[read.length - 1].id, "id-509");
  });
});

test("malformed payloads and entries without signatures read as empty", () => {
  withMemoryLocalStorage((store) => {
    const key = codingSessionShelfCacheKey("wss://r", "abc");
    for (const bad of ["not json", "[]", '{"events": 42}']) {
      store.set(key, bad);
      assert.deepEqual(readCodingSessionShelfCache(key), [], bad);
    }
    store.set(
      key,
      JSON.stringify({ updatedAt: 1, events: [{ id: "x" }, null, "y"] }),
    );
    assert.deepEqual(readCodingSessionShelfCache(key), []);
  });
});

test("channel filtering keeps only in-scope events", () => {
  const events = [
    fakeEvent("a", "channel-1"),
    fakeEvent("b", "channel-2"),
    { ...fakeEvent("c", "channel-1"), tags: [] },
  ];
  assert.deepEqual(
    filterShelfCacheEventsToChannels(events, ["channel-1"]).map((e) => e.id),
    ["a"],
  );
});
