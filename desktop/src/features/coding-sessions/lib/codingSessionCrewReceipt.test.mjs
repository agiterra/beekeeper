import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "@/shared/constants/kinds.ts";
import {
  awaitCodingSessionCreateReceipt,
  buildCodingSessionCrewReceiptFilter,
} from "./codingSessionCrewReceipt.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "./codingSessionTrustedIngress.ts";

const CHANNEL_ID = "channel-1";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const IMPOSTOR_SECRET = generateSecretKey();
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function receiptEvent(overrides = {}, secret = PROVIDER_SECRET) {
  const value = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "cmd-1",
    status: "created",
    session: TARGET,
    error: null,
    ...overrides,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: 1_800_000_000,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", value.commandId],
        ["csl-key", lifecycleReceiptSemanticKey(value.commandId)],
      ],
      content: JSON.stringify(value),
    },
    secret,
  );
}

function client(events, { live = [] } = {}) {
  const emitters = [];
  return {
    emitters,
    async fetchEvents() {
      return events;
    },
    async subscribeLive(_filter, onEvent) {
      emitters.push(onEvent);
      for (const event of live) onEvent(event);
      return () => {};
    },
  };
}

const COMMAND = {
  channelId: CHANNEL_ID,
  commandId: "cmd-1",
  providerAuthorityPubkey: PROVIDER_PUBKEY,
  timeoutMs: 50,
};

test("the filter is one kind, one channel, one provider", () => {
  assert.deepEqual(
    buildCodingSessionCrewReceiptFilter({
      channelId: CHANNEL_ID,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
    }),
    {
      kinds: [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
      "#h": [CHANNEL_ID],
      authors: [PROVIDER_PUBKEY],
      limit: 200,
    },
  );
});

test("a backfilled receipt hands back the target it minted", async () => {
  const target = await awaitCodingSessionCreateReceipt(COMMAND, {
    client: client([receiptEvent()]),
  });
  assert.deepEqual(target, TARGET);
});

test("a live receipt resolves the same wait", async () => {
  const source = client([], { live: [receiptEvent()] });
  const target = await awaitCodingSessionCreateReceipt(COMMAND, {
    client: source,
  });
  assert.deepEqual(target, TARGET);
});

test("a seat awaiting its metadata still counts as created", async () => {
  // The gate is "the seat exists", and a receipt establishes that on its own —
  // waiting for metadata would hang a healthy launch on a later fact.
  const target = await awaitCodingSessionCreateReceipt(COMMAND, {
    client: client([receiptEvent({ status: "created" })]),
  });
  assert.deepEqual(target, TARGET);
});

test("a refusal rejects with the provider's own code and words", async () => {
  await assert.rejects(
    awaitCodingSessionCreateReceipt(COMMAND, {
      client: client([
        receiptEvent({
          status: "failed",
          session: null,
          error: { code: "ACTOR_UNAVAILABLE", message: "no staged seat" },
        }),
      ]),
    }),
    /ACTOR_UNAVAILABLE: no staged seat/,
  );
});

test("a receipt signed by anyone else is not an answer", async () => {
  await assert.rejects(
    awaitCodingSessionCreateReceipt(COMMAND, {
      client: client([receiptEvent({}, IMPOSTOR_SECRET)]),
    }),
    /did not answer within the wait/,
  );
});

test("a receipt for a different command is not an answer", async () => {
  await assert.rejects(
    awaitCodingSessionCreateReceipt(COMMAND, {
      client: client([receiptEvent({ commandId: "cmd-2" })]),
    }),
    /did not answer within the wait/,
  );
});
