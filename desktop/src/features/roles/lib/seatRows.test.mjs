import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildSeatRows,
  compareSeatsLiveFirst,
  seatAgeSeconds,
  seatIsLive,
} from "@/features/roles/lib/seatRows";

const NOW = 1_800_000_000;
const LEAD_PUBKEY = "1".repeat(64);

function entry(overrides = {}) {
  const { session: sessionOverrides = {}, ...rest } = overrides;
  return {
    placement: "project",
    projectId: "proj-1",
    channelId: "chan-1",
    generationId: "gen-1",
    label: "Fix the gate",
    isClosed: false,
    isArchived: false,
    session: {
      status: "running",
      statusAt: (NOW - 90) * 1_000,
      agentRef: LEAD_PUBKEY,
      role: "lead",
      packRef: null,
      ...sessionOverrides,
    },
    ...rest,
  };
}

function project(overrides = {}) {
  return { id: "proj-1", name: "Beekeeper", ...overrides };
}

test("seatAgeSeconds converts catalog milliseconds to whole seconds and never goes negative", () => {
  assert.equal(seatAgeSeconds((NOW - 90) * 1_000, NOW), 90);
  assert.equal(seatAgeSeconds((NOW + 5) * 1_000, NOW), 0);
  assert.equal(seatAgeSeconds(null, NOW), null);
  assert.equal(seatAgeSeconds(undefined, NOW), null);
  assert.equal(seatAgeSeconds(Number.NaN, NOW), null);
});

test("seatIsLive names exactly the catalog words a running provider can report", () => {
  assert.equal(seatIsLive("starting"), true);
  assert.equal(seatIsLive("idle"), true);
  assert.equal(seatIsLive("running"), true);
  assert.equal(seatIsLive("waiting_for_input"), true);
  assert.equal(seatIsLive("completed"), false);
  assert.equal(seatIsLive("stopped"), false);
  assert.equal(seatIsLive("failed"), false);
  assert.equal(seatIsLive("interrupted"), false);
  assert.equal(seatIsLive("disconnected"), false);
  assert.equal(seatIsLive("unknown"), false);
});

test("compareSeatsLiveFirst puts every live seat before every not-live seat", () => {
  const rows = [
    { key: "b", status: "completed", ageSeconds: 5 },
    { key: "a", status: "running", ageSeconds: 500 },
    { key: "c", status: "failed", ageSeconds: 1 },
    { key: "d", status: "idle", ageSeconds: 1_000 },
  ];
  const sorted = [...rows].sort(compareSeatsLiveFirst);
  // Live first (a, d — freshest of the two live rows first), then not-live,
  // still freshest-first within that half (c at 1s ages before b at 5s).
  assert.deepEqual(
    sorted.map((row) => row.key),
    ["a", "d", "c", "b"],
  );
});

test("buildSeatRows drops closed entries by default and includes them with includeClosed", () => {
  const shelfEntries = [
    entry({ generationId: "gen-open" }),
    entry({ generationId: "gen-closed", isClosed: true }),
  ];
  const withoutClosed = buildSeatRows({
    shelfEntries,
    agents: [],
    projects: [project()],
    nowSeconds: NOW,
  });
  assert.deepEqual(
    withoutClosed.map((row) => row.generationId),
    ["gen-open"],
  );
  const withClosed = buildSeatRows({
    shelfEntries,
    agents: [],
    projects: [project()],
    nowSeconds: NOW,
    includeClosed: true,
  });
  assert.deepEqual(withClosed.map((row) => row.generationId).sort(), [
    "gen-closed",
    "gen-open",
  ]);
});

test("buildSeatRows sorts live-first, then freshest-first", () => {
  const shelfEntries = [
    entry({
      generationId: "gen-old-live",
      session: { status: "running", statusAt: (NOW - 500) * 1_000 },
    }),
    entry({
      generationId: "gen-fresh-done",
      session: { status: "completed", statusAt: (NOW - 5) * 1_000 },
    }),
    entry({
      generationId: "gen-fresh-live",
      session: { status: "idle", statusAt: (NOW - 10) * 1_000 },
    }),
  ];
  const rows = buildSeatRows({
    shelfEntries,
    agents: [],
    projects: [project()],
    nowSeconds: NOW,
  });
  assert.deepEqual(
    rows.map((row) => row.generationId),
    ["gen-fresh-live", "gen-old-live", "gen-fresh-done"],
  );
});

test("relay identity names are fallback-only and do not replace session status or role", () => {
  const key = "ab".repeat(32);
  const input = {
    shelfEntries: [
      entry({
        session: {
          agentRef: key.toUpperCase(),
          role: null,
          status: "disconnected",
        },
      }),
    ],
    agents: [],
    relayAgents: [
      {
        pubkey: key,
        name: "Remote builder",
        status: "online",
        capabilities: ["builder"],
      },
    ],
    projects: [project()],
    nowSeconds: NOW,
  };
  const [remote] = buildSeatRows(input);
  assert.equal(remote.agentName, "Remote builder");
  assert.equal(remote.agentPubkey, key);
  assert.equal(remote.status, "disconnected");
  assert.equal(remote.role, null);
  const [local] = buildSeatRows({
    ...input,
    agents: [{ pubkey: key.toUpperCase(), name: "Local name" }],
  });
  assert.equal(local.agentName, "Local name");
});
