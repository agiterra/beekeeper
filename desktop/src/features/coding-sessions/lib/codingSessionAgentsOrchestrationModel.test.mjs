import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionAgentState,
  codingSessionAgentStateWord,
  codingSessionPhaseTitle,
  deriveCodingSessionAgentsOrchestration,
  formatCodingSessionAgentModel,
  formatCodingSessionAgentTokens,
  formatCodingSessionPhaseCounts,
  formatCodingSessionSpawnMeta,
  formatCodingSessionToolCount,
  newestOpenToolName,
  readCodingSessionAgentOutcome,
  readCodingSessionAgentTokens,
} from "./codingSessionAgentsOrchestrationModel.ts";
import { deriveCodingSessionSubagentPanel } from "./codingSessionSubagents.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";
import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel.ts";
import {
  codingSessionWireWorkspaceStatus,
  deriveCodingSessionExecutionStatus,
} from "./codingSessionWorkspaceModel.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const BASE_MS = 1_800_000_000_000;

function target(instanceId, sessionId) {
  return { driver: "claude-agent-acp", instanceId, sessionId, generation: 1 };
}

/** Kind-44225 envelopes, as a relay delivers them, projected as any client does. */
function transcript(seatTarget, items) {
  return projectCodingSessionTranscript(
    items.map((entry, index) => ({
      target: seatTarget,
      eventSeq: index + 1,
      timestamp: BASE_MS + index * 1_000,
      turnId: entry.turnId ?? "turn-1",
      item: entry.turnId ? entry.item : entry,
    })),
    { channelId: "channel-1", generationId: `gen-${seatTarget.instanceId}` },
  );
}

function toolCall(toolId, toolName) {
  return {
    kind: "tool_call",
    tool: { toolName, toolKind: "execute", toolId, input: { command: "x" } },
  };
}

function toolResult(toolId, toolName) {
  return {
    kind: "tool_result",
    toolId,
    toolName,
    content: "ok",
    isError: false,
  };
}

function turnResult(usage) {
  return {
    kind: "result",
    subtype: "success",
    isError: false,
    durationMs: 1_000,
    result: "Done.",
    ...(usage === undefined ? {} : { usage }),
  };
}

/** A catalog record carrying only what kind 44223 metadata and 44225 say. */
function record({ actor, instanceId, model, provider, role, status, items }) {
  const seatTarget = target(
    instanceId,
    `${instanceId}-0000-4000-8000-000000000000`,
  );
  return {
    generationId: `gen-${instanceId}`,
    label: `Seat ${instanceId}`,
    title: `Seat ${instanceId}`,
    providerAuthorityPubkey: provider,
    metadataAuthorityPubkey: provider,
    lastEventAt: new Date(BASE_MS).toISOString(),
    status,
    statusAt: null,
    statusEventId: null,
    transcript: transcript(seatTarget, items),
    conflictCount: 0,
    commandTarget: seatTarget,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-agent-acp",
    runtime: "claude-agent-acp",
    model,
    agentRef: actor,
    role,
    turnBudget: null,
    routing: null,
    capabilities: null,
    beeStamp: null,
    packRef: null,
  };
}

const NAMES = new Map([
  ["a".repeat(64), "build r1"],
  ["b".repeat(64), "build r2"],
  ["c".repeat(64), "gate r1"],
  ["d".repeat(64), "review r1"],
]);

/**
 * A three-phase mission built from relay events alone: two builders (one
 * done, one running a Bash call), a gate still running, a reviewer that has
 * not reported usage.
 */
