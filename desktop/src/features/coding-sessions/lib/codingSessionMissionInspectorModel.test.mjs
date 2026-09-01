import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionMissionInspectorModel } from "./codingSessionMissionInspectorModel.ts";
import { MISSION_INSPECTOR_LIMITS } from "./codingSessionMissionInspectorModel.ts";

function baseInput(overrides = {}) {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    seatPlans: [],
    reports: [],
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    missionState: { kind: "unknown", detail: null },
    usage: null,
    rejectedEventCount: 0,
    rejectionsTruncated: false,
    rejectedReasons: [],
    conflicts: [],
    ...overrides,
  };
}

function taskModel() {
  return {
    sourceItemId: "seat-plan-event",
    turnId: "turn-1",
    timestamp: "2026-08-30T16:00:00.000Z",
    tasks: [
      { id: "one", text: "Inspect signed inputs", status: "completed" },
      { id: "two", text: "Render the inspector", status: "in_progress" },
    ],
    completedCount: 1,
    explanation: "This is Bob's own plan.",
    copyText: null,
    state: "active",
  };
}

test("an accepted plan is never inferred from a seat-reported plan", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      seatPlans: [
        {
          executionKey: "builder",
          ownerLabel: "Bob · Builder",
          model: taskModel(),
        },
      ],
    }),
  );

  assert.equal(model.acceptedPlan.kind, "absent");
  assert.equal(model.acceptedPlan.label, "No accepted plan published");
  assert.equal(model.acceptedPlan.steps.length, 0);
  assert.equal(model.seatPlans[0].ownerLabel, "Bob · Builder");
  assert.equal(model.seatPlans[0].steps.length, 2);
  assert.equal(model.seatPlans[0].sourceEventId, "seat-plan-event");
});

test("signed assignments preserve bounded per-step provenance in deterministic order", () => {
  const ownership = Array.from(
    { length: 120 },
    (_, index) => `src/${index}.ts`,
  );
  const inputPlanSteps = [
    {
      text: "Run the signed smoke test",
      sourceEventId: "assignment-b",
      authorLabel: "Verifier",
      sourceCreatedAt: 123,
      sourceIndex: 7,
    },
    {
      text: "Implement the bridge",
      sourceEventId: "assignment-a",
      authorLabel: "Founder",
      sourceCreatedAt: 10,
      sourceIndex: 0,
    },
  ];
  const build = (steps) =>
    deriveCodingSessionMissionInspectorModel(
      baseInput({
        acceptedPlan: {
          kind: "available",
          steps,
        },
        assignments: [
          {
            sourceEventId: "assignment-a",
            authorLabel: "Founder",
            assigneeRole: "builder",
            objective: "Build the portable team loop",
            brief: "Preserve every accepted signed fact.",
            fileOwnership: ownership,
          },
          {
            sourceEventId: "assignment-b",
            authorLabel: "Verifier",
            assigneeRole: "verifier",
            objective: "Verify the portable team loop",
            brief: "Attack every accepted signed fact.",
            fileOwnership: [],
          },
        ],
      }),
    );
  const model = build(inputPlanSteps);
  const reversed = build([...inputPlanSteps].reverse());

  ownership[0] = "mutated";
  inputPlanSteps[0].text = "mutated";
  inputPlanSteps[0].sourceCreatedAt = 999;
  inputPlanSteps[0].sourceIndex = 999;
  assert.deepEqual(model.acceptedPlan, reversed.acceptedPlan);
  assert.deepEqual(model.acceptedPlan.sourceEventIds, [
    "assignment-a",
    "assignment-b",
  ]);
  assert.deepEqual(
    model.acceptedPlan.steps.map(
      ({
        id,
        text,
        status,
        sourceEventId,
        authorLabel,
        sourceCreatedAt,
        sourceIndex,
      }) => ({
        id,
        text,
        status,
        sourceEventId,
        authorLabel,
        sourceCreatedAt,
        sourceIndex,
      }),
    ),
    [
      {
        id: "assignment-a:accepted:0",
        text: "Implement the bridge",
        status: null,
        sourceEventId: "assignment-a",
        authorLabel: "Founder",
        sourceCreatedAt: 10,
        sourceIndex: 0,
      },
      {
        id: "assignment-b:accepted:7",
        text: "Run the signed smoke test",
        status: null,
        sourceEventId: "assignment-b",
        authorLabel: "Verifier",
        sourceCreatedAt: 123,
        sourceIndex: 7,
      },
    ],
  );
  assert.deepEqual(
    model.contextFacts.slice(0, 4).map(({ label, value, sourceEventId }) => ({
      label,
      value,
      sourceEventId,
    })),
    [
      {
        label: "Assignment role",
        value: "builder",
        sourceEventId: "assignment-a",
      },
      {
        label: "Objective",
        value: "Build the portable team loop",
        sourceEventId: "assignment-a",
      },
      {
        label: "Brief",
        value: "Preserve every accepted signed fact.",
        sourceEventId: "assignment-a",
      },
      {
        label: "Ownership",
        value: "src/0.ts",
        sourceEventId: "assignment-a",
      },
    ],
  );
  assert.equal(
    model.contextFacts.filter(({ label }) => label === "Ownership").length,
    MISSION_INSPECTOR_LIMITS.ownershipFilesPerAssignment,
  );
  assert.ok(
    model.truncations.some(
      ({ id }) => id === "context:assignment ownership paths",
    ),
  );
});

