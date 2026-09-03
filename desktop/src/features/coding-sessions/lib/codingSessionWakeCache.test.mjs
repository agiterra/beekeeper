import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";

import {
  buildCodingSessionWakeOperationIndex,
  codingSessionWakeOperationScopeKey,
  codingSessionWakeReadingForText,
  readCachedCodingSessionWakeOperations,
  rememberCodingSessionWakeOperations,
  resetCodingSessionWakeOperations,
} from "./codingSessionWakeReading.ts";

// L5.4. REPORT-L2 §6c left Conversation's wake line unresolved because that
// lens subscribes to no fold, and REVIEW-L2 advised against giving it one.
// The ruling: resolve from what Mission already folded, never fetch.

const CHANNEL = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const SESSION = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const GENESIS = "ce".repeat(32);
const FOUNDER = "3d".repeat(32);
const AUTHOR = "11".repeat(32);
const ASSIGNMENT_ID =
  "3234382f094519e81c1a572ce1383516fe093b0a6b179100525d6f47909e2a9a";
const ABSENT_ID = "0".repeat(64);

const SCOPE = {
  channelRef: CHANNEL,
  sessionRef: SESSION,
  genesisRef: GENESIS,
  founderPubkey: FOUNDER,
};

const WAKE = JSON.stringify({
  operationId: ASSIGNMENT_ID,
  type: "assignment",
});

function missionIndex() {
  return buildCodingSessionWakeOperationIndex({
    assignments: [
      {
        sourceEventId: ASSIGNMENT_ID,
        assigneeRole: "builder",
        objective: "Render kind 44246 in the Audit tab",
      },
    ],
    transactions: [
      {
        sourceEventId: ASSIGNMENT_ID,
        type: "assignment",
        authorPubkey: AUTHOR,
        parentEventId: null,
        summary: "Render kind 44246 in the Audit tab",
      },
    ],
  });
}

function read(operations) {
  return codingSessionWakeReadingForText({
    operations,
    signerPubkey: AUTHOR,
    text: WAKE,
    who: "Keystone",
  });
}

beforeEach(() => resetCodingSessionWakeOperations());

test("a cold cache reads the no-fold line, and nothing is invoked", () => {
  // The cache is a plain module value: there is no Tauri command, no relay
  // client and no subscription in this path at all, so "no invoke fires" is a
  // property of the code rather than of a spy. The count is asserted anyway,
  // through the only seam a fetch could hide behind.
  const invokes = [];
  globalThis.__TAURI_INTERNALS__ = {
    invoke: (...args) => {
      invokes.push(args);
      return Promise.resolve(null);
    },
  };
  try {
    const operations = readCachedCodingSessionWakeOperations(SCOPE);
    assert.equal(operations.size, 0);
    assert.equal(
      read(operations),
      "Keystone sent a wake for operation 3234382f — this lens holds no session records; open Mission to read it.",
    );
    assert.equal(invokes.length, 0);
  } finally {
    delete globalThis.__TAURI_INTERNALS__;
  }
});

test("a warm cache reads the resolved line, and still invokes nothing", () => {
  const invokes = [];
  globalThis.__TAURI_INTERNALS__ = {
    invoke: (...args) => {
      invokes.push(args);
      return Promise.resolve(null);
    },
  };
  try {
    rememberCodingSessionWakeOperations(SCOPE, missionIndex());
    const operations = readCachedCodingSessionWakeOperations(SCOPE);
    assert.equal(operations.size, 1);
    assert.equal(
      read(operations),
      "Keystone assigned builder: Render kind 44246 in the Audit tab",
    );
    assert.equal(invokes.length, 0);
  } finally {
    delete globalThis.__TAURI_INTERNALS__;
  }
});

test("a pointer absent from a warm fold reads the unresolved row, not the no-fold one", () => {
  rememberCodingSessionWakeOperations(SCOPE, missionIndex());
  const operations = readCachedCodingSessionWakeOperations(SCOPE);
  const line = codingSessionWakeReadingForText({
    operations,
    signerPubkey: AUTHOR,
    text: JSON.stringify({ operationId: ABSENT_ID, type: "report" }),
    who: "Keystone",
  });
  // A lens that *does* hold records and does not hold this one says so; the
  // no-fold sentence is reserved for a lens that has not looked.
  assert.equal(
    line,
    "Keystone sent a wake for operation 00000000 (report) — not in this session's records yet",
  );
});

test("a different session never reads another's cache", () => {
  rememberCodingSessionWakeOperations(SCOPE, missionIndex());
  for (const other of [
    { ...SCOPE, channelRef: "00000000-0000-4000-8000-000000000000" },
    { ...SCOPE, sessionRef: "00000000-0000-4000-8000-000000000000" },
    { ...SCOPE, genesisRef: "ff".repeat(32) },
    { ...SCOPE, founderPubkey: "ff".repeat(32) },
  ]) {
    assert.equal(readCachedCodingSessionWakeOperations(other).size, 0);
  }
});

test("a partly-unknown scope neither reads nor writes", () => {
  const partial = { ...SCOPE, genesisRef: null };
  assert.equal(codingSessionWakeOperationScopeKey(partial), null);
  rememberCodingSessionWakeOperations(partial, missionIndex());
  assert.equal(readCachedCodingSessionWakeOperations(SCOPE).size, 0);
  assert.equal(readCachedCodingSessionWakeOperations(partial).size, 0);
});

test("an empty index is never remembered, so a cold lens stays cold", () => {
  rememberCodingSessionWakeOperations(SCOPE, new Map());
  assert.equal(readCachedCodingSessionWakeOperations(SCOPE).size, 0);
});

test("the reset releases it, for the community switch that needs it to", () => {
  rememberCodingSessionWakeOperations(SCOPE, missionIndex());
  assert.equal(readCachedCodingSessionWakeOperations(SCOPE).size, 1);
  resetCodingSessionWakeOperations();
  assert.equal(readCachedCodingSessionWakeOperations(SCOPE).size, 0);
});
