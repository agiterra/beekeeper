import assert from "node:assert/strict";
import { test } from "node:test";

import { fetchProjectPulseDigest } from "@/features/project-pulse/lib/pulseQueries";

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

test("a resolved but empty channel set is a complete, confirmed-empty read", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, [], {
    fetchEvents: noEvents,
    channelsUnresolved: false,
  });
  assert.equal(digest.complete, true);
  assert.deepEqual(digest.errors, []);
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
