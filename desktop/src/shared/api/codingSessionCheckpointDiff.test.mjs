import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeCodingSessionCheckpointDiff,
  fetchCodingSessionCheckpointDiff,
} from "./codingSessionCheckpointDiff.ts";

test("decodes the four native answers", () => {
  assert.deepEqual(
    decodeCodingSessionCheckpointDiff({ state: "no_checkout" }),
    { state: "no_checkout" },
  );
  assert.deepEqual(
    decodeCodingSessionCheckpointDiff({ state: "baseline_missing" }),
    { state: "baseline_missing" },
  );
  assert.deepEqual(
    decodeCodingSessionCheckpointDiff({
      state: "objects_missing",
      missing: ["a"],
    }),
    { state: "objects_missing", missing: ["a"] },
  );
  const local = decodeCodingSessionCheckpointDiff({
    state: "local",
    checkout: "seat_worktree",
    diff: {
      files: [
        {
          path: "a",
          additions: 1,
          deletions: 0,
          patch: "+x",
          truncated: false,
        },
      ],
      additions: 1,
      deletions: 0,
      commit_body: null,
    },
    filesNotListed: 2,
  });
  assert.equal(local.state, "local");
  assert.equal(local.filesNotListed, 2);
  assert.equal(local.files[0].patch, "+x");
});

test("an unknown or malformed answer throws rather than reading as no change", () => {
  for (const raw of [
    null,
    { state: "local" },
    {
      state: "local",
      checkout: "elsewhere",
      diff: { files: [], additions: 0, deletions: 0 },
      filesNotListed: 0,
    },
    { state: "objects_missing", missing: [1] },
    { state: "remote" },
  ]) {
    assert.throws(() => decodeCodingSessionCheckpointDiff(raw), /unreadable/);
  }
});

test("the request names the command's own camelCase arguments", async () => {
  const calls = [];
  await fetchCodingSessionCheckpointDiff(
    {
      sessionRef: "s",
      target: "t",
      fromTree: null,
      toTree: "b".repeat(40),
      projectRef: null,
    },
    async (command, args) => {
      calls.push([command, args]);
      return { state: "baseline_missing" };
    },
  );
  assert.deepEqual(calls, [
    [
      "coding_session_checkpoint_diff",
      {
        sessionRef: "s",
        target: "t",
        fromTree: null,
        toTree: "b".repeat(40),
        projectRef: null,
      },
    ],
  ]);
});
