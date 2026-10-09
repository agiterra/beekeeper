import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionStreamPresence } from "./codingSessionStreamPresence.ts";

const PROVIDER = "ab".repeat(32);
const LEAD_ACTOR = "11".repeat(32);
const BUILDER_ACTOR = "22".repeat(32);
const VERIFIER_ACTOR = "33".repeat(32);
const START = Date.parse("2026-08-30T15:00:00.000Z");

function record({
  actor,
  executionKey,
  model = "gpt-5.6-sol",
  role,
  runtime = "codex-acp",
  status,
  transcript = [],
}) {
  return {
    executionKey,
    signerPubkey: PROVIDER,
    operatorPubkey: null,
    priorGenerations: [],
    activeGeneration: {
      generationId: `${executionKey}-generation`,
      label: executionKey,
      title: "Portable team loop",
      providerAuthorityPubkey: PROVIDER,
      metadataAuthorityPubkey: PROVIDER,
      lastEventAt: "2026-08-30T15:03:18.000Z",
      status,
      statusAt: START,
      transcript,
      conflictCount: 0,
      commandTarget: null,
      projectRef: null,
      repoRef: null,
      sessionRef: "mission-1",
      provider: runtime,
      runtime,
      model,
      agentRef: actor,
      role,
      turnBudget: null,
      routing: null,
      capabilities: null,
    },
  };
}

function umbrella(executions) {
  return {
    umbrellaKey: "mission-1",
    sessionRef: "mission-1",
    title: "Portable team loop",
    executions,
    founderPubkey: null,
    genesisRef: null,
    genesisResolution: "legacy",
    status: "running",
    lastEventAt: "2026-08-30T15:03:18.000Z",
    conflictCount: 0,
    foreignAttachmentCount: 0,
  };
}

function openTurn() {
  return [
    {
      id: "prompt",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "Build the stream foundation.",
      timestamp: "2026-08-30T15:00:00.000Z",
      turnId: "turn-1",
    },
    {
      id: "plan",
      type: "plan",
      renderClass: "plan",
      title: "Plan",
      text: "- [ ] Implement participant and activity bars with a deliberately long sentence (in progress)",
      timestamp: "2026-08-30T15:00:05.000Z",
      turnId: "turn-1",
    },
    {
      id: "tool-1",
      type: "tool",
      renderClass: "file-read",
      descriptor: { renderClass: "file-read", label: "Read", preview: null },
      title: "Read file",
      toolName: "Read",
      beekeeperToolName: null,
      status: "completed",
      args: {},
      result: "",
      isError: false,
      timestamp: "2026-08-30T15:00:06.000Z",
      startedAt: "2026-08-30T15:00:06.000Z",
      completedAt: "2026-08-30T15:00:07.000Z",
      turnId: "turn-1",
    },
    {
      id: "tool-2",
      type: "tool",
      renderClass: "file-edit",
      descriptor: { renderClass: "file-edit", label: "Edit", preview: null },
      title: "Edited file",
      toolName: "Edit",
      beekeeperToolName: null,
      status: "completed",
      args: {},
      result: "",
      isError: false,
      timestamp: "2026-08-30T15:00:10.000Z",
      startedAt: "2026-08-30T15:00:10.000Z",
      completedAt: "2026-08-30T15:00:11.000Z",
      turnId: "turn-1",
    },
  ];
}

test("the participant bar leads with team identity and never provider keys", () => {
  const lead = record({
    actor: LEAD_ACTOR,
    executionKey: "lead",
    role: "lead",
    status: "completed",
  });
  const builder = record({
    actor: BUILDER_ACTOR,
    executionKey: "builder",
    role: "builder",
    runtime: "claude-agent-acp",
    model: "sonnet",
    status: "running",
    transcript: openTurn(),
  });
  const verifier = record({
    actor: VERIFIER_ACTOR,
    executionKey: "verifier",
    role: "verifier",
    status: "waiting_for_input",
  });
  const statuses = {
    lead: { kind: "idle", label: "Idle" },
    builder: { kind: "working", label: "Working" },
    verifier: { kind: "waiting", label: "Waiting" },
  };
  const names = new Map([
    [LEAD_ACTOR, "Helios"],
    [BUILDER_ACTOR, "Bob"],
  ]);

  const presence = deriveCodingSessionStreamPresence({
    umbrella: umbrella([builder, verifier, lead]),
    resolveStatus: (execution) => statuses[execution.executionKey],
    resolveActorName: (actor) => names.get(actor) ?? null,
    canSteer: true,
    nowMs: START + 198_000,
  });

  assert.deepEqual(
    presence.participants.map((item) => item.label),
    ["Helios · Lead", "Bob · Builder", "Verifier · Codex · gpt-5.6-sol"],
  );
  assert.deepEqual(
    presence.participants.map((item) => item.disposition),
    ["idle", "live", "waiting for you"],
  );
  assert.equal(presence.participants[1].activity.length, 48);
  assert.match(presence.participants[1].activity, /…$/);
  assert.equal(presence.participants[2].activity, null);
  assert.doesNotMatch(JSON.stringify(presence), new RegExp(PROVIDER));
  assert.doesNotMatch(JSON.stringify(presence), new RegExp(LEAD_ACTOR));
});

