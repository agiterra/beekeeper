import assert from "node:assert/strict";
import { test } from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_SYSTEM_MESSAGE } from "../../../shared/constants/kinds.ts";
import {
  CODING_SESSION_DELETION_RECEIPT_TYPE,
  parseCodingSessionDeletionReceipt,
  publishForgottenCodingSession,
  subscribeToForgottenCodingSessions,
} from "./codingSessionForgotten.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";
const GENESIS_REF = "ab".repeat(32);
const RELAY_SECRET = generateSecretKey();
const RELAY_PUBKEY = getPublicKey(RELAY_SECRET);
const MEMBER_SECRET = generateSecretKey();

function receipt({
  secret = RELAY_SECRET,
  content = {
    type: CODING_SESSION_DELETION_RECEIPT_TYPE,
    genesisRef: GENESIS_REF,
    sessionRef: SESSION_REF,
    deletionEventId: "cd".repeat(32),
    channelId: CHANNEL_ID,
  },
  tags = [["h", CHANNEL_ID]],
  kind = KIND_SYSTEM_MESSAGE,
} = {}) {
  return finalizeEvent(
    { kind, created_at: 1_700_000_000, tags, content: JSON.stringify(content) },
    secret,
  );
}

test("the relay's deletion receipt names the session to forget", () => {
  assert.deepEqual(parseCodingSessionDeletionReceipt(receipt(), RELAY_PUBKEY), {
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
  });
});

test("a receipt anybody else signed hides nothing", () => {
  assert.equal(
    parseCodingSessionDeletionReceipt(
      receipt({ secret: MEMBER_SECRET }),
      RELAY_PUBKEY,
    ),
    null,
  );
});

test("another system message, a malformed one, or one reaching across channels is not a receipt", () => {
  const base = {
    type: CODING_SESSION_DELETION_RECEIPT_TYPE,
    genesisRef: GENESIS_REF,
    sessionRef: SESSION_REF,
    deletionEventId: "cd".repeat(32),
    channelId: CHANNEL_ID,
  };
  assert.equal(
    parseCodingSessionDeletionReceipt(
      receipt({ content: { ...base, type: "member_joined" } }),
      RELAY_PUBKEY,
    ),
    null,
  );
  assert.equal(
    parseCodingSessionDeletionReceipt(
      receipt({ content: { ...base, genesisRef: "nope" } }),
      RELAY_PUBKEY,
    ),
    null,
  );
  assert.equal(
    parseCodingSessionDeletionReceipt(
      receipt({ tags: [["h", "other-channel"]] }),
      RELAY_PUBKEY,
    ),
    null,
  );
  assert.equal(
    parseCodingSessionDeletionReceipt(receipt({ kind: 1 }), RELAY_PUBKEY),
    null,
  );
});

test("the forget bus reaches every listener, and a listener that throws does not stop the rest", () => {
  const heard = [];
  const stop1 = subscribeToForgottenCodingSessions(() => {
    throw new Error("listener failed");
  });
  const stop2 = subscribeToForgottenCodingSessions((forgotten) =>
    heard.push(forgotten),
  );
  const original = console.error;
  console.error = () => {};
  try {
    publishForgottenCodingSession({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: null,
    });
  } finally {
    console.error = original;
  }
  assert.deepEqual(heard, [
    { channelId: CHANNEL_ID, sessionRef: SESSION_REF, genesisRef: null },
  ]);
  stop1();
  stop2();
  publishForgottenCodingSession({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    genesisRef: null,
  });
  assert.equal(heard.length, 1);
});
