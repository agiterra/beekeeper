import assert from "node:assert/strict";
import test from "node:test";

import {
  activityStorageKey,
  absentActivityStorageKey,
  readAbsentThreadActivityIds,
  readActivityFromStorage,
  recordAbsentThreadActivityIds,
  writeActivityToStorage,
} from "@/features/channels/threadActivityStorage";

import { evictRelayScopedCommunityCaches } from "./relayScopedCacheEviction.ts";

// The relay-identity guard calls evictRelayScopedCommunityCaches when a relay
// at a familiar URL comes back signing with a different NIP-11 `self` — i.e.
// it is a new instance and nothing cached under that URL describes reality.
// Thread-activity rows are keyed by relay URL, so if they are missing from the
// bundle the reinstalled relay silently inherits the previous instance's Inbox.

const PUBKEY = "pk1";
const RELAY = "wss://relay.example.com";
const OTHER_RELAY = "wss://other.example.com";

function installStorage() {
  const store = new Map();
  const previousWindow = globalThis.window;
  const previousStorage = globalThis.localStorage;
  const localStorage = {
    get length() {
      return store.size;
    },
    key: (index) => [...store.keys()][index] ?? null,
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => store.set(key, String(value)),
    removeItem: (key) => store.delete(key),
  };
  // Some helpers in the bundle read the bare `localStorage` global rather than
  // `window.localStorage`; both must resolve for the whole bundle to run.
  globalThis.window = { localStorage };
  globalThis.localStorage = localStorage;
  return {
    store,
    restore: () => {
      globalThis.window = previousWindow;
      globalThis.localStorage = previousStorage;
    },
  };
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

test("identity-change eviction clears thread-activity rows and tombstones for the relay", () => {
  const storage = installStorage();
  try {
    writeActivityToStorage(PUBKEY, RELAY, [makeItem("ghost-1")]);
    recordAbsentThreadActivityIds(PUBKEY, RELAY, ["ghost-0"]);
    writeActivityToStorage(PUBKEY, OTHER_RELAY, [makeItem("keep-1")]);

    assert.ok(storage.store.has(activityStorageKey(PUBKEY, RELAY)));
    assert.ok(storage.store.has(absentActivityStorageKey(PUBKEY, RELAY)));

    evictRelayScopedCommunityCaches({
      communityId: "community-1",
      relayUrl: RELAY,
    });

    assert.deepEqual(
      readActivityFromStorage(PUBKEY, RELAY),
      [],
      "rows from the previous relay instance must not survive a re-key",
    );
    assert.equal(readAbsentThreadActivityIds(PUBKEY, RELAY).size, 0);
    assert.equal(storage.store.has(activityStorageKey(PUBKEY, RELAY)), false);

    assert.deepEqual(
      readActivityFromStorage(PUBKEY, OTHER_RELAY).map((item) => item.id),
      ["keep-1"],
      "an unrelated relay's rows must be untouched",
    );
  } finally {
    storage.restore();
  }
});
