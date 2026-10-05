import assert from "node:assert/strict";
import test from "node:test";

import {
  advanceCodingSessionLastSeen,
  codingSessionLastSeenStorageKey,
  CODING_SESSION_LAST_SEEN_LABEL,
  readCodingSessionLastSeen,
  resolveCodingSessionLastSeenIndex,
  writeCodingSessionLastSeen,
} from "./codingSessionLastSeen.ts";

const scope = {
  relayUrl: "wss://hive.example",
  channelId: "chan-1",
  sessionKey: "session-1",
};

function memoryStorage() {
  const values = new Map();
  return {
    values,
    getItem: (key) => (values.has(key) ? values.get(key) : null),
    setItem: (key, value) => values.set(key, String(value)),
  };
}

test("the key carries the community, the channel and the session", () => {
  assert.equal(
    codingSessionLastSeenStorageKey(scope),
    "beekeeper:session-last-seen:v1:wss://hive.example:chan-1:session-1",
  );
  assert.equal(CODING_SESSION_LAST_SEEN_LABEL, "on this device");
});

test("write then read round-trips; garbage and absence read as none", () => {
  const storage = memoryStorage();
  assert.equal(readCodingSessionLastSeen(scope, storage), null);
  writeCodingSessionLastSeen(scope, "p3", storage, 42);
  assert.equal(readCodingSessionLastSeen(scope, storage), "p3");
  assert.deepEqual(
    JSON.parse(storage.values.get(codingSessionLastSeenStorageKey(scope))),
    { itemId: "p3", at: 42 },
  );
  storage.setItem(codingSessionLastSeenStorageKey(scope), "{nope");
  assert.equal(readCodingSessionLastSeen(scope, storage), null);
  assert.equal(readCodingSessionLastSeen(scope, null), null);
  // Another session on the same channel is a different marker.
  assert.equal(
    readCodingSessionLastSeen({ ...scope, sessionKey: "other" }, storage),
    null,
  );
});

test("a refusing store never throws", () => {
  const storage = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("full");
    },
  };
  assert.equal(readCodingSessionLastSeen(scope, storage), null);
  writeCodingSessionLastSeen(scope, "p1", storage);
});

test("the rule follows the last seen turn, only when newer turns exist", () => {
  const ids = ["p1", "p2", "p3", "p4"];
  assert.equal(resolveCodingSessionLastSeenIndex(ids, "p2"), 1);
  // Seen everything: nothing new, no rule.
  assert.equal(resolveCodingSessionLastSeenIndex(ids, "p4"), null);
  // First visit, or a marker this transcript does not hold: no rule.
  assert.equal(resolveCodingSessionLastSeenIndex(ids, null), null);
  assert.equal(resolveCodingSessionLastSeenIndex(ids, "gone"), null);
});

test("the marker advances to the newest turn on screen, never back", () => {
  const ids = ["p1", "p2", "p3", "p4"];
  assert.equal(
    advanceCodingSessionLastSeen(ids, null, new Set(["p1", "p2"])),
    "p2",
  );
  assert.equal(advanceCodingSessionLastSeen(ids, "p3", new Set(["p1"])), null);
  assert.equal(advanceCodingSessionLastSeen(ids, "p3", new Set(["p3"])), null);
  assert.equal(
    advanceCodingSessionLastSeen(ids, "gone", new Set(["p1"])),
    "p1",
  );
  assert.equal(advanceCodingSessionLastSeen(ids, null, new Set()), null);
});
