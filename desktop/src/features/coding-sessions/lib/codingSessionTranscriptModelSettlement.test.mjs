import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveCodingSessionExecutionRestingStatus,
  resolveCodingSessionTurnSettlement,
  resolveCodingSessionUmbrellaBlockRestingStatuses,
  resolveCodingSessionWorkspaceSettlement,
} from "./codingSessionTranscriptModelSettlement.ts";
import { deriveCodingSessionWorkspaceStatus } from "./codingSessionWorkspaceModel.ts";

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

/**
 * SV-43: the single workspace's settlement for one signed status, as it
 * derives it — the header status from the transcript and the wire, then the
 * shared map.
 */
function singleViewSettlement(wireStatus, transcript = []) {
  return resolveCodingSessionWorkspaceSettlement({
    wireStatus,
    workspaceStatus: deriveCodingSessionWorkspaceStatus(
      transcript,
      wireStatus,
      null,
    ),
  });
}

test("SV-43: one signed status settles the same open turn alike in both views", () => {
  const openTurn = [{ id: "tool-1", type: "tool", turnId: "turn-1" }];
  const latest = block(1);
  for (const status of [
    "running",
    "idle",
    "completed",
    "stopped",
    "failed",
    "interrupted",
    "starting",
    "waiting_for_input",
    "disconnected",
    "unknown",
  ]) {
    const mission = resolveCodingSessionUmbrellaBlockRestingStatuses(
      umbrellaWith(status),
      [latest],
    ).get(key(latest));
    const single = singleViewSettlement(status, openTurn);
    const open = { completion: null, superseded: false };
    assert.equal(
      resolveCodingSessionTurnSettlement(
        { ...open, isWorking: single.isWorking },
        single.restingStatus,
      ),
      resolveCodingSessionTurnSettlement(
        { ...open, isWorking: status === "running" },
        mission,
      ),
      status,
    );
  }
});

test("SV-43: the header says Working exactly when the transcript shows a live turn", () => {
  const transcripts = {
    empty: [],
    openTurn: [{ id: "tool-1", type: "tool", turnId: "turn-1" }],
    statusRow: [
      {
        id: "status-1",
        type: "lifecycle",
        title: "Status",
        text: "running",
        timestamp: "2026-10-04T00:00:00Z",
      },
    ],
  };
  for (const status of [
    undefined,
    "running",
    "idle",
    "completed",
    "stopped",
    "failed",
    "interrupted",
    "starting",
    "waiting_for_input",
    "disconnected",
    "unknown",
  ]) {
    for (const [name, transcript] of Object.entries(transcripts)) {
      const header = deriveCodingSessionWorkspaceStatus(
        transcript,
        status,
        null,
      );
      const { isWorking, restingStatus } = singleViewSettlement(
        status,
        transcript,
      );
      const live =
        resolveCodingSessionTurnSettlement(
          { completion: null, isWorking, superseded: false },
          restingStatus,
        ) === "live";
      const label = `${String(status)} / ${name}`;
      assert.equal(header.label === "Working", live, label);
      if (status === "starting") assert.equal(header.label, "Starting", label);
    }
  }
});

test("SV-43: failed reads stopped in both views, starting is live in neither", () => {
  const latest = block(1);
  const missionFailed = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("failed"),
    [latest],
  ).get(key(latest));
  assert.equal(missionFailed, "stopped");
  assert.deepEqual(singleViewSettlement("failed"), {
    isWorking: false,
    restingStatus: "stopped",
  });
  const openTurn = [{ id: "tool-1", type: "tool", turnId: "turn-1" }];
  // The header reads a starting provider in its own word, not Working…
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(openTurn, "starting", null),
    { kind: "working", label: "Starting" },
  );
  // …and nothing settles a call under it as live.
  assert.deepEqual(singleViewSettlement("starting", openTurn), {
    isWorking: false,
    restingStatus: "unknown",
  });
});

test("SV-42: an Idle or Ended session settles its unfinished call as stopped", () => {
  for (const status of ["idle", "completed", "stopped", "interrupted"]) {
    const { isWorking, restingStatus } = singleViewSettlement(status);
    assert.equal(isWorking, false, status);
    assert.equal(restingStatus, "stopped", status);
    assert.equal(
      resolveCodingSessionTurnSettlement(
        { completion: null, isWorking, superseded: false },
        restingStatus,
      ),
      "settled",
      status,
    );
  }
  assert.deepEqual(singleViewSettlement(undefined), {
    isWorking: false,
    restingStatus: "unknown",
  });
});

test("a status the lease demoted vouches for nothing, not even a signed running", () => {
  const demoted = resolveCodingSessionWorkspaceSettlement({
    wireStatus: "running",
    workspaceStatus: {
      kind: "unknown",
      label: "No provider answering",
      attention: "unreachable",
    },
  });
  assert.deepEqual(demoted, { isWorking: false, restingStatus: "unknown" });
  assert.deepEqual(singleViewSettlement("running"), {
    isWorking: true,
    restingStatus: "running",
  });
});

test("a signed running the newer transcript overrode settles nothing", () => {
  assert.deepEqual(
    resolveCodingSessionWorkspaceSettlement({
      wireStatus: "running",
      workspaceStatus: { kind: "idle", label: "Idle" },
    }),
    { isWorking: false, restingStatus: "unknown" },
  );
});

// SV-44 on Mission's blocks, mirroring codingSessionTranscriptModelSupersession:
// a later turn ends an earlier one only if it starts after the earlier turn's
// last block.

test("SV-44: a later turn whose blocks interleave with an earlier turn's does not stop it", () => {
  // turn-a runs `long-build`; a queued prompt opens turn-b; turn-a keeps
  // publishing after it.
  const longBuild = block(1, { turnId: "turn-a" });
  const queued = block(2, { turnId: "turn-b" });
  const stillBuilding = block(3, { turnId: "turn-a" });
  const entries = [longBuild, queued, stillBuilding];
  const running = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    entries,
  );
  assert.equal(running.get(key(longBuild)), "running");
  assert.equal(running.get(key(queued)), "running");
  assert.equal(running.get(key(stillBuilding)), "running");
  // The active generation's signed status still decides once nothing in the
  // timeline does.
  const idle = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("idle"),
    entries,
  );
  assert.equal(idle.get(key(longBuild)), "stopped");
  const waiting = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("waiting_for_input"),
    entries,
  );
  assert.equal(waiting.get(key(longBuild)), "unknown");
});

test("SV-44: a turn that starts after the earlier turn went quiet stops it", () => {
  const quiet = block(1, { turnId: "turn-a" });
  const interleaved = block(2, { turnId: "turn-b" });
  const lastOfA = block(3, { turnId: "turn-a" });
  const afterA = block(4, { turnId: "turn-c" });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    [quiet, interleaved, lastOfA, afterA],
  );
  // turn-c began after turn-a's last block, so turn-a (every fragment) and
  // turn-b (whose last block precedes turn-c) are over.
  assert.equal(statuses.get(key(quiet)), "stopped");
  assert.equal(statuses.get(key(lastOfA)), "stopped");
  assert.equal(statuses.get(key(interleaved)), "stopped");
  assert.equal(statuses.get(key(afterA)), "running");
});

test("SV-44: another generation's or another seat's turns do not count as starting later", () => {
  const mine = block(1, { turnId: "turn-a" });
  const theirs = block(2, { executionKey: "exec-b", turnId: "turn-z" });
  const statuses = resolveCodingSessionUmbrellaBlockRestingStatuses(
    umbrellaWith("running"),
    [mine, theirs],
  );
  assert.equal(statuses.get(key(mine)), "running");
});