test("structured report facts remain attributed and are not blended with observed edits", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      reports: [
        {
          sourceEventId: "report-event",
          authorLabel: "Bob · Builder",
          summary: "Inspector foundation complete.",
          assignmentRef: "assignment-event",
          branch: "singularity-stream",
          baseSha: "3001e46a",
          headSha: "860a0af7",
          files: ["reported-only.ts", "observed.ts"],
          tests: [
            {
              name: "focused model test",
              command: "node --test model.test.mjs",
              outcome: "passed",
              evidence: "3 passed",
            },
          ],
        },
      ],
      observedChanges: {
        files: [
          {
            path: "observed.ts",
            filename: "observed.ts",
            additions: 12,
            deletions: 3,
            diffs: [],
            editCount: 2,
          },
        ],
        unreportedEditCount: 1,
      },
    }),
  );

  assert.deepEqual(model.tests[0], {
    id: "report-event:test:0",
    name: "focused model test",
    command: "node --test model.test.mjs",
    outcome: "passed",
    evidence: "3 passed",
    sourceEventId: "report-event",
    authorLabel: "Bob · Builder",
  });
  assert.equal(model.changes.state, "mixed");
  assert.equal(model.changes.namedEditCount, 2);
  assert.equal(model.changes.unreportedEditCount, 1);
  assert.equal(model.changes.additions, 12);
  assert.deepEqual(model.files, [
    {
      path: "observed.ts",
      observed: true,
      observedSourceEventIds: [],
      observedSourceKnown: false,
      reportedBy: [
        { authorLabel: "Bob · Builder", sourceEventId: "report-event" },
      ],
      additions: 12,
      deletions: 3,
      editCount: 2,
    },
    {
      path: "reported-only.ts",
      observed: false,
      observedSourceEventIds: [],
      observedSourceKnown: false,
      reportedBy: [
        { authorLabel: "Bob · Builder", sourceEventId: "report-event" },
      ],
      additions: null,
      deletions: null,
      editCount: null,
    },
  ]);
  assert.deepEqual(
    model.contextFacts.map(({ label, value }) => [label, value]),
    [
      ["Assignment", "assignment-event"],
      ["Branch", "singularity-stream"],
      ["Base", "3001e46a"],
      ["Head", "860a0af7"],
    ],
  );
});

test("unknown line counts suppress aggregate additions and deletions", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      observedChanges: {
        files: [
          {
            path: "known.ts",
            filename: "known.ts",
            additions: 2,
            deletions: 1,
            diffs: [],
            editCount: 1,
          },
          {
            path: "unknown.ts",
            filename: "unknown.ts",
            additions: null,
            deletions: null,
            diffs: [],
            editCount: 1,
          },
        ],
        unreportedEditCount: 0,
      },
    }),
  );
  assert.equal(model.changes.state, "named");
  assert.equal(model.changes.additions, null);
  assert.equal(model.changes.deletions, null);
});

test("conflicts, rejected records, unknown state, and absent usage remain explicit", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      acceptedPlan: { kind: "conflict", eventIds: ["plan-a", "plan-b"] },
      missionState: { kind: "conflict", eventIds: ["done", "blocked"] },
      usage: {
        sourceEventId: "empty-usage",
        inputTokens: null,
        outputTokens: null,
        totalTokens: null,
        toolCalls: null,
        costUsd: null,
      },
      rejectedEventCount: 2,
      rejectedReasons: [
        {
          code: "invalid-causal-ref",
          summary: "Unknown cause",
          eventIds: ["bad"],
        },
      ],
      conflicts: [
        {
          code: "mission-terminal",
          summary: "Terminal records disagree",
          eventIds: ["done", "blocked"],
        },
      ],
    }),
  );
  assert.equal(model.acceptedPlan.kind, "conflict");
  assert.deepEqual(model.acceptedPlan.sourceEventIds, ["plan-a", "plan-b"]);
  assert.equal(model.missionState.kind, "conflict");
  assert.equal(model.usage, null);
  assert.equal(model.integrity.rejectedEventCount, 2);
  assert.equal(model.integrity.rejectedReasons[0].code, "invalid-causal-ref");
  assert.equal(model.integrity.conflicts[0].code, "mission-terminal");
});