test("live activity counts the open signed turn and omits every absent clause", () => {
  const builder = record({
    actor: BUILDER_ACTOR,
    executionKey: "builder",
    role: "builder",
    status: "running",
    transcript: openTurn(),
  });
  const silent = record({
    actor: LEAD_ACTOR,
    executionKey: "silent",
    role: "lead",
    status: "running",
  });
  const idle = record({
    actor: VERIFIER_ACTOR,
    executionKey: "idle",
    role: "verifier",
    status: "completed",
  });
  const presence = deriveCodingSessionStreamPresence({
    umbrella: umbrella([builder, silent, idle]),
    resolveStatus: (execution) =>
      execution.executionKey === "idle"
        ? { kind: "idle", label: "Idle" }
        : { kind: "working", label: "Working" },
    resolveActorName: (actor) =>
      actor === BUILDER_ACTOR ? "Bob" : actor === LEAD_ACTOR ? "Helios" : null,
    nowMs: START + 198_000,
  });

  assert.deepEqual(
    presence.liveActivity.map((item) => item.executionKey),
    ["silent", "builder"],
  );
  assert.equal(presence.liveActivity[0].startedAtMs, null);
  assert.equal(presence.liveActivity[0].openToolCount, null);
  assert.equal(presence.liveActivity[0].activity, null);
  assert.equal(presence.liveActivity[1].startedAtMs, START);
  assert.equal(presence.liveActivity[1].openToolCount, 2);
});

test("a trusted turn_started receipt outranks the transcript timestamp", () => {
  const builder = record({
    actor: BUILDER_ACTOR,
    executionKey: "builder",
    role: "builder",
    status: "running",
    transcript: openTurn(),
  });
  const receiptStartedAtMs = START + 42_000;
  const presence = deriveCodingSessionStreamPresence({
    umbrella: umbrella([builder]),
    resolveStatus: () => ({ kind: "working", label: "Working" }),
    resolveTurnStartedAt: (execution, turnId) => {
      assert.equal(execution.executionKey, "builder");
      assert.equal(turnId, "turn-1");
      return receiptStartedAtMs;
    },
    nowMs: START + 198_000,
  });
  assert.equal(presence.liveActivity[0].startedAtMs, receiptStartedAtMs);
});

test("an absent trusted receipt falls back to the earliest transcript item", () => {
  const builder = record({
    actor: BUILDER_ACTOR,
    executionKey: "builder",
    role: "builder",
    status: "running",
    transcript: openTurn(),
  });
  const presence = deriveCodingSessionStreamPresence({
    umbrella: umbrella([builder]),
    resolveStatus: () => ({ kind: "working", label: "Working" }),
    resolveTurnStartedAt: () => null,
  });
  assert.equal(presence.liveActivity[0].startedAtMs, START);
});

test("a terminal item closes the activity clauses even when metadata still says live", () => {
  const transcript = [
    ...openTurn(),
    {
      id: "result",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "Completed",
      timestamp: "2026-08-30T15:00:12.000Z",
      turnId: "turn-1",
    },
  ];
  const builder = record({
    actor: BUILDER_ACTOR,
    executionKey: "builder",
    role: "builder",
    status: "running",
    transcript,
  });
  const [activity] = deriveCodingSessionStreamPresence({
    umbrella: umbrella([builder]),
    resolveStatus: () => ({ kind: "working", label: "Working" }),
    nowMs: START + 198_000,
  }).liveActivity;
  assert.equal(activity.activity, null);
  assert.equal(activity.startedAtMs, null);
  assert.equal(activity.openToolCount, null);
});
