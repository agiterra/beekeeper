import assert from "node:assert/strict";
import { test } from "node:test";

import {
  agentProgressChunkReads,
  fetchAgentProgressCoordination,
} from "@/features/agent-progress/lib/agentProgressCoordination";

const OWNER = "ab".repeat(32);

test("one bundle per 128-channel chunk, two filters with two budgets each", async () => {
  const batches = [];
  const channels = Array.from(
    { length: 129 },
    (_, index) => `channel-${String(index).padStart(3, "0")}`,
  ).reverse();
  await fetchAgentProgressCoordination(channels, {
    fetchEventsBatch: async (filters) => {
      batches.push(filters);
      return [];
    },
  });
  assert.equal(batches.length, 2);
  assert.deepEqual(
    batches.map((bundle) => bundle.map((filter) => filter["#h"].length)),
    [
      [128, 128],
      [1, 1],
    ],
  );
  assert.deepEqual(
    batches[0],
    agentProgressChunkReads(batches[0][0]["#h"]).map((read) => read.filter),
  );
  assert.deepEqual(
    batches[0][0].kinds,
    [44221, 44223, 44224, 44227, 44229, 44230],
  );
  assert.deepEqual(batches[0][1].kinds, [24223]);
  assert.deepEqual(batches[0][0]["#h"].slice(0, 2), [
    "channel-000",
    "channel-001",
  ]);
});

test("a failed bundle records both of its reads as not having happened", async () => {
  const read = await fetchAgentProgressCoordination(["channel-1"], {
    fetchEventsBatch: async () => {
      throw new Error("relay unreachable");
    },
  });
  assert.equal(read.complete, false);
  assert.deepEqual(
    read.errors
      .filter((error) => error.message === "relay unreachable")
      .map((error) => error.scope)
      .sort(),
    ["leases", "sessions"],
  );
});

test("truncation is attributed to the read whose budget filled", async () => {
  const lease = (index) => ({
    id: `lease-${index}`,
    pubkey: OWNER,
    created_at: 1_000 + index,
    kind: 24223,
    tags: [],
    content: "{}",
    sig: "0".repeat(128),
  });
  const read = await fetchAgentProgressCoordination(["channel-1"], {
    fetchEventsBatch: async () =>
      Array.from({ length: 1_000 }, (_, index) => lease(index)),
  });
  assert.deepEqual(
    read.errors.filter((error) => error.message.includes("truncated")),
    [{ scope: "leases", message: "lease snapshot truncated at 1000 events" }],
  );
});

// Ledger 310: with a held-read store the durable read becomes a delta, the
// lease snapshot does not.
test("a held store makes the second durable read a delta and keeps leases whole", async () => {
  const batches = [];
  const heldReads = new Map();
  const fetchEventsBatch = async (filters) => {
    batches.push(filters);
    return [];
  };
  await fetchAgentProgressCoordination(["c1"], { fetchEventsBatch, heldReads });
  await fetchAgentProgressCoordination(["c1"], { fetchEventsBatch, heldReads });
  assert.equal(batches[0][0].since, undefined);
  assert.equal(typeof batches[1][0].since, "number");
  assert.equal(batches[1][1].since, undefined);
  assert.equal(batches[1][0].limit, batches[0][0].limit);
});
