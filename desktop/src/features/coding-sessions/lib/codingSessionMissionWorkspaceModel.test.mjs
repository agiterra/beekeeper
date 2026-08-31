import assert from "node:assert/strict";
import test from "node:test";

import { emptyCodingSessionMissionInspectorInput } from "./codingSessionMissionEvidenceModel.ts";
import { mergeCodingSessionMissionWorkspaceInput } from "./codingSessionMissionWorkspaceModel.ts";

const observed = {
  files: [
    {
      path: "desktop/src/mission.tsx",
      filename: "mission.tsx",
      diffs: [],
      editCount: 1,
      additions: null,
      deletions: null,
    },
  ],
  unreportedEditCount: 0,
};
const participant = {
  executionKey: "builder",
  label: "Bob · Builder",
  secondaryLabel: "claude-agent-acp · sonnet",
  role: "builder",
  status: { kind: "idle", label: "Idle" },
  disposition: "idle",
  activity: null,
  lastTurnLabel: "last turn just now",
};

test("trusted workspace facts merge without manufacturing absent transaction facts", () => {
  const evidence = emptyCodingSessionMissionInspectorInput();
  const model = mergeCodingSessionMissionWorkspaceInput({
    evidence,
    goal: {
      channelId: "channel",
      content: "Ship the portable team loop.",
      createdAt: 1,
      eventId: "goal-event",
      founderPubkey: "f".repeat(64),
      sessionRef: "session",
    },
    goalAuthorLabel: "Founder",
    observedChanges: observed,
    participants: [participant],
    contextLoads: new Map([["builder", null]]),
    seatPlans: [
      { executionKey: "builder", ownerLabel: "Bob · Builder", model: null },
    ],
  });

  assert.deepEqual(model.goal, {
    kind: "available",
    sourceEventId: "goal-event",
    authorLabel: "Founder",
    text: "Ship the portable team loop.",
  });
  assert.equal(model.participants[0].label, "Bob · Builder");
  assert.equal(model.observedChanges.files[0].path, "desktop/src/mission.tsx");
  assert.equal(model.contextLoads.get("builder"), null);
  assert.equal(model.acceptedPlan.kind, "absent");
  assert.equal(model.missionState.kind, "unknown");
  assert.equal(model.usage, null);
  assert.deepEqual(model.observedFileSources, new Map());
});

test("absent workspace facts remain absent and canonical evidence survives", () => {
  const evidence = {
    ...emptyCodingSessionMissionInspectorInput(),
    acceptedPlan: {
      kind: "available",
      steps: [
        {
          text: "Run the gate",
          sourceEventId: "plan-event",
          authorLabel: "Lead",
          sourceCreatedAt: 1,
          sourceIndex: 0,
        },
      ],
    },
    reports: [
      {
        sourceEventId: "report-event",
        authorLabel: "Builder",
        summary: "Done",
        assignmentRef: "assignment-event",
        branch: null,
        baseSha: null,
        headSha: null,
        files: [],
        tests: [],
      },
    ],
  };
  const model = mergeCodingSessionMissionWorkspaceInput({
    evidence,
    goal: null,
    goalAuthorLabel: "Founder",
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    seatPlans: [],
  });

  assert.equal(model.goal.kind, "absent");
  assert.equal(model.acceptedPlan.kind, "available");
  assert.equal(model.reports[0].sourceEventId, "report-event");
  assert.deepEqual(model.participants, []);
  assert.deepEqual(model.observedChanges.files, []);
});
