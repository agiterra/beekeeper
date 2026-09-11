import assert from "node:assert/strict";
import { test } from "node:test";

import { partitionChannels } from "./hooks.ts";

const PROJECT = {
  id: "proj-ssd",
  address:
    "30621:ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34:ssd",
  channelIds: ["stream-1"],
};

function channel(id, channelType, projectRef = null) {
  return { id, name: id, channelType, projectRef };
}

test("a project's session transport is bucketed to the project, so a founded row can place by it", () => {
  const buckets = partitionChannels(
    [PROJECT],
    [
      channel("stream-1", "stream"),
      channel("forum-1", "forum", PROJECT.address),
      channel("ssd-sessions", "transport", PROJECT.address),
      channel("elsewhere", "transport", null),
    ],
  );
  assert.deepEqual(
    (buckets.channelsByProject.get(PROJECT.id) ?? []).map((c) => c.id),
    ["stream-1"],
  );
  assert.deepEqual(
    (buckets.forumsByProject.get(PROJECT.id) ?? []).map((c) => c.id),
    ["forum-1"],
  );
  // The bucket the founded shelf placement reads: before 2026-09-11 there was
  // none, and every "Not started" row filed under General.
  assert.deepEqual(
    (buckets.transportsByProject.get(PROJECT.id) ?? []).map((c) => c.id),
    ["ssd-sessions"],
  );
  // A transport nobody claims is not somebody's project.
  assert.equal(
    [...buckets.transportsByProject.values()]
      .flat()
      .some((c) => c.id === "elsewhere"),
    false,
  );
});
