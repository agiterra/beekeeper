import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSurfaceSeenKey,
  parseCodingSessionDiffSeenMarker,
  readCodingSessionSurfaceSeenRaw,
  writeCodingSessionDiffSeenMarker,
} from "./codingSessionSurfaceSeen.ts";

function memoryStorage() {
  const values = new Map();
  return {
    values,
    getItem: (key) => (values.has(key) ? values.get(key) : null),
    setItem: (key, value) => values.set(key, String(value)),
  };
}

test("the key separates relay, channel, session and surface", () => {
  const base = {
    relayUrl: "wss://relay.one",
    channelId: "c1",
    sessionKey: "s1",
    surfaceId: "diff",
  };
  const keys = new Set([
    codingSessionSurfaceSeenKey(base),
    codingSessionSurfaceSeenKey({ ...base, relayUrl: "wss://relay.two" }),
    codingSessionSurfaceSeenKey({ ...base, channelId: "c2" }),
    codingSessionSurfaceSeenKey({ ...base, sessionKey: "s2" }),
    codingSessionSurfaceSeenKey({ ...base, surfaceId: "plan" }),
  ]);
  assert.equal(keys.size, 5);
  // A separator inside a value cannot forge another session's key.
  assert.notEqual(
    codingSessionSurfaceSeenKey({ ...base, channelId: "a:b", sessionKey: "c" }),
    codingSessionSurfaceSeenKey({ ...base, channelId: "a", sessionKey: "b:c" }),
  );
});

test("a written marker reads back exactly", () => {
  const storage = memoryStorage();
  const marker = { via: "opened", atMs: 42, seen: { "src/a.ts": "e1" } };
  assert.equal(writeCodingSessionDiffSeenMarker(storage, "k", marker), true);
  assert.deepEqual(
    parseCodingSessionDiffSeenMarker(
      readCodingSessionSurfaceSeenRaw(storage, "k"),
    ),
    marker,
  );
});

test("a malformed marker reads as none, which counts nothing", () => {
  for (const raw of [
    null,
    "not json",
    "null",
    "[]",
    JSON.stringify({ via: "later", atMs: 1, seen: {} }),
    JSON.stringify({ via: "opened", atMs: "1", seen: {} }),
    JSON.stringify({ via: "opened", atMs: 1, seen: [] }),
    JSON.stringify({ via: "opened", atMs: 1, seen: { a: 3 } }),
  ]) {
    assert.equal(parseCodingSessionDiffSeenMarker(raw), null, String(raw));
  }
});

test("a storage that refuses leaves the old marker and says so", () => {
  const refusing = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("quota");
    },
  };
  assert.equal(readCodingSessionSurfaceSeenRaw(refusing, "k"), null);
  assert.equal(
    writeCodingSessionDiffSeenMarker(refusing, "k", {
      via: "baseline",
      atMs: 1,
      seen: {},
    }),
    false,
  );
  assert.equal(readCodingSessionSurfaceSeenRaw(null, "k"), null);
  assert.equal(
    writeCodingSessionDiffSeenMarker(null, "k", {
      via: "baseline",
      atMs: 1,
      seen: {},
    }),
    false,
  );
});
