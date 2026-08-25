import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionActiveTaskModel } from "./useCodingSessionTaskDock.ts";

const item = (id, turnId) => ({
  id,
  turnId,
  type: "plan",
  renderClass: "plan",
  title: "Plan updated",
  text: "- [ ] Work",
  timestamp: "2026-08-25T12:00:00.000Z",
});

const model = {
  sourceItemId: "plan-1",
  turnId: "turn-2",
  timestamp: "2026-08-25T12:00:00.000Z",
  tasks: [{ id: "task-1", text: "Work", status: "in_progress" }],
  completedCount: 0,
  explanation: null,
  copyText: null,
  state: "active",
};

test("attaches only an active plan from the running transcript turn", () => {
  assert.equal(
    deriveCodingSessionActiveTaskModel({
      isWorking: true,
      model,
      transcript: [item("plan-1", "turn-2")],
    }),
    model,
  );
  assert.equal(
    deriveCodingSessionActiveTaskModel({
      isWorking: false,
      model,
      transcript: [item("plan-1", "turn-2")],
    }),
    null,
  );
});

test("does not pin a stale or untraceable plan to a newer turn", () => {
  assert.equal(
    deriveCodingSessionActiveTaskModel({
      isWorking: true,
      model,
      transcript: [item("plan-1", "turn-2"), item("plan-2", "turn-3")],
    }),
    null,
  );
  assert.equal(
    deriveCodingSessionActiveTaskModel({
      isWorking: true,
      model: { ...model, turnId: null },
      transcript: [item("plan-1", null)],
    }),
    null,
  );
});

test("completed plans release from the composer attachment", () => {
  assert.equal(
    deriveCodingSessionActiveTaskModel({
      isWorking: true,
      model: { ...model, state: "complete", completedCount: 1 },
      transcript: [item("plan-1", "turn-2")],
    }),
    null,
  );
});
