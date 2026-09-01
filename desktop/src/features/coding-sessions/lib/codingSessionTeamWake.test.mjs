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
  observeCodingSessionTeamWake,
  clearCodingSessionTeamWakePublishFailure,
  codingSessionTeamWakeFallbackNotBefore,
  codingSessionTeamWakeEvidenceIsComplete,
  codingSessionTeamWakePublishFailed,
  recordCodingSessionTeamWakeCustody,
  recordCodingSessionTeamWakePublishFailure,
  releaseCodingSessionTeamWakeCustody,
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
      sourceActorPubkey: BUILDER_ACTOR,
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

test("a differently named provider wake suppresses the Desktop fallback by operation id", () => {
  const candidate = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence({ withReport: true }),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates[0];
  assert.equal(
    pendingCodingSessionTeamWake({
      state: baselineCodingSessionTeamWakes([]),
      candidates: [candidate],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      acknowledgedSourceEventIds: new Set([candidate.sourceEventId]),
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

test("provider-first fallback grace is persisted from local observation, never source time", () => {
  const candidate = {
    ...deriveCodingSessionTeamWakePlan({
      umbrella: umbrella(),
      evidence: evidence(),
      currentUserPubkey: FOUNDER,
      sessionClosed: false,
    }).candidates[0],
    sourceCreatedAtMs: 1,
  };
  const state = observeCodingSessionTeamWake(
    baselineCodingSessionTeamWakes([]),
    candidate.sourceEventId,
    50_000,
  );
  assert.equal(
    codingSessionTeamWakeFallbackNotBefore(state, candidate.sourceEventId),
    50_000,
  );
  assert.equal(
    observeCodingSessionTeamWake(state, candidate.sourceEventId, 99_000),
    state,
    "a re-render cannot move an already persisted deadline",
  );
  const storage = new MemoryStorage();
  writeCodingSessionTeamWakeState(storage, "grace", state);
  assert.equal(
    codingSessionTeamWakeFallbackNotBefore(
      readCodingSessionTeamWakeState(storage, "grace"),
      candidate.sourceEventId,
    ),
    50_000,
    "restart preserves the locally observed grace deadline",
  );
});

test("provider operation acknowledgement remains resolved after its transcript echo ages out", () => {
  const candidate = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates[0];
  const observed = observeCodingSessionTeamWake(
    baselineCodingSessionTeamWakes([]),
    candidate.sourceEventId,
    50_000,
  );
  const resolved = acknowledgeCodingSessionTeamWakes(
    observed,
    new Set(),
    new Set([candidate.sourceEventId]),
  );
  assert.deepEqual(resolved.observed, []);
  assert.deepEqual(resolved.resolvedSourceEventIds, [candidate.sourceEventId]);
  assert.equal(
    pendingCodingSessionTeamWake({
      state: resolved,
      candidates: [candidate],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      acknowledgedSourceEventIds: new Set(),
      attemptedThisMount: new Set(),
    }),
    null,
    "the durable source ledger survives after the bounded echo projection forgets it",
  );
});

test("failed or saturated evidence cannot initialize or migrate the durable wake ledger", () => {
  assert.equal(
    codingSessionTeamWakeEvidenceIsComplete({
      isLoading: false,
      errorMessage: "Mission evidence overflowed its exact bound.",
    }),
    false,
  );
  assert.equal(
    codingSessionTeamWakeEvidenceIsComplete({
      isLoading: false,
      errorMessage: null,
    }),
    true,
  );
});

test("author timestamps never decide discovery after the legacy cursor is migrated", () => {
  const template = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence(),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates[0];
  const future = {
    ...template,
    sourceEventId: "e".repeat(64),
    sourceCreatedAtMs: 99_000,
  };
  const past = {
    ...template,
    sourceEventId: "1".repeat(64),
    sourceCreatedAtMs: 1,
  };
  const attempted = recordCodingSessionTeamWakeAttempt({
    state: baselineCodingSessionTeamWakes([]),
    candidate: future,
    commandId: "future-wake",
    leadTarget: LEAD_TARGET,
    acknowledgedCommandIds: new Set(),
  });
  const resolved = acknowledgeCodingSessionTeamWakes(
    attempted,
    new Set(["future-wake"]),
  );
  assert.equal(resolved.cursor, null);
  assert.equal(
    pendingCodingSessionTeamWake({
      state: resolved,
      candidates: [future, past],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      attemptedThisMount: new Set(),
    }).sourceEventId,
    past.sourceEventId,
  );
});

test("D-T1 (model): a custodied source is never a candidate; only a start resolves it", () => {
  const candidate = deriveCodingSessionTeamWakePlan({
    umbrella: umbrella(),
    evidence: evidence({ withReport: true }),
    currentUserPubkey: FOUNDER,
    sessionClosed: false,
  }).candidates.find((row) => row.kind === "operation_ready");
  const observed = observeCodingSessionTeamWake(
    baselineCodingSessionTeamWakes([]),
    candidate.sourceEventId,
    50_000,
  );
  const suppressed = new Set([candidate.sourceEventId]);
  assert.equal(
    pendingCodingSessionTeamWake({
      state: observed,
      candidates: [candidate],
      leadTarget: LEAD_TARGET,
      acknowledgedCommandIds: new Set(),
      acknowledgedSourceEventIds: new Set(),
      suppressedSourceEventIds: suppressed,
      attemptedThisMount: new Set(),
    }),
    null,
    "a provider command already queued for this pointer suppresses the Desktop fallback",
  );
  const custodied = recordCodingSessionTeamWakeCustody(observed, {
    sourceEventId: candidate.sourceEventId,
    leadTargetKey: buildCodingSessionTargetKey(LEAD_TARGET),
    commandId: "provider-cmd",
  });
  assert.deepEqual(
    custodied.resolvedSourceEventIds,
    [],
    "custody is not permanent resolution",
  );
  assert.deepEqual(custodied.custodied, [
    {
      sourceEventId: candidate.sourceEventId,
      leadTargetKey: buildCodingSessionTargetKey(LEAD_TARGET),
      commandId: "provider-cmd",
      fromProvider: true,
    },
  ]);
  assert.equal(
    recordCodingSessionTeamWakeCustody(custodied, {
      sourceEventId: candidate.sourceEventId,
      leadTargetKey: buildCodingSessionTargetKey(LEAD_TARGET),
      commandId: "provider-cmd",
    }),
    custodied,
    "custody is idempotent per source, target and command",
  );

  const acknowledged = acknowledgeCodingSessionTeamWakes(
    custodied,
    new Set(),
    new Set(),
    suppressed,
  );
  assert.deepEqual(
    acknowledged.resolvedSourceEventIds,
    [candidate.sourceEventId],
    "a started or echoed command is what writes the permanent ledger",
  );
  assert.deepEqual(
    acknowledged.custodied,
    [],
    "permanent resolution retires the custody row",
  );
  assert.deepEqual(acknowledged.observed, []);

  const released = releaseCodingSessionTeamWakeCustody(custodied, {
    sourceEventId: candidate.sourceEventId,
    leadTargetKey: buildCodingSessionTargetKey(LEAD_TARGET),
  });
  assert.deepEqual(released.custodied, []);
  assert.deepEqual(released.resolvedSourceEventIds, []);
});

test("D-T2 (state): a publish that threw is durable and clears when it succeeds", () => {
  const targetKey = buildCodingSessionTargetKey(LEAD_TARGET);
  const failed = recordCodingSessionTeamWakePublishFailure(
    baselineCodingSessionTeamWakes([]),
    { sourceEventId: REPORT_ID, leadTargetKey: targetKey },
  );
  assert.equal(
    codingSessionTeamWakePublishFailed(failed, REPORT_ID, targetKey),
    true,
  );
  const storage = new MemoryStorage();
  writeCodingSessionTeamWakeState(storage, "pf", failed);
  assert.equal(
    codingSessionTeamWakePublishFailed(
      readCodingSessionTeamWakeState(storage, "pf"),
      REPORT_ID,
      targetKey,
    ),
    true,
    "the failure survives a restart, so the row never reads Waiting",
  );
  const cleared = clearCodingSessionTeamWakePublishFailure(failed, {
    sourceEventId: REPORT_ID,
    leadTargetKey: targetKey,
  });
  assert.equal(
    codingSessionTeamWakePublishFailed(cleared, REPORT_ID, targetKey),
    false,
  );
});

test("D-T6 (state): a v1 blob migrates to v2 with an empty re-arm ledger", () => {
  const storage = new MemoryStorage();
  storage.setItem(
    "key",
    JSON.stringify({
      schema: "buzz-coding-session-team-wake-state/v1",
      cursor: null,
      pending: [],
      observed: [],
      resolvedSourceEventIds: [REPORT_ID],
    }),
  );
  const state = readCodingSessionTeamWakeState(storage, "key");
  assert.deepEqual(state.reArmed, [], "v1 gains an empty re-arm ledger");
  assert.deepEqual(state.custodied, [], "v1 gains an empty custody ledger");
  assert.deepEqual(state.publishFailed, []);
  assert.deepEqual(state.resolvedSourceEventIds, [REPORT_ID]);
  assert.equal(state.schema, "buzz-coding-session-team-wake-state/v2");
});