function missionRecords() {
  return [
    record({
      actor: "a".repeat(64),
      instanceId: "build-1",
      model: "claude-opus-5-5[1m]",
      provider: "1".repeat(64),
      role: "builder",
      status: "completed",
      items: [
        toolCall("t1", "Edit"),
        toolResult("t1", "Edit"),
        toolCall("t2", "Bash"),
        toolResult("t2", "Bash"),
        turnResult({ inputTokens: 150_000, outputTokens: 9_000 }),
      ],
    }),
    record({
      actor: "b".repeat(64),
      instanceId: "build-2",
      model: "claude-opus-5-5[1m]",
      provider: "2".repeat(64),
      role: "builder",
      status: "running",
      items: [
        toolCall("t1", "Read"),
        toolResult("t1", "Read"),
        turnResult({ inputTokens: 100_000, outputTokens: 5_000 }),
        { turnId: "turn-2", item: { kind: "user_prompt", content: "Next." } },
        { turnId: "turn-2", item: toolCall("t2", "Bash") },
      ],
    }),
    record({
      actor: "c".repeat(64),
      instanceId: "gate-1",
      model: "gpt-6",
      provider: "3".repeat(64),
      role: "gate",
      status: "running",
      items: [toolCall("t1", "StructuredOutput")],
    }),
    record({
      actor: "d".repeat(64),
      instanceId: "review-1",
      model: null,
      provider: "4".repeat(64),
      role: "reviewer",
      status: "failed",
      items: [toolCall("t1", "Grep"), toolResult("t1", "Grep"), turnResult()],
    }),
  ];
}

/** What any client computes from the catalog: no reachability, no local provider. */
function relayOnlyInput(records, extra = {}) {
  const [umbrella] = groupCodingSessionCatalog(records);
  const executions = umbrella.executions.map((execution) => ({
    execution,
    wireStatus: execution.activeGeneration.status,
    status: deriveCodingSessionExecutionStatus(execution),
  }));
  return {
    title: "Session parity wave B",
    missionKey: umbrella.umbrellaKey,
    executions,
    subagents: deriveCodingSessionSubagentPanel(
      umbrella.executions.map(
        (execution) => execution.activeGeneration.transcript,
      ),
    ),
    transactions: null,
    declaredWork: null,
    resolveActorName: (pubkey) => NAMES.get(pubkey) ?? null,
    ...extra,
  };
}

test("a three-phase mission renders one card with N/M settled in route order", () => {
  const model = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords()),
  );
  assert.equal(model.cards.length, 1);
  const [card] = model.cards;
  assert.deepEqual(
    card.phases.map((phase) => [phase.title, phase.state, phase.agents.length]),
    [
      ["Builder", "running", 2],
      ["Gate", "running", 1],
      ["Reviewer", "failed", 1],
    ],
  );
  assert.equal(card.settled, 2, "a completed builder and a failed reviewer");
  assert.equal(card.failed, 1);
  assert.equal(card.total, 4);
  assert.equal(card.live, true);
  assert.deepEqual(
    card.phases[0].agents.map((agent) => agent.state),
    ["done", "running"],
  );
  assert.equal(card.phases[2].agents[0].state, "failed");
  assert.equal(
    formatCodingSessionPhaseCounts(card.phases[0]),
    "1 active · 1 done",
  );
  assert.equal(formatCodingSessionPhaseCounts(card.phases[2]), "1 failed");
});

test("the lead's phase leads the pipeline, as the lead leads the route", () => {
  const records = missionRecords();
  records.push(
    record({
      actor: "e".repeat(64),
      instanceId: "lead-1",
      model: "claude-opus-5-5",
      provider: "5".repeat(64),
      role: "lead",
      status: "idle",
      items: [],
    }),
  );
  const [card] = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(records),
  ).cards;
  assert.deepEqual(
    card.phases.map((phase) => phase.title),
    ["Lead", "Builder", "Gate", "Reviewer"],
  );
});

test("expanded agents carry current tool, model, tokens and tool count", () => {
  const [card] = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords()),
  ).cards;
  const [done, running] = card.phases[0].agents;
  assert.equal(running.label, "build r2");
  assert.equal(running.currentTool, "Bash");
  assert.equal(running.model, "claude-opus-5-5[1m]");
  assert.equal(formatCodingSessionAgentTokens(running.tokens), "105k tok");
  assert.equal(running.toolCount, 2);
  // A settled seat has no current tool, even with a call left open.
  assert.equal(done.currentTool, null);
  assert.equal(formatCodingSessionAgentTokens(done.tokens), "159k tok");
  assert.equal(formatCodingSessionToolCount(done.toolCount), "2 tools");
  const [gate] = card.phases[1].agents;
  assert.equal(gate.currentTool, "StructuredOutput");
});

