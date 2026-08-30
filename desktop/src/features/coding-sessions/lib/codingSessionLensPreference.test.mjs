import assert from "node:assert/strict";
import test from "node:test";

import {
  readCodingSessionLensPreference,
  shouldSuggestCodingSessionMission,
  writeCodingSessionLensPreference,
} from "./codingSessionLensPreference.ts";

function memoryStorage() {
  const values = new Map();
  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
  };
}

test("Conversation stays the default and a Mission choice persists per session", () => {
  const storage = memoryStorage();
  const alpha = {
    communityScope: "wss://hive.example/alpha",
    channelId: "engineering",
    sessionKey: "alpha",
    storage,
  };
  const beta = { ...alpha, sessionKey: "beta" };
  assert.equal(readCodingSessionLensPreference(alpha), "conversation");
  writeCodingSessionLensPreference({ ...alpha, lens: "mission" });
  assert.equal(readCodingSessionLensPreference(alpha), "mission");
  assert.equal(readCodingSessionLensPreference(beta), "conversation");
});

test("identical session coordinates never leak between communities", () => {
  const storage = memoryStorage();
  const first = {
    communityScope: "wss://hive.example/first",
    channelId: "engineering",
    sessionKey: "same-session",
    storage,
  };
  const second = {
    ...first,
    communityScope: "wss://hive.example/second",
  };
  writeCodingSessionLensPreference({ ...first, lens: "mission" });
  assert.equal(readCodingSessionLensPreference(first), "mission");
  assert.equal(readCodingSessionLensPreference(second), "conversation");
});

test("team evidence suggests Mission but never changes the selected lens", () => {
  assert.equal(
    shouldSuggestCodingSessionMission({
      currentLens: "conversation",
      participantCount: 1,
      hasTypedTeamTransaction: false,
    }),
    false,
  );
  assert.equal(
    shouldSuggestCodingSessionMission({
      currentLens: "conversation",
      participantCount: 2,
      hasTypedTeamTransaction: false,
    }),
    true,
  );
  assert.equal(
    shouldSuggestCodingSessionMission({
      currentLens: "conversation",
      participantCount: 1,
      hasTypedTeamTransaction: true,
    }),
    true,
  );
  assert.equal(
    shouldSuggestCodingSessionMission({
      currentLens: "mission",
      participantCount: 3,
      hasTypedTeamTransaction: true,
    }),
    false,
  );
});

test("denied storage degrades to Conversation without breaking the session", () => {
  const denied = {
    getItem() {
      throw new Error("denied");
    },
    setItem() {
      throw new Error("denied");
    },
  };
  assert.equal(
    readCodingSessionLensPreference({
      communityScope: "wss://hive.example/alpha",
      channelId: "engineering",
      sessionKey: "alpha",
      storage: denied,
    }),
    "conversation",
  );
  assert.equal(
    writeCodingSessionLensPreference({
      communityScope: "wss://hive.example/alpha",
      channelId: "engineering",
      sessionKey: "alpha",
      lens: "mission",
      storage: denied,
    }),
    false,
  );
});
