import assert from "node:assert/strict";
import test from "node:test";

import {
  checkpointDiffMockKey,
  handleCheckpointDiffMockCommand,
} from "./e2eBridgeCheckpoints.ts";

const A = "a".repeat(40);
const B = "b".repeat(40);

test("passes through unless a spec configured answers", async () => {
  globalThis.window = {};
  assert.equal(
    await handleCheckpointDiffMockCommand(
      "coding_session_checkpoint_diff",
      {},
      null,
    ),
    null,
  );
  assert.equal(await handleCheckpointDiffMockCommand("other", {}, null), null);
});

test("answers the configured pair, no_checkout otherwise, baseline_missing for null", async () => {
  const local = { state: "local" };
  globalThis.window = {
    __BEEKEEPER_E2E_CHECKPOINT_DIFF__: { [checkpointDiffMockKey(A, B)]: local },
  };
  const command = "coding_session_checkpoint_diff";
  assert.deepEqual(
    await handleCheckpointDiffMockCommand(
      command,
      { fromTree: A, toTree: B },
      null,
    ),
    { handled: true, value: local },
  );
  assert.deepEqual(
    await handleCheckpointDiffMockCommand(
      command,
      { fromTree: B, toTree: A },
      null,
    ),
    { handled: true, value: { state: "no_checkout" } },
  );
  assert.deepEqual(
    await handleCheckpointDiffMockCommand(
      command,
      { fromTree: null, toTree: B },
      null,
    ),
    { handled: true, value: { state: "baseline_missing" } },
  );
  delete globalThis.window;
});
