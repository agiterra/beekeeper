import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_SETUP_MODE_KEY,
  isCodingSessionSetupMode,
  readCodingSessionSetupMode,
  writeCodingSessionSetupMode,
} from "./codingSessionSetupMode.ts";

function memoryStorage() {
  const map = new Map();
  return {
    getItem: (key) => (map.has(key) ? map.get(key) : null),
    setItem: (key, value) => map.set(key, value),
    map,
  };
}

test("the first ever page opens in Solo", () => {
  assert.equal(readCodingSessionSetupMode(memoryStorage()), "solo");
});

test("the mode round-trips under the versioned key", () => {
  const storage = memoryStorage();
  assert.equal(writeCodingSessionSetupMode("team", storage), true);
  assert.equal(storage.map.get(CODING_SESSION_SETUP_MODE_KEY), "team");
  assert.equal(readCodingSessionSetupMode(storage), "team");
  assert.equal(writeCodingSessionSetupMode("solo", storage), true);
  assert.equal(readCodingSessionSetupMode(storage), "solo");
});

test("a foreign value in storage reads as Solo, never as a third mode", () => {
  const storage = memoryStorage();
  storage.setItem(CODING_SESSION_SETUP_MODE_KEY, "crew");
  assert.equal(readCodingSessionSetupMode(storage), "solo");
  assert.equal(isCodingSessionSetupMode("crew"), false);
  assert.equal(isCodingSessionSetupMode("team"), true);
});

test("unavailable storage reads as Solo and writes as a refused no-op", () => {
  const throwing = {
    getItem: () => {
      throw new Error("SecurityError");
    },
    setItem: () => {
      throw new Error("QuotaExceededError");
    },
  };
  assert.equal(readCodingSessionSetupMode(throwing), "solo");
  assert.equal(writeCodingSessionSetupMode("team", throwing), false);
  // No storage at all is the same answer.
  assert.equal(readCodingSessionSetupMode(undefined), "solo");
});