test("a missing token count reads 'not reported', never 0", () => {
  const [card] = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords()),
  ).cards;
  const gate = card.phases[1].agents[0];
  assert.deepEqual(gate.tokens, { kind: "not-reported" });
  assert.equal(
    formatCodingSessionAgentTokens(gate.tokens),
    "tokens not reported",
  );
  const reviewer = card.phases[2].agents[0];
  // A turn result with no usage block is not a zero.
  assert.deepEqual(reviewer.tokens, { kind: "not-reported" });
  assert.equal(reviewer.model, null);
  assert.equal(
    formatCodingSessionAgentModel(reviewer.model),
    "model not reported",
  );
  assert.doesNotMatch(formatCodingSessionAgentTokens(reviewer.tokens), /\b0\b/);
});

test("partial usage is a lower bound, not a total", () => {
  const items = transcript(target("x", "x-0000-4000-8000-000000000000"), [
    turnResult({ inputTokens: 1_000, outputTokens: 200 }),
    turnResult(),
    turnResult({ inputTokens: 500 }),
  ]);
  const tokens = readCodingSessionAgentTokens([items]);
  assert.deepEqual(tokens, {
    kind: "reported",
    total: 1_700,
    partial: true,
    reportedTurns: 2,
    turns: 3,
  });
  assert.equal(formatCodingSessionAgentTokens(tokens), "≥ 1.7k tok");
});

test("the newest open tool call wins; closed calls never count as current", () => {
  const items = transcript(target("y", "y-0000-4000-8000-000000000000"), [
    toolCall("t1", "Read"),
    toolCall("t2", "Grep"),
    toolResult("t2", "Grep"),
  ]);
  assert.equal(newestOpenToolName([items]), "Read");
  assert.equal(newestOpenToolName([[]]), null);
});

test("teammate: relay events alone give the same cards as the owner's view", () => {
  // The owner's machine runs the seats and holds each provider's lease, so
  // its statuses come with reachability; the teammate's has no local provider
  // at all and folds the relay events alone. Both get the same cards.
  const [umbrella] = groupCodingSessionCatalog(missionRecords());
  const owner = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords(), {
      executions: umbrella.executions.map((execution) => ({
        execution,
        wireStatus: execution.activeGeneration.status,
        status: deriveCodingSessionExecutionStatus(execution, {
          known: true,
          reachable: true,
        }),
      })),
    }),
  );
  const teammate = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(structuredClone(missionRecords())),
  );
  assert.deepEqual(teammate, owner);
  assert.equal(teammate.cards[0].phases.length, 3);
  assert.equal(teammate.cards[0].settled, 2);
  const gate = teammate.cards[0].phases[1].agents[0];
  assert.equal(gate.currentTool, "StructuredOutput");
  assert.equal(gate.model, "gpt-6");
});

test("an open ruling turns its asker and the seat it holds amber, until answered", () => {
  const assignmentId = "f".repeat(64);
  const requestId = `${"e".repeat(63)}1`;
  const transactions = [
    {
      sourceEventId: assignmentId,
      type: "assignment",
      authorPubkey: "0".repeat(64),
      createdAt: 1_800_000_100,
      counterpartyPubkey: "b".repeat(64),
      parentEventId: null,
      summary: "Build the second half",
      decision: null,
      requiredAction: null,
      fileCount: null,
      testCount: null,
      unseated: false,
      assigneeRole: "builder",
    },
    {
      sourceEventId: requestId,
      type: "decision.request",
      authorPubkey: "c".repeat(64),
      createdAt: 1_800_000_200,
      counterpartyPubkey: null,
      parentEventId: null,
      summary: "Ship with the flaky test quarantined?",
      decision: null,
      requiredAction: null,
      fileCount: null,
      testCount: null,
      unseated: false,
    },
  ];
  const open = {
    requestId,
    heldOn: "founder",
    blocks: [assignmentId],
    answeredBy: null,
    answerId: null,
  };
  const states = (decisions) => {
    const model = deriveCodingSessionAgentsOrchestration(
      relayOnlyInput(missionRecords(), { transactions, decisions }),
    );
    return model.cards[0].phases.flatMap((phase) =>
      phase.agents.map((agent) => [agent.label, agent.state, agent.waitingOn]),
    );
  };
  assert.deepEqual(states([open]), [
    ["build r1", "done", null],
    ["build r2", "waiting", "ruling"],
    ["gate r1", "waiting", "ruling"],
    ["review r1", "failed", null],
  ]);
  assert.deepEqual(
    states([{ ...open, answeredBy: "0".repeat(64), answerId: "9".repeat(64) }]),
    [
      ["build r1", "done", null],
      ["build r2", "running", null],
      ["gate r1", "running", null],
      ["review r1", "failed", null],
    ],
  );
  // Without the signed rows nothing maps a ruling to a seat: no guess.
  const unread = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords(), { decisions: [open] }),
  );
  assert.equal(
    unread.cards[0].phases.some((phase) =>
      phase.agents.some((agent) => agent.state === "waiting"),
    ),
    false,
  );
  assert.equal(
    codingSessionAgentStateWord({ state: "waiting", waitingOn: "ruling" }),
    "Waiting on a ruling",
  );
});

