import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import {
  acknowledgeCodingSessionTeamWakes,
  baselineCodingSessionTeamWakes,
  buildCodingSessionTeamWakeCommandId,
  codingSessionTeamWakeText,
  deriveCodingSessionTeamWakePlan,
  pendingCodingSessionTeamWake,
  readCodingSessionTeamWakeState,
  recordCodingSessionTeamWakeAttempt,
  writeCodingSessionTeamWakeState,
} from "./codingSessionTeamWake.ts";

const FOUNDER = "a".repeat(64);
const LEAD_ACTOR = "b".repeat(64);
const BUILDER_ACTOR = "c".repeat(64);
const ASSIGNMENT_ID = "d".repeat(64);
const REPORT_ID = "e".repeat(64);
const TERMINAL_ID = "f".repeat(64);
const LEAD_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-primary",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const BUILDER_TARGET = {
  driver: "claude-acp",
  instanceId: "claude-primary",
  sessionId: "66666666-7777-8888-9999-aaaaaaaaaaaa",
  generation: 1,
};

function generation({
  role,
  actor,
  target,
  transcript = [],
  generationId = role,
}) {
  return {
    generationId,
    label: role,
    title: role,
    providerAuthorityPubkey: "9".repeat(64),
    metadataAuthorityPubkey: "9".repeat(64),
    lastEventAt: "2026-08-31T04:00:02.000Z",
    status: "idle",
    statusAt: 1_777_777_602_000,
    statusEventId: "8".repeat(64),
    transcript,
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef: "session-1",
    provider: null,
    runtime: null,
    model: null,
    agentRef: actor,
    role,
    turnBudget: null,
    routing: null,
    capabilities: null,
  };
}

function execution({ role, actor, target, transcript = [], suffix = role }) {
  return {
    executionKey: suffix,
    signerPubkey: "9".repeat(64),
    activeGeneration: generation({
      role,
      actor,
      target,
      transcript,
      generationId: suffix,
    }),
    priorGenerations: [],
    operatorPubkey: FOUNDER,
  };
}

function terminalTranscript() {
  return [
    {
      id: "prompt",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "do the assignment",
      timestamp: "2026-08-31T04:00:00.000Z",
      turnId: "turn-1",
      sessionId: "builder",
      channelId: "channel-1",
      commandId: "assignment-command",
    },
    {
      id: "terminal",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "READY",
      timestamp: "2026-08-31T04:00:02.000Z",
      turnId: "turn-1",
      sessionId: "builder",
      channelId: "channel-1",
      sourceEventId: TERMINAL_ID,
      sourceEventSeq: 12,
      sourceTargetKey: buildCodingSessionTargetKey(BUILDER_TARGET),
    },
  ];
}

function umbrella({
  builderTranscript = terminalTranscript(),
  extraLead = false,
} = {}) {
  const executions = [
    execution({
      role: "lead",
      actor: LEAD_ACTOR,
      target: LEAD_TARGET,
      suffix: "lead",
    }),
    execution({
      role: "builder",
      actor: BUILDER_ACTOR,
      target: BUILDER_TARGET,
      transcript: builderTranscript,
      suffix: "builder",
    }),
  ];
  if (extraLead) {
    executions.push(
      execution({
        role: "LEAD",
        actor: "7".repeat(64),
        target: { ...LEAD_TARGET, generation: 2 },
        suffix: "lead-2",
      }),
    );
  }
  return {
    umbrellaKey: "session-1",
    sessionRef: "session-1",
    title: "TeamTest",
    executions,
    founderPubkey: FOUNDER,
    genesisRef: "6".repeat(64),
    genesisResolution: "governed",
    status: "idle",
    lastEventAt: "2026-08-31T04:00:02.000Z",
    conflictCount: 0,
    foreignAttachmentCount: 0,
  };
}

function evidence({ withReport = false, deliveryCommandId = null } = {}) {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    assignments: withReport
      ? [
          {
            sourceEventId: ASSIGNMENT_ID,
            authorLabel: FOUNDER,
            assigneeRole: "builder",
            objective: "Build it",
            brief: "Exact brief",
            fileOwnership: [],
          },
        ]
      : [],
    seatPlans: [],
    reports: withReport
      ? [
          {
            sourceEventId: REPORT_ID,
            authorPubkey: BUILDER_ACTOR,
            sourceCreatedAt: 1_788_148_801,
            deliveryCommandId,
            authorLabel: BUILDER_ACTOR,
            summary: "done",
            assignmentRef: ASSIGNMENT_ID,
            branch: null,
            baseSha: null,
            headSha: null,
            files: [],
            tests: [],
          },
        ]
      : [],
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    missionState: { kind: "unknown", detail: null },
    usage: null,
    rejectedEventCount: 0,
    rejectionsTruncated: false,
    rejectedReasons: [],
    conflicts: [],
  };
}

class MemoryStorage {
  values = new Map();
  getItem(key) {
    return this.values.get(key) ?? null;
  }
  setItem(key, value) {
    this.values.set(key, value);
  }
}

