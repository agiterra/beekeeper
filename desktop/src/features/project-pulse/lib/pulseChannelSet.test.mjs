import assert from "node:assert/strict";
import { test } from "node:test";

import { projectPulseChannelIds } from "@/features/project-pulse/lib/pulseChannelSet";

const OWNER = "ab".repeat(32);
const PROJECT_ADDRESS = `30621:${OWNER}:pulse-demo`;

const project = {
  id: "project-1",
  address: PROJECT_ADDRESS,
  channelIds: [],
};

/**
 * The transport channel is where 44223/44227/44229/44230 live, and
 * `partitionChannels` — the sidebar's stream/forum split — drops it before the
 * back-reference is consulted. If the set were built from that partition
 * alone, a sessions channel created by a collaborator (no owner-curated
 * forward ref on the 30621 head) would be invisible, Pulse would query no
 * `#h` for it, and the screen would render "the project is quiet" over a live
 * session the CLI happily lists.
 */
test("a transport channel's own projectRef puts it in the set with no forward ref", () => {
  const ids = projectPulseChannelIds(
    project,
    [
      {
        id: "transport-1",
        channelType: "transport",
        projectRef: PROJECT_ADDRESS,
      },
    ],
    [],
  );
  assert.deepEqual(ids, ["transport-1"]);
});

test("a channel back-referencing a different project stays out", () => {
  const ids = projectPulseChannelIds(
    project,
    [
      { id: "transport-2", projectRef: `30621:${"cd".repeat(32)}:other` },
      { id: "transport-3", projectRef: null },
      { id: "transport-4" },
    ],
    [],
  );
  assert.deepEqual(ids, []);
});

/** The relay's `validate_project_ref_tag` accepts any ASCII hex pubkey, so a
 * stored `project_ref` can be case-variant; the set must still contain it. */
test("a case-variant coordinate normalizes into the same set", () => {
  const ids = projectPulseChannelIds(
    project,
    [
      {
        id: "transport-5",
        projectRef: `30621:${OWNER.toUpperCase()}:pulse-demo`,
      },
    ],
    [],
  );
  assert.deepEqual(ids, ["transport-5"]);
});

test("all three sources union, deduplicate, and sort", () => {
  const ids = projectPulseChannelIds(
    { ...project, channelIds: ["forward-1", "shared-1"] },
    [{ id: "shared-1", projectRef: PROJECT_ADDRESS }],
    ["bucketed-1", "shared-1"],
  );
  assert.deepEqual(ids, ["bucketed-1", "forward-1", "shared-1"]);
});
