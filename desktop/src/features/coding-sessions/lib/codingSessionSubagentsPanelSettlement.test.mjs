import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionOrchestrationSpawn } from "./codingSessionAgentsOrchestrationModel.ts";
import {
  deriveCodingSessionSubagentPanel,
  formatCodingSessionSubagentFooter,
  settleCodingSessionSubagentPanel,
  settleCodingSessionSubagentSpawns,
} from "./codingSessionSubagents.ts";
import { deriveCodingSessionTranscriptModel } from "./codingSessionTranscriptModel.ts";
import {
  resolveCodingSessionGenerationCallSettlement,
  resolveCodingSessionTurnSettlement,
  resolveCodingSessionTurnSettlementsById,
  resolveCodingSessionWorkspaceSettlement,
} from "./codingSessionTranscriptModelSettlement.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";
import { deriveCodingSessionWorkspaceStatus } from "./codingSessionWorkspaceModel.ts";

// The Agents surface (panel rows and orchestration "Direct spawns") reads an
// open Task call through the same turn settlement as its stream row, so the
// two never contradict each other side by side.

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function projectTurns(pairs) {
  return projectCodingSessionTranscript(
    pairs.map(([turnId, item], index) => ({
      target: TARGET,
      eventSeq: index + 1,
      timestamp: 1_700_000_000_000 + index * 1_000,
      turnId,
      item,
    })),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

function taskCall(toolId, description) {
  return {
    kind: "tool_call",
    tool: {
      toolName: description,
      toolKind: "think",
      toolId,
      input: { description, prompt: "Look around", subagent_type: "Explore" },
    },
  };
}

/** An abandoned turn: an open Task call, no result, no later prompt. */
function abandonedTurn() {
  return projectTurns([
    ["turn-a", { kind: "user_prompt", content: "Go" }],
    ["turn-a", taskCall("task-1", "Abandoned")],
  ]);
}

/** The single workspace's path, as `CodingSessionWorkspace` wires it. */
function workspacePanel(transcript, workspaceStatus, wireStatus) {
  const settlement = resolveCodingSessionWorkspaceSettlement({
    wireStatus,
    workspaceStatus,
  });
  const model = deriveCodingSessionTranscriptModel(transcript, {
    isWorking: settlement.isWorking,
  });
  const byTurn = resolveCodingSessionTurnSettlementsById(
    model.blocks,
    settlement.restingStatus,
  );
  const panel = settleCodingSessionSubagentPanel(
    deriveCodingSessionSubagentPanel([transcript]),
    (call) => (call.turnId ? (byTurn.get(call.turnId) ?? "live") : "live"),
  );
  const turn = model.blocks.find((block) => block.kind === "turn");
  const entry = turn.entries.find(
    (candidate) => candidate.kind === "subagents",
  );
  const stream = settleCodingSessionSubagentSpawns(
    entry.spawns,
    resolveCodingSessionTurnSettlement(turn, settlement.restingStatus),
  );
  return { panel, stream };
}

test("an open Task in an idle session reads stopped in the panel, as in the stream", () => {
  const transcript = abandonedTurn();
  const { panel, stream } = workspacePanel(
    transcript,
    deriveCodingSessionWorkspaceStatus(transcript, "idle", null),
    "idle",
  );
  assert.deepEqual(
    stream.map((spawn) => spawn.status),
    ["stopped"],
  );
  assert.equal(panel.rows[0].status, "stopped");
  assert.equal(panel.running, 0);
  assert.equal(panel.unknown, 0);
  assert.equal(formatCodingSessionSubagentFooter(panel), "1 settled");
  assert.equal(
    deriveCodingSessionOrchestrationSpawn(panel.rows[0]).status,
    "stopped",
  );
});

test("an open Task reads status unknown in the panel when the provider is unreachable", () => {
  const transcript = abandonedTurn();
  const { panel, stream } = workspacePanel(
    transcript,
    {
      kind: "unknown",
      label: "No provider answering",
      attention: "unreachable",
    },
    "running",
  );
  assert.deepEqual(
    stream.map((spawn) => spawn.status),
    ["unknown"],
  );
  assert.equal(panel.rows[0].status, "unknown");
  assert.equal(panel.running, 0);
  assert.equal(panel.settled, 0, "unknown is never counted as settled");
  assert.equal(
    formatCodingSessionSubagentFooter(panel),
    "0 settled · 1 status unknown",
  );
  assert.equal(
    deriveCodingSessionOrchestrationSpawn(panel.rows[0]).status,
    "unknown",
  );
});

test("a working session keeps the open Task running in both", () => {
  const transcript = abandonedTurn();
  const { panel, stream } = workspacePanel(
    transcript,
    deriveCodingSessionWorkspaceStatus(transcript, "running", null),
    "running",
  );
  assert.deepEqual(
    stream.map((spawn) => spawn.status),
    ["running"],
  );
  assert.equal(panel.rows[0].status, "running");
  assert.equal(panel.running, 1);
});

test("Mission's panel settles an open Task by its generation's signed status", () => {
  const transcript = abandonedTurn();
  const read = (status, generationId = "generation-1") =>
    deriveCodingSessionSubagentPanel([transcript], (call) =>
      resolveCodingSessionGenerationCallSettlement({
        activeGeneration: { generationId: "generation-1", status },
        generationId,
        transcript,
      })(call),
    ).rows[0].status;
  assert.equal(read("running"), "running");
  assert.equal(read("idle"), "stopped");
  assert.equal(read("disconnected"), "unknown");
  assert.equal(read("waiting_for_input"), "unknown");
  // A replaced generation's open call is over whatever the seat says now.
  assert.equal(read("running", "generation-0"), "stopped");
});

test("Mission's panel: a turn another turn started after is over (SV-44)", () => {
  const quiet = projectTurns([
    ["turn-a", { kind: "user_prompt", content: "Go" }],
    ["turn-a", taskCall("task-1", "Abandoned")],
    ["turn-b", { kind: "assistant_text", text: "Later turn." }],
  ]);
  const interleaved = projectTurns([
    ["turn-a", { kind: "user_prompt", content: "Go" }],
    ["turn-a", taskCall("task-1", "Still going")],
    ["turn-b", { kind: "assistant_text", text: "Queued turn." }],
    ["turn-a", { kind: "assistant_text", text: "still building" }],
  ]);
  const read = (transcript) =>
    deriveCodingSessionSubagentPanel([transcript], (call) =>
      resolveCodingSessionGenerationCallSettlement({
        activeGeneration: { generationId: "generation-1", status: "running" },
        generationId: "generation-1",
        transcript,
      })(call),
    ).rows[0].status;
  assert.equal(read(quiet), "stopped");
  assert.equal(read(interleaved), "running");
});