test("READY plus idle is a diagnostic terminal wake, never completion", () => {
  const plan = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  });
  assert.equal(plan.lead.executionKey, "lead");
  assert.deepEqual(plan.candidates, [
    {
      sourceEventId: TERMINAL_ID,
      sourceCreatedAtMs: 1_788_148_802_000,
      sourceEventSeq: 12,
      sourceTargetKey: buildCodingSessionTargetKey(BUILDER_TARGET),
      kind: "turn_ended_without_required_operation",
      operationType: null,
      seatRole: "builder",
      causedByCommandId: "assignment-command",
      preferredCommandId: null,
    },
  ]);
  assert.equal(
    codingSessionTeamWakeText(plan.candidates[0]),
    JSON.stringify({
      schema: "buzz-team-wake/v1",
      type: "turn_ended_without_required_operation",
      terminalEventId: TERMINAL_ID,
      seatRole: "builder",
      causedByCommandId: "assignment-command",
    }),
  );
});

test("a canonical report suppresses the turn diagnostic and emits an exact operation pointer", () => {
  const plan = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence({ withReport: true, deliveryCommandId: "shared-id" }),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  });
  assert.equal(plan.candidates.length, 1);
  assert.equal(plan.candidates[0].kind, "operation_ready");
  assert.equal(plan.candidates[0].preferredCommandId, "shared-id");
  assert.equal(
    codingSessionTeamWakeText(plan.candidates[0]),
    JSON.stringify({ operationId: REPORT_ID, type: "report" }),
  );
});

test("only the founder and exactly one normalized lead may arm delivery", () => {
  const notFounder = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: "1".repeat(64),
    sessionClosed: false,
  });
  assert.equal(notFounder.lead, null);
  assert.deepEqual(notFounder.candidates, []);

  const ambiguous = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella({ extraLead: true }),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  });
  assert.equal(ambiguous.lead, null);
  assert.match(ambiguous.refusal, /more than one/);
});

test("first observation baselines history; accepted wakes remain pending until signed echo", () => {
  const [candidate] = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates;
  const baseline = baselineCodingSessionTeamWakes([candidate]);
  assert.equal(
    pendingCodingSessionTeamWake({
      state: baseline,
      candidates: [candidate],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      attemptedThisMount: new Set(),
    }),
    null,
  );

  const newer = { ...candidate, sourceEventId: "1".repeat(64) };
  const attempted = recordCodingSessionTeamWakeAttempt({
    state: baseline,
    candidate: newer,
    commandId: "wake-1",
    leadTarget: LEAD_TARGET,
    acknowledgedCommandIds: new Set(),
  });
  assert.equal(attempted.pending.length, 1);
  assert.equal(
    pendingCodingSessionTeamWake({
      state: attempted,
      candidates: [candidate, newer],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      attemptedThisMount: new Set(),
    }).sourceEventId,
    newer.sourceEventId,
    "an unconfirmed relay-accepted wake retries once after remount",
  );
  assert.equal(
    acknowledgeCodingSessionTeamWakes(attempted, new Set(["wake-1"])).pending
      .length,
    0,
  );
});

test("an existing provider echo suppresses an already-addressed operation", () => {
  const candidate = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence({ withReport: true, deliveryCommandId: "shared-id" }),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates[0];
  assert.equal(
    pendingCodingSessionTeamWake({
      state: baselineCodingSessionTeamWakes([]),
      candidates: [candidate],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(["shared-id"]),
      attemptedThisMount: new Set(),
    }),
    null,
  );
});

test("lead generation changes get a distinct deterministic dedupe id", async () => {
  const candidate = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates[0];
  const first = await buildCodingSessionTeamWakeCommandId(
    candidate,
    LEAD_TARGET,
  );
  const second = await buildCodingSessionTeamWakeCommandId(candidate, {
    ...LEAD_TARGET,
    generation: 2,
  });
  assert.notEqual(first, second);
  assert.match(first, /^team-wake-v1:[0-9a-f]{64}:[0-9a-f]{24}$/);
});

test("durable state round-trips and malformed state fails closed", () => {
  const storage = new MemoryStorage();
  const state = baselineCodingSessionTeamWakes([]);
  writeCodingSessionTeamWakeState(storage, "key", state);
  assert.deepEqual(readCodingSessionTeamWakeState(storage, "key"), state);
  storage.setItem("key", '{"schema":"wrong","pending":[]}');
  assert.equal(readCodingSessionTeamWakeState(storage, "key"), null);

  storage.setItem(
    "key",
    JSON.stringify({
      schema: state.schema,
      cursor: null,
      pending: [
        {
          sourceEventId: REPORT_ID,
          commandId: "wake\nforged",
          leadTargetKey: "target",
          candidate: {
            sourceEventId: REPORT_ID,
            sourceCreatedAtMs: 1,
            sourceEventSeq: null,
            sourceTargetKey: null,
            kind: "operation_ready",
            operationType: null,
            seatRole: "builder",
            causedByCommandId: null,
            preferredCommandId: null,
          },
        },
      ],
    }),
  );
  assert.deepEqual(readCodingSessionTeamWakeState(storage, "key")?.pending, []);
});
