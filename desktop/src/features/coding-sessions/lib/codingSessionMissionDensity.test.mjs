import assert from "node:assert/strict";
import test from "node:test";

import {
  readCodingSessionMissionDensity,
  writeCodingSessionMissionDensity,
} from "./codingSessionMissionDensity.ts";

function memoryStorage() {
  const values = new Map();
  return {
    values,
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
  };
}

const coordinates = {
  communityScope: "wss://relay.example",
  channelId: "channel",
  sessionKey: "session",
};

test("Mission density defaults to Live and persists without an execution key", () => {
  const storage = memoryStorage();
  assert.equal(
    readCodingSessionMissionDensity({ coordinates, storage }),
    "live",
  );
  writeCodingSessionMissionDensity({
    coordinates,
    density: "trace",
    storage,
  });
  assert.equal(
    readCodingSessionMissionDensity({ coordinates, storage }),
    "trace",
  );
  assert.equal([...storage.values.keys()][0].includes("execution"), false);
});

test("the same session coordinates never leak between communities", () => {
  const storage = memoryStorage();
  writeCodingSessionMissionDensity({
    coordinates,
    density: "brief",
    storage,
  });
  assert.equal(
    readCodingSessionMissionDensity({
      coordinates: { ...coordinates, communityScope: "wss://other.example" },
      storage,
    }),
    "live",
  );
});

test("invalid persisted values fail back to Live", () => {
  const storage = memoryStorage();
  storage.setItem(
    "buzz:coding-session-mission-density:v1:wss%3A%2F%2Frelay.example:channel:session",
    "running",
  );
  assert.equal(
    readCodingSessionMissionDensity({ coordinates, storage }),
    "live",
  );
});