test("a provider nobody can reach never reads running", () => {
  assert.equal(
    codingSessionAgentState({
      wireStatus: "running",
      status: {
        kind: "unknown",
        label: "No provider answering",
        attention: "unreachable",
      },
    }),
    "unknown",
  );
  assert.equal(
    codingSessionAgentState({
      wireStatus: "completed",
      status: { kind: "unknown", label: "No provider answering" },
    }),
    "done",
    "a signed terminal word stands whatever the reachability",
  );
  assert.equal(
    codingSessionAgentState({
      wireStatus: "waiting_for_input",
      status: codingSessionWireWorkspaceStatus("waiting_for_input"),
    }),
    "waiting",
  );
});

test("an assignment adds a phase nobody holds; declared work says 'declared'", () => {
  const model = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(missionRecords(), {
      transactions: [
        {
          sourceEventId: "f".repeat(64),
          type: "assignment",
          authorPubkey: "e".repeat(64),
          createdAt: 1_800_000_100,
          counterpartyPubkey: "9".repeat(64),
          parentEventId: null,
          summary: "Fix the findings",
          decision: null,
          requiredAction: null,
          fileCount: null,
          testCount: null,
          unseated: false,
          assigneeRole: "fixer",
        },
      ],
      declaredWork: [
        {
          workId: "11111111-2222-4333-8444-555555555555",
          declarationRef: "a".repeat(64),
          planRef: {
            repository: "30617:x:y",
            commit: "c".repeat(40),
            path: "plans/publish-notes.md",
          },
          state: "head",
          supersedes: [],
          supersededBy: [],
          stateReasonCode: null,
          stateReason: null,
          planResolved: true,
          planDrift: {
            declaredCommit: "c".repeat(40),
            currentCommit: null,
            state: "unknown",
          },
          candidateArtifact: null,
          artifactCommits: [],
          criteria: [
            {
              criterionId: "notes",
              accept: "Notes published",
              proof: { kind: "review" },
              status: "open",
              assignmentRefs: [],
              evidence: [],
              artifactCommit: null,
              reasonCode: null,
              reason: null,
            },
          ],
          coverageComplete: false,
          coverageReasonCode: null,
          coverageReason: null,
        },
      ],
    }),
  );
  const phases = model.cards[0].phases;
  assert.deepEqual(
    phases.map((phase) => [phase.title, phase.origin, phase.state]),
    [
      ["Builder", "seat", "running"],
      ["Gate", "seat", "running"],
      ["Reviewer", "seat", "failed"],
      ["Fixer", "assigned", "pending"],
      ["Publish notes", "declared", "pending"],
    ],
  );
  assert.equal(
    formatCodingSessionPhaseCounts(phases[3]),
    "assigned · no seat yet",
  );
  assert.equal(
    formatCodingSessionPhaseCounts(phases[4]),
    "declared · not assigned",
  );
  assert.equal(model.cards[0].total, 4, "a phase with no seat adds no agent");
  assert.equal(model.assignmentsRead, true);
  assert.equal(model.declaredWorkRead, true);
});

