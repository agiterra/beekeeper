import assert from "node:assert/strict";
import test from "node:test";

import {
  absentActivityStorageKey,
  activityStorageKey,
  findAbsentThreadActivityIds,
  readAbsentThreadActivityIds,
  readActivityFromStorage,
  recordAbsentThreadActivityIds,
  removeThreadActivityForRelay,
  writeActivityToStorage,
} from "./threadActivityStorage.ts";

// Full localStorage stand-in: unlike the minimal mock in
// useUnreadChannels.storage.test.mjs this one implements `length` + `key(i)`,
// which the relay-scoped remover iterates.
function installStorage() {
  const store = new Map();
  const localStorage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => store.set(key, String(value)),
    removeItem: (key) => store.delete(key),
    get length() {
      return store.size;
    },
    key: (index) => [...store.keys()][index] ?? null,
  };
  const previous = globalThis.window;
  globalThis.window = { ...(previous ?? {}), localStorage };
  return {
    store,
    restore: () => {
      globalThis.window = previous;
    },
  };
}

function makeItem(id, { kind = 9, createdAt = 1_700_000_000 } = {}) {
  return {
    id,
    kind,
    pubkey: "author",
    content: "reply",
    createdAt,
    channelId: "channel-1",
    channelName: "general",
    tags: [
      ["h", "channel-1"],
      ["e", "root-1", "", "root"],
    ],
  };
}

const PUBKEY = "pk1";
const RELAY = "wss://relay.example.com";

// ── findAbsentThreadActivityIds: the fail-closed contract ────────────────────

test("reconcile drops an item the relay completed a query for and did not return", async () => {
  const items = [makeItem("ghost-1"), makeItem("live-1")];
  const result = await findAbsentThreadActivityIds(items, async ({ ids }) =>
    ids.filter((id) => id === "live-1").map((id) => ({ id })),
  );

  assert.deepEqual(result.absentIds, ["ghost-1"]);
  assert.equal(result.complete, true);
});

test("reconcile evicts NOTHING when the relay is unreachable (probe rejects)", async () => {
  const items = [makeItem("ghost-1"), makeItem("ghost-2")];
  let calls = 0;
  const result = await findAbsentThreadActivityIds(items, async () => {
    calls += 1;
    throw new Error("Relay unreachable.");
  });

  assert.equal(calls, 1);
  assert.deepEqual(
    result.absentIds,
    [],
    "a relay that never answered must not condemn a single row",
  );
  assert.equal(result.complete, false);
});

test("reconcile evicts NOTHING when the relay times out mid-pass, and keeps the ids it already judged", async () => {
  // 30 items over a chunk size of 2: chunk 1 completes, chunk 2 rejects.
  const items = Array.from({ length: 4 }, (_, i) => makeItem(`id-${i}`));
  let call = 0;
  const result = await findAbsentThreadActivityIds(
    items,
    async () => {
      call += 1;
      if (call === 1) return []; // authoritative: neither id-0 nor id-1 exists
      throw new Error("Timed out while loading channel history.");
    },
    2,
  );

  assert.deepEqual(result.absentIds, ["id-0", "id-1"]);
  assert.equal(
    result.complete,
    false,
    "the un-probed chunk must be reported as unjudged",
  );
});

test("reconcile evicts NOTHING when the relay returns a non-array (rate-limit / CLOSED shim)", async () => {
  const items = [makeItem("ghost-1")];
  const result = await findAbsentThreadActivityIds(items, async () => null);

  assert.deepEqual(result.absentIds, []);
  assert.equal(result.complete, false);
});

test("reconcile refuses to judge a chunk it cannot query with explicit kinds", async () => {
  // The relay's p-gate rejects kind-less filters, so an unqueryable chunk must
  // abort rather than resolve to a spurious "not found".
  const items = [{ ...makeItem("no-kind"), kind: undefined }];
  let probed = false;
  const result = await findAbsentThreadActivityIds(items, async () => {
    probed = true;
    return [];
  });

  assert.equal(probed, false);
  assert.deepEqual(result.absentIds, []);
  assert.equal(result.complete, false);
});

test("reconcile queries with the ids' own kinds and never more than chunkSize ids", async () => {
  const items = [
    makeItem("a", { kind: 9 }),
    makeItem("b", { kind: 40002 }),
    makeItem("c", { kind: 9 }),
  ];
  const filters = [];
  await findAbsentThreadActivityIds(
    items,
    async ({ ids, kinds }) => {
      filters.push({ ids, kinds });
      return ids.map((id) => ({ id }));
    },
    2,
  );

  assert.equal(filters.length, 2);
  assert.deepEqual(filters[0].ids, ["a", "b"]);
  assert.deepEqual(filters[0].kinds, [9, 40002]);
  assert.deepEqual(filters[1].ids, ["c"]);
  assert.deepEqual(filters[1].kinds, [9]);
});

test("reconcile is a no-op on an empty store and issues no relay traffic", async () => {
  let probed = false;
  const result = await findAbsentThreadActivityIds([], async () => {
    probed = true;
    return [];
  });

  assert.equal(probed, false);
  assert.deepEqual(result, { absentIds: [], complete: true });
});

// ── tombstones survive into storage ──────────────────────────────────────────