test("an overflowed rejection set preserves an unknown total", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      rejectedEventCount: null,
      rejectionsTruncated: true,
      rejectedReasons: Array.from({ length: 100 }, (_, index) => ({
        code: `rejected-${index}`,
        summary: `Rejected event ${index}`,
        eventIds: [`event-${index}`],
      })),
    }),
  );
  assert.equal(model.integrity.rejectedEventCount, null);
  assert.equal(model.integrity.rejectionsTruncated, true);
  assert.equal(model.integrity.rejectedReasons.length, 100);
});

test("no projection facts become zeros or synthetic completion", () => {
  const model = deriveCodingSessionMissionInspectorModel(baseInput());
  assert.equal(model.missionState.kind, "unknown");
  assert.equal(model.changes.state, "none");
  assert.equal(model.tests.length, 0);
  assert.equal(model.files.length, 0);
  assert.equal(model.contextFacts.length, 0);
  assert.equal(model.usage, null);
});

test("huge outer and nested collections are copied, bounded, and disclosed", () => {
  const participant = (index) => ({
    executionKey: `seat-${index}`,
    label: `Seat ${index}`,
    secondaryLabel: null,
    role: "builder",
    status: { kind: "idle", label: "Idle" },
    disposition: "idle",
    activity: null,
    lastTurnLabel: "last turn just now",
  });
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      goal: {
        kind: "conflict",
        eventIds: Array.from({ length: 50 }, (_, index) => `goal-${index}`),
      },
      acceptedPlan: {
        kind: "available",
        steps: Array.from({ length: 150 }, (_, index) => ({
          text: `step-${index}`,
          sourceEventId: `accepted-${index}`,
          authorLabel: `Lead ${index}`,
          sourceCreatedAt: index,
          sourceIndex: 0,
        })),
      },
      seatPlans: Array.from({ length: 50 }, (_, index) => ({
        executionKey: `seat-${index}`,
        ownerLabel: `Seat ${index}`,
        model: {
          ...taskModel(),
          sourceItemId: `plan-${index}`,
          tasks: Array.from({ length: 150 }, (_, taskIndex) => ({
            id: `${index}:${taskIndex}`,
            text: `task-${taskIndex}`,
            status: "pending",
          })),
        },
      })),
      participants: Array.from({ length: 100 }, (_, index) =>
        participant(index),
      ),
      missionState: {
        kind: "completed",
        sourceEventId: "completed",
        summary: "done",
        landedShas: Array.from({ length: 150 }, (_, index) => `sha-${index}`),
        followUps: Array.from({ length: 150 }, (_, index) => `follow-${index}`),
        canonicalChain: [],
      },
      rejectedReasons: Array.from({ length: 120 }, (_, index) => ({
        code: `rejected-${index}`,
        summary: "rejected",
        eventIds: Array.from(
          { length: 30 },
          (_, eventIndex) => `${index}:${eventIndex}`,
        ),
      })),
    }),
  );

  assert.equal(
    model.goal.eventIds.length,
    MISSION_INSPECTOR_LIMITS.goalSourceEventIds,
  );
  assert.equal(
    model.acceptedPlan.steps.length,
    MISSION_INSPECTOR_LIMITS.acceptedPlanSteps,
  );
  assert.equal(
    model.acceptedPlan.sourceEventIds.length,
    MISSION_INSPECTOR_LIMITS.acceptedPlanSteps,
  );
  assert.ok(
    model.truncations.some(
      (item) =>
        item.notice === "Showing 100 of 150 accepted-plan steps; 50 omitted.",
    ),
  );
  assert.ok(
    model.truncations.some(
      (item) =>
        item.notice === "Showing 100 of 150 accepted-plan sources; 50 omitted.",
    ),
  );
  assert.equal(model.seatPlans.length, MISSION_INSPECTOR_LIMITS.seatPlans);
  assert.ok(
    model.seatPlans.every(
      (plan) => plan.steps.length === MISSION_INSPECTOR_LIMITS.seatPlanSteps,
    ),
  );
  assert.equal(
    model.participants.length,
    MISSION_INSPECTOR_LIMITS.participants,
  );
  assert.equal(
    model.missionState.landedShas.length,
    MISSION_INSPECTOR_LIMITS.missionStateItems,
  );
  assert.equal(
    model.missionState.followUps.length,
    MISSION_INSPECTOR_LIMITS.missionStateItems,
  );
  assert.equal(
    model.integrity.rejectedReasons.length,
    MISSION_INSPECTOR_LIMITS.disclosures,
  );
  assert.ok(
    model.integrity.rejectedReasons.every(
      (item) =>
        item.eventIds.length === MISSION_INSPECTOR_LIMITS.disclosureEventIds,
    ),
  );
  for (const section of [
    "goal",
    "accepted-plan",
    "seat-plans",
    "team",
    "mission-state",
    "integrity",
  ]) {
    assert.ok(model.truncations.some((item) => item.section === section));
  }
  assert.ok(model.truncations.every((item) => item.notice.includes("omitted")));
  const seatStepNotice = model.truncations.find(
    (item) => item.id === "seat-plans:seat plan steps",
  );
  assert.deepEqual(
    {
      shown: seatStepNotice.shown,
      total: seatStepNotice.total,
      omitted: seatStepNotice.omitted,
    },
    { shown: 3_200, total: 4_800, omitted: 1_600 },
  );
});

