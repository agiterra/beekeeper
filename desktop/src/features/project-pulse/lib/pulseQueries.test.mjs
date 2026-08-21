import assert from "node:assert/strict";
import { test } from "node:test";

import {
  fetchProjectPulseDigest,
  projectPulseChannelSetUnresolved,
  pulseLeaseExpiryDelayMs,
} from "@/features/project-pulse/lib/pulseQueries";

const OWNER = "ab".repeat(32);
const PROJECT = `30621:${OWNER}:pulse-demo`;

const noEvents = async () => [];

/**
 * An unresolved channel set is a read that did not happen, not a project with
 * no channels. Without this the digest comes back `complete: true` with
 * `sessions: []` and the screen paints "the project is quiet" while a live
 * session runs in a channel the client never learned about. The Rust twin
 * (`scan_project_sessions`) pushes the same `{scope:"channels"}` row.
 */
test("an unresolved channel set makes the read partial, never quiet", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, [], {
    fetchEvents: noEvents,
    channelsUnresolved: true,
  });
  assert.equal(digest.complete, false);
  assert.deepEqual(
    digest.errors.map((error) => error.scope),
    ["channels"],
  );
});

test("a stale persisted channel floor remains unresolved during revalidation", () => {
  assert.equal(
    projectPulseChannelSetUnresolved({
      isPending: false,
      isError: false,
      isFetching: true,
    }),
    true,
  );
  assert.equal(
    projectPulseChannelSetUnresolved({
      isPending: false,
      isError: false,
      isFetching: false,
    }),
    false,
  );
});

test("a resolved but empty channel set is a complete, confirmed-empty read", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, [], {
    fetchEvents: noEvents,
    channelsUnresolved: false,
  });
  assert.equal(digest.complete, true);
  assert.deepEqual(digest.errors, []);
});

test("cold reads split durable history from the one-shot lease snapshot", async () => {
  const filters = [];
  await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEvents: async (filter) => {
      filters.push(filter);
      return [];
    },
  });
  assert.equal(filters.length, 3);
  assert.deepEqual(filters[0], {
    kinds: [44240],
    "#a": [PROJECT],
    limit: 500,
  });
  assert.deepEqual(filters[1], {
    kinds: [44221, 44223, 44224, 44227, 44229, 44230],
    "#h": ["channel-1"],
    limit: 1000,
  });
  assert.deepEqual(filters[2], {
    kinds: [24223],
    "#h": ["channel-1"],
    limit: 1000,
  });
});

test("cold reads sort, dedupe, and chunk explicit channels at the relay cap", async () => {
  const filters = [];
  const channels = [
    ...Array.from(
      { length: 129 },
      (_, index) => `channel-${String(index).padStart(3, "0")}`,
    ),
    "channel-000",
  ].reverse();
  await fetchProjectPulseDigest(PROJECT, channels, {
    fetchEvents: async (filter) => {
      filters.push(filter);
      return [];
    },
  });
  const channelFilters = filters.filter((filter) => filter["#h"]);
  assert.equal(channelFilters.length, 4);
  assert.ok(channelFilters.every((filter) => filter["#h"].length <= 128));
  assert.deepEqual(channelFilters[0]["#h"].slice(0, 2), [
    "channel-000",
    "channel-001",
  ]);
  assert.deepEqual(
    channelFilters.map((filter) => filter["#h"].length),
    [128, 1, 128, 1],
  );
});

test("reachable lease schedules invalidation at its exact folded expiry", () => {
  const digest = {
    sessions: [
      {
        coordinationState: "provider_reachable",
        generations: [
          {
            current: true,
            reachability: "provider_reachable",
            leaseExpiresAt: 1_000,
          },
        ],
      },
    ],
  };
  assert.equal(pulseLeaseExpiryDelayMs(digest, 999_250), 750);
  assert.equal(pulseLeaseExpiryDelayMs(digest, 1_000_000), 0);
  assert.equal(pulseLeaseExpiryDelayMs({ sessions: [] }, 999_250), null);
});

test("a rejected lease snapshot makes the digest partial, never quiet", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEvents: async (filter) => {
      if (filter.kinds.includes(24223))
        throw new Error("lease snapshot timed out");
      return [];
    },
  });
  assert.equal(digest.complete, false);
  assert.deepEqual(digest.providerReachableSessions, []);
  assert.ok(
    digest.errors.some(
      (error) =>
        error.scope === "leases" &&
        error.message === "lease snapshot timed out",
    ),
  );
});

/**
 * An event dropped by client-side validation is an observation the surface
 * does not have. Dropping it without a trace turns one undecodable live
 * session into a completed negative verdict.
 */
test("events excluded by client validation are recorded, not silently dropped", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEvents: async (filter) =>
      filter.kinds.includes(44240)
        ? []
        : [
            {
              id: "f".repeat(64),
              pubkey: OWNER,
              created_at: 1_785_512_437,
              kind: 44223,
              tags: [],
              content: "not json at all",
              sig: "0".repeat(128),
            },
          ],
  });
  assert.equal(digest.sessions.length, 0);
  assert.ok(
    digest.errors.some(
      (error) =>
        error.scope === "invalid-event" && error.message.includes("44223"),
    ),
    `expected an invalid-event row, got ${JSON.stringify(digest.errors)}`,
  );
});
