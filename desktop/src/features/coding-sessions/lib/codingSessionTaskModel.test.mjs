import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionTaskModel,
  MAX_CODING_SESSION_TASKS,
  MAX_CODING_SESSION_TASK_TEXT_LENGTH,
} from "./codingSessionTaskModel.ts";

function tool({
  args,
  descriptor = {},
  id,
  result = "",
  toolName,
  timestamp = "2026-07-30T12:00:00.000Z",
  turnId,
}) {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: {
      renderClass: "generic",
      label: "Ran tool",
      preview: null,
      ...descriptor,
    },
    title: toolName,
    toolName,
    beekeeperToolName: null,
    status: "completed",
    args,
    result,
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId,
  };
}

function plan(id, text, timestamp = "2026-07-30T12:00:00.000Z") {
  return {
    id,
    type: "plan",
    renderClass: "plan",
    title: "Plan",
    text,
    timestamp,
  };
}

test("normalizes Codex update_plan snapshots without provider-specific UI data", () => {
  const model = deriveCodingSessionTaskModel([
    tool({
      id: "codex-plan",
      toolName: "functions.update_plan",
      args: {
        explanation: "Working top to bottom",
        plan: [
          { step: "Inspect current state", status: "completed" },
          { step: "Build the rail", status: "in_progress" },
          { step: "Verify the app", status: "pending" },
          { step: "Report a strange state", status: "blocked" },
        ],
      },
    }),
  ]);

  assert.deepEqual(
    model.tasks.map(({ status, text }) => ({ status, text })),
    [
      { text: "Inspect current state", status: "completed" },
      { text: "Build the rail", status: "in_progress" },
      { text: "Verify the app", status: "pending" },
      { text: "Report a strange state", status: "blocked" },
    ],
  );
  assert.equal(model.completedCount, 1);
  assert.equal(model.state, "active");
  assert.equal(model.explanation, "Working top to bottom");
  assert.match(model.copyText, /^Working top to bottom/);
  assert.match(model.copyText, /- \[ \] Build the rail \(in progress\)/);
  assert.match(model.copyText, /- \[ \] Report a strange state \(blocked\)/);
});

test("normalizes Claude TodoWrite and Beekeeper todo snapshots", () => {
  const claude = deriveCodingSessionTaskModel([
    tool({
      id: "claude-todos",
      toolName: "TodoWrite",
      args: {
        todos: [
          {
            content: "Render Claude tasks",
            activeForm: "Rendering Claude tasks",
            status: "in_progress",
          },
          { content: "Ship it", status: "pending" },
        ],
      },
    }),
  ]);
  assert.deepEqual(
    claude.tasks.map(({ status, text }) => ({ status, text })),
    [
      { text: "Render Claude tasks", status: "in_progress" },
      { text: "Ship it", status: "pending" },
    ],
  );

  const buzz = deriveCodingSessionTaskModel([
    tool({
      id: "buzz-todos",
      toolName: "buzz_dev_mcp_todo",
      descriptor: { groupKey: "plan:todo" },
      args: {
        todos: [
          { text: "Read the brief", done: true },
          { text: "Keep marching", checked: false },
        ],
      },
    }),
  ]);
  assert.deepEqual(
    buzz.tasks.map(({ status, text }) => ({ status, text })),
    [
      { text: "Read the brief", status: "completed" },
      { text: "Keep marching", status: "pending" },
    ],
  );
});