test("a confirmed-absent id is filtered out of the next hydration", async () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [
      makeItem("ghost-1"),
      makeItem("live-1"),
    ]);

    const { absentIds } = await findAbsentThreadActivityIds(
      readActivityFromStorage(PUBKEY, RELAY),
      async ({ ids }) =>
        ids.filter((id) => id === "live-1").map((id) => ({ id })),
    );
    recordAbsentThreadActivityIds(PUBKEY, RELAY, absentIds);

    assert.deepEqual(
      readActivityFromStorage(PUBKEY, RELAY).map((item) => item.id),
      ["live-1"],
    );
    assert.ok(readAbsentThreadActivityIds(PUBKEY, RELAY).has("ghost-1"));
  } finally {
    storage.restore();
  }
});

test("a later write from the in-memory buffer cannot resurrect a tombstoned row", () => {
  const storage = installStorage();
  try {
    const buffer = [makeItem("ghost-1"), makeItem("live-1")];
    writeActivityToStorage(PUBKEY, RELAY, buffer);
    recordAbsentThreadActivityIds(PUBKEY, RELAY, ["ghost-1"]);

    // The buffer belongs to useUnreadChannels and is never pruned in place, so
    // the next coalesced flush still contains the ghost.
    writeActivityToStorage(PUBKEY, RELAY, buffer);

    assert.deepEqual(
      readActivityFromStorage(PUBKEY, RELAY).map((item) => item.id),
      ["live-1"],
    );
  } finally {
    storage.restore();
  }
});

test("recording absences prunes the stored blob without resetting its TTL clock", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [
      makeItem("ghost-1"),
      makeItem("live-1"),
    ]);
    const before = JSON.parse(
      storage.store.get(activityStorageKey(PUBKEY, RELAY)),
    );

    recordAbsentThreadActivityIds(PUBKEY, RELAY, ["ghost-1"]);
    const after = JSON.parse(
      storage.store.get(activityStorageKey(PUBKEY, RELAY)),
    );

    assert.deepEqual(
      after.items.map((item) => item.id),
      ["live-1"],
    );
    assert.equal(
      after.updatedAt,
      before.updatedAt,
      "pruning dead rows is not fresh activity and must not refresh the age",
    );
  } finally {
    storage.restore();
  }
});

test("recording an empty absence list touches nothing", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("live-1")]);
    recordAbsentThreadActivityIds(PUBKEY, RELAY, []);

    assert.equal(
      storage.store.has(absentActivityStorageKey(PUBKEY, RELAY)),
      false,
    );
    assert.equal(readActivityFromStorage(PUBKEY, RELAY).length, 1);
  } finally {
    storage.restore();
  }
});

// ── persisted envelope + legacy migration ────────────────────────────────────

test("writes carry a root updatedAt so the localStorage sweep can age the key", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("live-1")]);
    const parsed = JSON.parse(
      storage.store.get(activityStorageKey(PUBKEY, RELAY)),
    );

    assert.equal(typeof parsed.updatedAt, "number");
    assert.ok(Array.isArray(parsed.items));
  } finally {
    storage.restore();
  }
});

test("a legacy bare-array blob is migrated in place and dated by its newest row", () => {
  const storage = installStorage();
  try {
    const july = 1_753_000_000; // unix seconds
    storage.store.set(
      activityStorageKey(PUBKEY, RELAY),
      JSON.stringify([
        makeItem("old-1", { createdAt: july - 86_400 }),
        makeItem("old-2", { createdAt: july }),
      ]),
    );

    const read = readActivityFromStorage(PUBKEY, RELAY);
    assert.deepEqual(
      read.map((item) => item.id),
      ["old-1", "old-2"],
    );

    const migrated = JSON.parse(
      storage.store.get(activityStorageKey(PUBKEY, RELAY)),
    );
    assert.equal(
      migrated.updatedAt,
      july * 1_000,
      "migration must preserve the real age, not stamp it fresh",
    );
  } finally {
    storage.restore();
  }
});

// ── relay-identity eviction ──────────────────────────────────────────────────

test("relay-scoped removal clears rows and tombstones for that relay only", () => {
  const storage = installStorage();
  try {
    const other = "wss://other.example.com";
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("a")]);
    writeActivityToStorage("pk2", RELAY, [makeItem("b")]);
    writeActivityToStorage(PUBKEY, other, [makeItem("c")]);
    recordAbsentThreadActivityIds(PUBKEY, RELAY, ["ghost-1"]);
    recordAbsentThreadActivityIds(PUBKEY, other, ["ghost-2"]);

    removeThreadActivityForRelay(RELAY);

    assert.deepEqual(readActivityFromStorage(PUBKEY, RELAY), []);
    assert.deepEqual(readActivityFromStorage("pk2", RELAY), []);
    assert.equal(readAbsentThreadActivityIds(PUBKEY, RELAY).size, 0);

    assert.deepEqual(
      readActivityFromStorage(PUBKEY, other).map((item) => item.id),
      ["c"],
      "a different relay's rows must survive",
    );
    assert.ok(readAbsentThreadActivityIds(PUBKEY, other).has("ghost-2"));
  } finally {
    storage.restore();
  }
});

test("relay-scoped removal normalizes the relay URL", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("a")]);
    removeThreadActivityForRelay("WSS://Relay.Example.Com/");

    assert.deepEqual(readActivityFromStorage(PUBKEY, RELAY), []);
  } finally {
    storage.restore();
  }
});
