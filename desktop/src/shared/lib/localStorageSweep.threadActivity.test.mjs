import assert from "node:assert/strict";
import test from "node:test";

import {
  activityStorageKey,
  writeActivityToStorage,
} from "@/features/channels/threadActivityStorage";

import {
  LOCAL_STORAGE_SWEEP_RULES,
  sweepStaleLocalStorage,
} from "./localStorageSweep.ts";

// End-to-end check that the *real* payload thread-activity writes is sweepable.
// The generic rule test only proves the rule matches a synthetic
// `{ updatedAt }` object; this proves the shipped writer produces one. Before
// the envelope change the writer emitted a bare JSON array, whose root carries
// no `updatedAt` — the sweep's parser returns null for that and treats the
// entry as un-ageable, so a TTL rule alone would have collected nothing.

const DAY_MS = 24 * 60 * 60 * 1_000;
const PUBKEY = "pk1";
const RELAY = "wss://relay.example.com";

function installStorage() {
  const store = new Map();
  const previous = globalThis.window;
  globalThis.window = {
    localStorage: {
      store,
      get length() {
        return store.size;
      },
      key: (index) => [...store.keys()][index] ?? null,
      getItem: (key) => store.get(key) ?? null,
      setItem: (key, value) => store.set(key, String(value)),
      removeItem: (key) => store.delete(key),
    },
  };
  return { store, restore: () => (globalThis.window = previous) };
}

function makeItem(id) {
  return {
    id,
    kind: 9,
    pubkey: "author",
    content: "reply",
    createdAt: 1_700_000_000,
    channelId: "channel-1",
    channelName: "general",
    tags: [["h", "channel-1"]],
  };
}

test("thread-activity rows and tombstones are registered sweep namespaces", () => {
  const prefixes = LOCAL_STORAGE_SWEEP_RULES.map((rule) => rule.keyPrefix);
  assert.ok(prefixes.includes("buzz-thread-activity.v1:"));
  assert.ok(prefixes.includes("buzz-thread-activity-absent.v1:"));

  const rows = LOCAL_STORAGE_SWEEP_RULES.find(
    (rule) => rule.keyPrefix === "buzz-thread-activity.v1:",
  );
  const tombstones = LOCAL_STORAGE_SWEEP_RULES.find(
    (rule) => rule.keyPrefix === "buzz-thread-activity-absent.v1:",
  );
  assert.equal(rows.maxAgeMs, 14 * DAY_MS);
  assert.ok(
    tombstones.maxAgeMs > rows.maxAgeMs,
    "tombstones must outlive the rows they censor, or an expired tombstone " +
      "lets a long-lived session write the ghost rows back",
  );
});

test("a thread-activity blob written 14 days ago is swept; a fresher one is kept", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("row-1")]);
    const key = activityStorageKey(PUBKEY, RELAY);
    const written = JSON.parse(storage.store.get(key));
    assert.equal(
      typeof written.updatedAt,
      "number",
      "the writer must stamp a root updatedAt or the sweep cannot age it",
    );

    // Fresh: one hour past the write.
    assert.equal(sweepStaleLocalStorage(written.updatedAt + 3_600_000), 0);
    assert.ok(storage.store.has(key));

    // Over-age: exactly the TTL later.
    assert.equal(sweepStaleLocalStorage(written.updatedAt + 14 * DAY_MS), 1);
    assert.equal(storage.store.has(key), false);
  } finally {
    storage.restore();
  }
});

test("a legacy bare-array blob is un-sweepable until it is migrated on read", async () => {
  const storage = installStorage();
  try {
    const key = activityStorageKey(PUBKEY, RELAY);
    storage.store.set(key, JSON.stringify([makeItem("row-1")]));

    // Bare array: no root updatedAt, so the sweep leaves it alone forever.
    assert.equal(sweepStaleLocalStorage(Date.now()), 0);
    assert.ok(storage.store.has(key));

    // Hydration migrates it to the envelope form, dated by its newest row
    // (1_700_000_000s → Nov 2023), which is instantly over-age.
    const { readActivityFromStorage } = await import(
      "@/features/channels/threadActivityStorage"
    );
    readActivityFromStorage(PUBKEY, RELAY);

    assert.equal(sweepStaleLocalStorage(Date.now()), 1);
    assert.equal(storage.store.has(key), false);
  } finally {
    storage.restore();
  }
});