test("blocked, conflict, and disclosure collections are capped and copied", () => {
  const blockers = Array.from(
    { length: 150 },
    (_, index) => `blocker-${index}`,
  );
  const conflictIds = Array.from(
    { length: 50 },
    (_, index) => `mission-conflict-${index}`,
  );
  const disclosureIds = Array.from(
    { length: 50 },
    (_, index) => `disclosure-${index}`,
  );
  const blocked = deriveCodingSessionMissionInspectorModel(
    baseInput({
      missionState: {
        kind: "blocked",
        sourceEventId: "blocked",
        summary: "blocked",
        blockers,
        requiredAction: "Unblock",
        canonicalChain: [],
      },
    }),
  );
  const conflict = deriveCodingSessionMissionInspectorModel(
    baseInput({
      missionState: { kind: "conflict", eventIds: conflictIds },
      conflicts: [
        {
          code: "causal-conflict",
          summary: "Conflicting signed causes",
          eventIds: disclosureIds,
        },
      ],
    }),
  );

  blockers[0] = "mutated";
  conflictIds[0] = "mutated";
  disclosureIds[0] = "mutated";
  assert.equal(
    blocked.missionState.blockers.length,
    MISSION_INSPECTOR_LIMITS.missionStateItems,
  );
  assert.equal(blocked.missionState.blockers[0], "blocker-0");
  assert.equal(
    conflict.missionState.eventIds.length,
    MISSION_INSPECTOR_LIMITS.disclosureEventIds,
  );
  assert.equal(conflict.missionState.eventIds[0], "mission-conflict-0");
  assert.equal(
    conflict.integrity.conflicts[0].eventIds.length,
    MISSION_INSPECTOR_LIMITS.disclosureEventIds,
  );
  assert.equal(conflict.integrity.conflicts[0].eventIds[0], "disclosure-0");
  assert.ok(
    blocked.truncations.some(
      (item) =>
        item.notice === "Showing 100 of 150 mission blockers; 50 omitted.",
    ),
  );
  assert.ok(
    conflict.truncations.some(
      (item) =>
        item.notice ===
        "Showing 20 of 50 conflicting mission-state sources; 30 omitted.",
    ),
  );
});