test("a session with no roles and no team data has no card", () => {
  const records = missionRecords().map((entry) => ({ ...entry, role: null }));
  const model = deriveCodingSessionAgentsOrchestration(relayOnlyInput(records));
  assert.equal(model.cards.length, 0);
  assert.equal(model.assignmentsRead, false);
  assert.equal(model.declaredWorkRead, false);
});

test("direct spawns carry duration and the first line of the result", () => {
  const seat = target("s", "s-0000-4000-8000-000000000000");
  const items = transcript(seat, [
    {
      kind: "tool_call",
      tool: {
        toolName: "Task",
        toolKind: "think",
        toolId: "task-1",
        input: {
          description: "Survey memory papers",
          prompt: "Go",
          subagent_type: "general-purpose",
        },
      },
    },
    {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "Task",
      content:
        "I found measured numbers for all five questions.\nDetails follow.",
      isError: false,
    },
  ]);
  const model = deriveCodingSessionAgentsOrchestration({
    ...relayOnlyInput(missionRecords()),
    subagents: deriveCodingSessionSubagentPanel([items]),
  });
  assert.equal(model.spawns.length, 1);
  const [spawn] = model.spawns;
  assert.equal(spawn.title, "Survey memory papers");
  assert.equal(spawn.type, "general-purpose");
  assert.equal(spawn.status, "done");
  assert.equal(spawn.durationMs, 1_000);
  assert.equal(
    spawn.preview,
    "I found measured numbers for all five questions.",
  );
  assert.equal(
    formatCodingSessionSpawnMeta(spawn),
    "model not reported · tokens not reported · tools not reported",
  );
});

test("a failed or stopped phase never reads as done under a check", () => {
  const records = missionRecords().map((entry) =>
    entry.role === "builder" ? { ...entry, status: "completed" } : entry,
  );
  records.push(
    record({
      actor: "e".repeat(64),
      instanceId: "gate-2",
      model: "gpt-6",
      provider: "5".repeat(64),
      role: "gate",
      status: "stopped",
      items: [],
    }),
  );
  records[2] = { ...records[2], status: "completed" };
  const [card] = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(records),
  ).cards;
  assert.deepEqual(
    card.phases.map((phase) => [
      phase.title,
      phase.state,
      formatCodingSessionPhaseCounts(phase),
    ]),
    [
      ["Builder", "done", "2 done"],
      ["Gate", "settled", "1 done · 1 stopped"],
      ["Reviewer", "failed", "1 failed"],
    ],
  );
  assert.equal(card.live, false);
  assert.equal(card.settled, 5);
  assert.equal(card.failed, 1);
});

test("a settled seat shows its outcome; a failure without detail says so", () => {
  const failing = record({
    actor: "d".repeat(64),
    instanceId: "review-2",
    model: null,
    provider: "4".repeat(64),
    role: "reviewer",
    status: "failed",
    items: [
      turnResult(),
      {
        kind: "result",
        subtype: "error_during_execution",
        isError: true,
        durationMs: 10,
        result: "The gate command exited 101.\nstderr follows",
      },
    ],
  });
  const records = [...missionRecords(), failing];
  const [card] = deriveCodingSessionAgentsOrchestration(
    relayOnlyInput(records),
  ).cards;
  const [done, running] = card.phases[0].agents;
  assert.deepEqual(done.outcome, { text: "Done.", failed: false });
  assert.equal(
    running.outcome,
    null,
    "a live seat shows its tool, not an outcome",
  );
  const [silent, explained] = card.phases[2].agents;
  // The silent reviewer's only result is a success; under a failed status it
  // is not shown as the outcome.
  assert.deepEqual(silent.outcome, {
    text: "No failure detail reported",
    failed: true,
  });
  assert.deepEqual(explained.outcome, {
    text: "The gate command exited 101.",
    failed: true,
  });
  assert.equal(readCodingSessionAgentOutcome("running", []), null);
});

test("phase titles read as words", () => {
  assert.equal(codingSessionPhaseTitle("code-review"), "Code review");
  assert.equal(codingSessionPhaseTitle("gate"), "Gate");
});