test("uses explicit Markdown checklists and preserves in-progress state", () => {
  const model = deriveCodingSessionTaskModel([
    plan(
      "markdown-plan",
      [
        "### Next",
        "- [x] Audit the signed feed",
        "- [ ] Build the rail (in progress)",
        "1. [ ] Run focused tests",
        "- this is not an explicit task",
      ].join("\n"),
    ),
  ]);

  assert.deepEqual(
    model.tasks.map(({ status, text }) => ({ status, text })),
    [
      { text: "Audit the signed feed", status: "completed" },
      { text: "Build the rail", status: "in_progress" },
      { text: "Run focused tests", status: "pending" },
    ],
  );
  assert.equal(model.explanation, "Next");
  assert.match(model.copyText, /### Next/);
});

test("preserves actionable provider states without inventing task actions", () => {
  const model = deriveCodingSessionTaskModel([
    tool({
      id: "stateful-plan",
      toolName: "update_plan",
      args: {
        plan: [
          { step: "Waiting on access", status: "waiting_for_input" },
          { step: "Broken verification", status: "failed" },
          { step: "No longer needed", status: "skipped" },
          { step: "Provider extension", status: "future_state" },
        ],
      },
    }),
  ]);

  assert.deepEqual(
    model.tasks.map(({ status }) => status),
    ["blocked", "failed", "cancelled", "unknown"],
  );
});

test("latest valid plan-bearing event replaces older tasks, including an empty plan", () => {
  const oldPlan = tool({
    id: "old-plan",
    toolName: "update_plan",
    args: { plan: [{ step: "Old task", status: "pending" }] },
  });
  const irrelevant = tool({
    id: "shell",
    toolName: "shell",
    args: { command: "echo harmless" },
  });
  const malformed = tool({
    id: "malformed-plan",
    toolName: "update_plan",
    args: { plan: [{ status: "pending" }] },
  });

  assert.equal(
    deriveCodingSessionTaskModel([oldPlan, irrelevant, malformed]).sourceItemId,
    "old-plan",
  );

  const cleared = deriveCodingSessionTaskModel([
    oldPlan,
    tool({
      id: "empty-plan",
      toolName: "update_plan",
      args: { plan: [] },
    }),
  ]);
  assert.equal(cleared.sourceItemId, "empty-plan");
  assert.equal(cleared.state, "empty");
  assert.deepEqual(cleared.tasks, []);
});

test("stable IDs survive status changes and task reordering", () => {
  const first = deriveCodingSessionTaskModel([
    tool({
      id: "plan-1",
      toolName: "update_plan",
      args: {
        plan: [
          { step: "Alpha", status: "pending" },
          { step: "Beta", status: "pending" },
        ],
      },
    }),
  ]);
  const second = deriveCodingSessionTaskModel([
    tool({
      id: "plan-2",
      toolName: "update_plan",
      args: {
        plan: [
          { step: "Beta", status: "completed" },
          { step: "Alpha", status: "in_progress" },
        ],
      },
    }),
  ]);

  assert.equal(
    first.tasks.find((task) => task.text === "Alpha").id,
    second.tasks.find((task) => task.text === "Alpha").id,
  );
  assert.equal(
    first.tasks.find((task) => task.text === "Beta").id,
    second.tasks.find((task) => task.text === "Beta").id,
  );
});

test("completed tasks derive elapsed time only from signed snapshots in their turn", () => {
  const model = deriveCodingSessionTaskModel([
    tool({
      id: "plan-1",
      timestamp: "2026-07-30T12:00:00.000Z",
      toolName: "update_plan",
      turnId: "turn-1",
      args: { plan: [{ step: "Inspect", status: "in_progress" }] },
    }),
    tool({
      id: "plan-2",
      timestamp: "2026-07-30T12:03:59.000Z",
      toolName: "update_plan",
      turnId: "turn-1",
      args: { plan: [{ step: "Inspect", status: "completed" }] },
    }),
  ]);

  assert.equal(model.tasks[0].elapsedMs, 239_000);
});

test("bounds task count and task text from signed transcript data", () => {
  const model = deriveCodingSessionTaskModel([
    tool({
      id: "bounded-plan",
      toolName: "update_plan",
      args: {
        plan: Array.from(
          { length: MAX_CODING_SESSION_TASKS + 10 },
          (_, index) => ({
            step: `${index}-${"x".repeat(MAX_CODING_SESSION_TASK_TEXT_LENGTH + 20)}`,
            status: "pending",
          }),
        ),
      },
    }),
  ]);

  assert.equal(model.tasks.length, MAX_CODING_SESSION_TASKS);
  assert.equal(model.tasks[0].text.length, MAX_CODING_SESSION_TASK_TEXT_LENGTH);
});

test("reads a Beekeeper todo checklist result only when args are not authoritative", () => {
  const model = deriveCodingSessionTaskModel([
    tool({
      id: "todo-result",
      toolName: "todo",
      args: {},
      result: JSON.stringify({
        stdout: "- [x] Done through result\n- [ ] Still open",
      }),
    }),
  ]);
  assert.deepEqual(
    model.tasks.map(({ status, text }) => ({ status, text })),
    [
      { text: "Done through result", status: "completed" },
      { text: "Still open", status: "pending" },
    ],
  );
});

test("returns null when the signed transcript contains no valid plan snapshot", () => {
  assert.equal(
    deriveCodingSessionTaskModel([
      plan("prose-plan", "Inspect, build, and verify."),
      tool({ id: "shell", toolName: "shell", args: {} }),
    ]),
    null,
  );
});
