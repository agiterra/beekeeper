import assert from "node:assert/strict";
import { test } from "node:test";

import { buildContributorRows } from "@/features/contributors/lib/contributorsModel";

const LEAD_PUBKEY = "1".repeat(64);
const REVIEWER_PUBKEY = "2".repeat(64);
const VERIFIER_PUBKEY = "3".repeat(64);

function seat(overrides = {}) {
  return {
    key: "chan-1/gen-1",
    channelId: "chan-1",
    generationId: "gen-1",
    label: "Fix the gate",
    agentPubkey: LEAD_PUBKEY,
    agentName: "Ada",
    role: "lead",
    projectId: "proj-1",
    projectName: "Beekeeper",
    status: "running",
    ageSeconds: 90,
    packSha: null,
    ...overrides,
  };
}

test("buildContributorRows excludes seats with no agent (a person's session)", () => {
  const rows = buildContributorRows({
    seats: [seat({ agentPubkey: null, agentName: null })],
    openKeys: new Set(["chan-1/gen-1"]),
  });
  assert.deepEqual(rows, []);
});

test("buildContributorRows groups multiple seats by agent, unioning roles and counting seats", () => {
  const rows = buildContributorRows({
    seats: [
      seat({ key: "chan-1/gen-1", role: "lead", ageSeconds: 500 }),
      seat({
        key: "chan-2/gen-1",
        channelId: "chan-2",
        role: "verifier",
        status: "completed",
        ageSeconds: 90,
      }),
    ],
    openKeys: new Set(["chan-1/gen-1"]),
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].agentPubkey, LEAD_PUBKEY);
  assert.deepEqual(rows[0].roles, ["lead", "verifier"]);
  assert.equal(rows[0].seatCount, 2);
  // Freshest observation wins, regardless of which seat is open.
  assert.equal(rows[0].lastStatus, "completed");
  assert.equal(rows[0].lastAgeSeconds, 90);
  // isSeatedNow is true because one of the two seats is open, even though
  // the freshest observation itself is the closed one.
  assert.equal(rows[0].isSeatedNow, true);
});

test("buildContributorRows: a row with no open seat is not seated now", () => {
  const rows = buildContributorRows({
    seats: [seat({ status: "completed", ageSeconds: 3_600 })],
    openKeys: new Set(),
  });
  assert.equal(rows[0].isSeatedNow, false);
});

test("buildContributorRows keeps the agent's name once any seat carries it", () => {
  const rows = buildContributorRows({
    seats: [
      seat({ key: "chan-1/gen-1", agentName: null }),
      seat({ key: "chan-2/gen-1", channelId: "chan-2", agentName: "Ada" }),
    ],
    openKeys: new Set(),
  });
  assert.equal(rows[0].name, "Ada");
});

test("buildContributorRows sorts seated-first, then freshest last-seen, then name", () => {
  const rows = buildContributorRows({
    seats: [
      seat({
        key: "chan-1/gen-1",
        agentPubkey: REVIEWER_PUBKEY,
        agentName: "Bea",
        status: "completed",
        ageSeconds: 60,
      }),
      seat({
        key: "chan-2/gen-1",
        channelId: "chan-2",
        agentPubkey: LEAD_PUBKEY,
        agentName: "Ada",
        status: "running",
        ageSeconds: 500,
      }),
      seat({
        key: "chan-3/gen-1",
        channelId: "chan-3",
        agentPubkey: VERIFIER_PUBKEY,
        agentName: "Cy",
        status: "stopped",
        ageSeconds: 30,
      }),
    ],
    openKeys: new Set(["chan-2/gen-1"]),
  });
  assert.deepEqual(
    rows.map((row) => row.name),
    ["Ada", "Cy", "Bea"],
  );
});

test("buildContributorRows: an unknown age sorts last among rows in the same seated state", () => {
  const rows = buildContributorRows({
    seats: [
      seat({
        key: "chan-1/gen-1",
        agentPubkey: REVIEWER_PUBKEY,
        agentName: "Bea",
        status: "unknown",
        ageSeconds: null,
      }),
      seat({
        key: "chan-2/gen-1",
        channelId: "chan-2",
        agentPubkey: LEAD_PUBKEY,
        agentName: "Ada",
        status: "stopped",
        ageSeconds: 120,
      }),
    ],
    openKeys: new Set(),
  });
  assert.deepEqual(
    rows.map((row) => row.name),
    ["Ada", "Bea"],
  );
});
