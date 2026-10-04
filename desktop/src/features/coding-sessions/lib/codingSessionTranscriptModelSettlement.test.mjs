import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveCodingSessionExecutionRestingStatus,
  resolveCodingSessionTurnSettlement,
  resolveCodingSessionUmbrellaBlockRestingStatuses,
} from "./codingSessionTranscriptModelSettlement.ts";

const completion = { state: "completed" };

test("a turn with a completion or a later turn is settled whatever the session says", () => {
  for (const restingStatus of ["running", "stopped", "unknown"]) {
    for (const isWorking of [true, false]) {
      assert.equal(
        resolveCodingSessionTurnSettlement(
          { completion, isWorking, superseded: false },
          restingStatus,
        ),
        "settled",
      );
      assert.equal(
        resolveCodingSessionTurnSettlement(
          { completion: null, isWorking, superseded: true },
          restingStatus,
        ),
        "settled",
      );
    }
  }
});

test("an open turn is live while worked on, else the caller's resting status decides", () => {
  const open = { completion: null, superseded: false };
  assert.equal(
    resolveCodingSessionTurnSettlement({ ...open, isWorking: true }, "unknown"),
    "live",
  );
  assert.equal(
    resolveCodingSessionTurnSettlement(
      { ...open, isWorking: false },
      "running",
    ),
    "live",
  );
  assert.equal(
    resolveCodingSessionTurnSettlement(
      { ...open, isWorking: false },
      "stopped",
    ),
    "settled",
  );
  assert.equal(
    resolveCodingSessionTurnSettlement(
      { ...open, isWorking: false },
      "unknown",
    ),
    "unknown",
  );
});

test("only a definite stop is stopped; waiting, starting, disconnected and unknown are unknown", () => {
  assert.equal(
    resolveCodingSessionExecutionRestingStatus("running"),
    "running",
  );
  for (const status of [
    "idle",
    "completed",
    "stopped",
    "failed",
    "interrupted",
  ]) {
    assert.equal(resolveCodingSessionExecutionRestingStatus(status), "stopped");
  }
  for (const status of [
    "starting",
    "waiting_for_input",
    "disconnected",
    "unknown",
  ]) {
    assert.equal(resolveCodingSessionExecutionRestingStatus(status), "unknown");
  }
});

function block(
  seq,
  {
    executionKey = "exec-a",
    generationId = "gen-1",
    turnId = "turn-1",
    completed = false,
  } = {},
) {
  const items = [{ id: `item-${seq}`, type: "tool", turnId }];
  if (completed) {
    items.push({
      id: `result-${seq}`,
      type: "lifecycle",
      title: "Turn result",
      turnId,
    });
  }
  return {
    kind: "turn-block",
    executionKey,
    signerPubkey: "signer",
    generation: 1,
    generationId,
    turnId,
    blockSeq: seq,
    items,
    timestampMs: seq,
  };
}

function umbrellaWith(status, activeGenerationId = "gen-1") {
  return {
    executions: [
      {
        executionKey: "exec-a",
        activeGeneration: { generationId: activeGenerationId, status },
      },
    ],
  };
}

const key = (entry) => `block:${entry.generationId}:${entry.blockSeq}`;

test("the execution's latest turn takes its seat's signed status", () => {
  const latest = block(1);
  for (const [status, expected] of [
    ["running", "running"],
    ["idle", "stopped"],
    ["waiting_for_input", "unknown"],
    ["disconnected", "unknown"],
  ]) {
    const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
      umbrellaWith(status),
      [latest],
    );
    assert.equal(statuses.get(key(latest)), expected, status);
  }
});

test("a completed block, or one a later turn followed, is stopped while the seat runs", () => {
  const completedBlock = block(1, { completed: true });
  const earlier = block(2, { turnId: "turn-2" });
  const later = block(3, { turnId: "turn-3" });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    [completedBlock, earlier, later],
  );
  assert.equal(statuses.get(key(completedBlock)), "stopped");
  assert.equal(statuses.get(key(earlier)), "stopped");
  assert.equal(statuses.get(key(later)), "running");
});

test("a fragment of a turn still running is not settled by its own later fragment", () => {
  const first = block(1);
  const second = block(2);
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    [first, second],
  );
  assert.equal(statuses.get(key(first)), "running");
  assert.equal(statuses.get(key(second)), "running");
});

test("a fragment whose turn completed later is stopped", () => {
  const first = block(1);
  const second = block(2, { completed: true });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    [first, second],
  );
  assert.equal(statuses.get(key(first)), "stopped");
});

test("another seat's later blocks are not evidence about this one", () => {
  const mine = block(1);
  const theirs = block(2, { executionKey: "exec-b", turnId: "turn-9" });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("waiting_for_input"),
    [mine, theirs],
  );
  assert.equal(statuses.get(key(mine)), "unknown");
  // exec-b is not in the umbrella: nothing vouches for it.
  assert.equal(statuses.get(key(theirs)), "unknown");
});

test("a replaced generation's open turn is stopped", () => {
  const old = block(1, { generationId: "gen-0" });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("waiting_for_input", "gen-1"),
    [old],
  );
  assert.equal(statuses.get(key(old)), "stopped");
});