test("report tests, files, observed sources, and context have nested and total caps", () => {
  const reports = Array.from(
    { length: MISSION_INSPECTOR_LIMITS.reports + 5 },
    (_, index) => ({
      sourceEventId: `report-${index}`,
      authorLabel: `Seat ${index}`,
      summary: "report",
      assignmentRef: `assignment-${index}`,
      branch: `branch-${index}`,
      baseSha: `base-${index}`,
      headSha: `head-${index}`,
      files: Array.from(
        { length: 250 },
        (_, fileIndex) => `file-${index}-${fileIndex}`,
      ),
      tests: Array.from({ length: 150 }, (_, testIndex) => ({
        name: `test-${index}-${testIndex}`,
        command: "run",
        outcome: "passed",
        evidence: null,
      })),
    }),
  );
  const observedChanges = {
    files: Array.from({ length: 510 }, (_, index) => ({
      path: `observed-${index}`,
      filename: `observed-${index}`,
      additions: null,
      deletions: null,
      diffs: [],
      editCount: 1,
    })),
    unreportedEditCount: 0,
  };
  const observedFileSources = new Map([
    [
      "observed-0",
      Array.from({ length: 30 }, (_, index) => `observed-source-${index}`),
    ],
  ]);
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({ reports, observedChanges, observedFileSources }),
  );

  assert.equal(model.reports.length, MISSION_INSPECTOR_LIMITS.reports);
  assert.equal(model.tests.length, MISSION_INSPECTOR_LIMITS.tests);
  assert.equal(model.files.length, MISSION_INSPECTOR_LIMITS.files);
  assert.equal(
    model.contextFacts.length,
    MISSION_INSPECTOR_LIMITS.contextFacts,
  );
  assert.equal(
    model.files[0].observedSourceEventIds.length,
    MISSION_INSPECTOR_LIMITS.fileSourceEventIds,
  );
  const reportTestNotice = model.truncations.find(
    (item) => item.id === "tests:report tests",
  );
  assert.deepEqual(
    {
      shown: reportTestNotice.shown,
      total: reportTestNotice.total,
      omitted: reportTestNotice.omitted,
    },
    { shown: 10_000, total: 15_000, omitted: 5_000 },
  );
  for (const section of ["reports", "tests", "files", "changes", "context"]) {
    assert.ok(model.truncations.some((item) => item.section === section));
  }
});

test("repeated attribution overflow reports one exact bounded-collection count", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      observedChanges: {
        files: [
          {
            path: "shared.ts",
            filename: "shared.ts",
            additions: 1,
            deletions: 0,
            diffs: [],
            editCount: 1,
          },
        ],
        unreportedEditCount: 0,
      },
      reports: Array.from({ length: 22 }, (_, index) => ({
        sourceEventId: `report-${index}`,
        authorLabel: `Seat ${index}`,
        summary: "report",
        assignmentRef: "assignment",
        branch: null,
        baseSha: null,
        headSha: null,
        files: ["shared.ts"],
        tests: [],
      })),
    }),
  );

  assert.equal(model.files[0].reportedBy.length, 20);
  const notice = model.truncations.find(
    (item) => item.id === "files:file report attributions",
  );
  assert.deepEqual(
    {
      shown: notice.shown,
      total: notice.total,
      omitted: notice.omitted,
      notice: notice.notice,
    },
    {
      shown: 20,
      total: 22,
      omitted: 2,
      notice: "Showing 20 of 22 file report attributions; 2 omitted.",
    },
  );
});

test("D-T9: report seat authority is unseated, granted, or unknown — never inferred", () => {
  const reports = [
    {
      sourceEventId: "aa".repeat(32),
      authorLabel: "builder",
      summary: "granted work",
      assignmentRef: "cc".repeat(32),
      branch: null,
      baseSha: null,
      headSha: null,
      files: [],
      tests: [],
    },
    {
      sourceEventId: "bb".repeat(32),
      authorLabel: "stranger",
      summary: "ungranted work",
      assignmentRef: "cc".repeat(32),
      branch: null,
      baseSha: null,
      headSha: null,
      files: [],
      tests: [],
    },
  ];
  const folded = deriveCodingSessionMissionInspectorModel(
    baseInput({ reports, unseatedReportEventIds: ["bb".repeat(32)] }),
  );
  assert.deepEqual(
    folded.reports.map((report) => report.seatAuthority),
    ["granted", "unseated"],
  );
  const unfolded = deriveCodingSessionMissionInspectorModel(
    baseInput({ reports }),
  );
  assert.deepEqual(
    unfolded.reports.map((report) => report.seatAuthority),
    ["unknown", "unknown"],
    "no fold means no claim about a seat",
  );
});

test("D-T9: transactions and their truncation count pass through unchanged", () => {
  const transactions = [
    {
      sourceEventId: "aa".repeat(32),
      type: "report",
      authorPubkey: "dd".repeat(32),
      createdAt: 10,
      counterpartyPubkey: null,
      parentEventId: null,
      summary: "one",
      decision: null,
      requiredAction: null,
      fileCount: 0,
      testCount: 0,
      unseated: false,
    },
  ];
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({ transactions, transactionsTruncated: 7 }),
  );
  assert.deepEqual(model.transactions, transactions);
  assert.equal(model.transactionsTruncated, 7);
  const empty = deriveCodingSessionMissionInspectorModel(baseInput());
  assert.deepEqual(empty.transactions, []);
  assert.equal(empty.transactionsTruncated, 0);
});
