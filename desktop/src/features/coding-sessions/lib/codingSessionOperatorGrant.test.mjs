import assert from "node:assert/strict";
import test from "node:test";

import {
  ensureCodingSessionCreateOperatorGrants,
  ensureCodingSessionOperatorGrant,
  ensureCodingSessionSeatGrant,
} from "./codingSessionOperatorGrant.ts";

const CHANNEL_ID = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a1".repeat(32);
const PROVIDER_A = "aa".repeat(32);
const PROVIDER_B = "bb".repeat(32);
const LEAD = "cc".repeat(32);
const BUILDER = "dd".repeat(32);

function receiptBackedHarness(initial = []) {
  const accepted = new Map(initial);
  const published = [];
  const dependencies = {
    fetchFold: async () => ({
      accepted: new Map(accepted),
      activeSeats: new Map(),
      acceptedHead: null,
      pending: [],
    }),
    publishTransition: async (input) => {
      published.push(input.granteePubkey);
      accepted.set(input.granteePubkey, "operator");
      return { id: `${published.length}`.padStart(64, "0") };
    },
    wait: async () => {},
  };
  return { accepted, dependencies, published };
}

test("first lead grants the verified provider authority before its actor", async () => {
  const harness = receiptBackedHarness();
  await ensureCodingSessionOperatorGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      granteePubkey: PROVIDER_A,
    },
    harness.dependencies,
  );
  await ensureCodingSessionOperatorGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      granteePubkey: LEAD,
    },
    harness.dependencies,
  );
  assert.deepEqual(harness.published, [PROVIDER_A, LEAD]);
});

test("lead seat grant is receipt-backed and idempotent", async () => {
  const activeSeats = new Map();
  const published = [];
  const dependencies = {
    fetchFold: async () => ({
      accepted: new Map(),
      activeSeats: new Map(activeSeats),
      acceptedHead: null,
      pending: [],
    }),
    publishTransition: async (input) => {
      published.push(input);
      activeSeats.set(input.granteePubkey, input.role);
      return { id: "12".repeat(32) };
    },
    wait: async () => {},
  };
  const first = await ensureCodingSessionSeatGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      actorPubkey: LEAD,
      role: "lead",
    },
    dependencies,
  );
  const second = await ensureCodingSessionSeatGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      actorPubkey: LEAD,
      role: "lead",
    },
    dependencies,
  );
  assert.equal(published.length, 1);
  assert.deepEqual(published[0], {
    channelId: CHANNEL_ID,
    genesisRef: GENESIS_REF,
    type: "grant-seat",
    granteePubkey: LEAD,
    role: "lead",
  });
  assert.equal(first.status, "granted");
  assert.equal(second.status, "already-active");
});

test("later mixed-provider hire preserves the existing grant and orders provider before actor", async () => {
  const harness = receiptBackedHarness([[PROVIDER_A, "operator"]]);
  const existing = await ensureCodingSessionOperatorGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      granteePubkey: PROVIDER_A,
    },
    harness.dependencies,
  );
  await ensureCodingSessionOperatorGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      granteePubkey: PROVIDER_B,
    },
    harness.dependencies,
  );
  await ensureCodingSessionOperatorGrant(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      granteePubkey: BUILDER,
    },
    harness.dependencies,
  );
  assert.deepEqual(existing, { status: "already-active", eventId: null });
  assert.deepEqual(harness.published, [PROVIDER_B, BUILDER]);
});

test("an accepted transition without a verified receipt never counts as granted", async () => {
  let publishes = 0;
  await assert.rejects(
    ensureCodingSessionOperatorGrant(
      {
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        granteePubkey: PROVIDER_A,
      },
      {
        fetchFold: async () => ({
          accepted: new Map(),
          acceptedHead: null,
          pending: [],
        }),
        publishTransition: async () => {
          publishes += 1;
          return { id: "01".repeat(32) };
        },
        wait: async () => {},
        receiptPollAttempts: 2,
      },
    ),
    /signed receipt did not appear/,
  );
  assert.equal(
    publishes,
    1,
    "missing receipt must not trigger a duplicate grant",
  );
});

test("first-create provider grant failure stops before actor authority", async () => {
  const attempted = [];
  const result = await ensureCodingSessionCreateOperatorGrants(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      providerAuthorityPubkey: PROVIDER_A,
      actorPubkey: LEAD,
    },
    {
      ensureGrant: async ({ granteePubkey }) => {
        attempted.push(granteePubkey);
        throw new Error("provider receipt missing");
      },
    },
  );
  assert.deepEqual(attempted, [PROVIDER_A]);
  assert.deepEqual(result, {
    ok: false,
    failed: "provider",
    reason: "provider receipt missing",
  });
});

test("first-create actor grant runs only after provider authority lands", async () => {
  const attempted = [];
  const result = await ensureCodingSessionCreateOperatorGrants(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      providerAuthorityPubkey: PROVIDER_A,
      actorPubkey: LEAD,
    },
    {
      ensureGrant: async ({ granteePubkey }) => {
        attempted.push(granteePubkey);
        if (granteePubkey === LEAD) throw new Error("actor receipt missing");
        return { status: "granted", eventId: "01".repeat(32) };
      },
    },
  );
  assert.deepEqual(attempted, [PROVIDER_A, LEAD]);
  assert.deepEqual(result, {
    ok: false,
    failed: "actor",
    reason: "actor receipt missing",
  });
});
