import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";

import {
  deriveCodingSessionTranscriptModel,
  formatCodingSessionBackgroundTasks,
  isCodingSessionTaskNotificationItem,
  parseCodingSessionTaskNotifications,
  stabilizeCodingSessionTranscriptModel,
} from "../lib/codingSessionTranscriptModel.ts";
import { CodingSessionTranscript } from "./CodingSessionTranscript.tsx";

/**
 * SV-78 (ledger 336): a turn that ended with a background task still running
 * does not read as plainly finished, and the turn the task's notification
 * woke renders as its own turn under a wake row — never as a person's bubble.
 * Wording is Claude Code 2.1.x's own, copied from a real transcript.
 */

const t0 = "2026-10-05T20:48:15.000Z";
const t1 = "2026-10-05T20:48:56.000Z";
const t2 = "2026-10-05T20:51:18.000Z";
const t3 = "2026-10-05T20:52:27.000Z";
const TASK = "bqlyvw89h";

const BACKGROUND_RESULT = `Command running in background with ID: ${TASK}. Output is being written to: /private/tmp/claude-502/x/tasks/${TASK}.output. You will be notified when it completes. To check interim output, use Read on that file path.`;

function notificationText(taskId = TASK, status = "completed") {
  return [
    "<task-notification>",
    `<task-id>${taskId}</task-id>`,
    "<tool-use-id>toolu_01</tool-use-id>",
    `<output-file>/private/tmp/claude-502/x/tasks/${taskId}.output</output-file>`,
    `<status>${status}</status>`,
    '<summary>Background command "sleep 150" completed (exit code 0)</summary>',
    "</task-notification>",
  ].join("\n");
}

function message(id, role, text, turnId, timestamp = t0) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId,
  };
}

function tool(id, turnId, overrides = {}) {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: {
      renderClass: "shell",
      label: "Ran command",
      preview: "sleep 150",
      action: { verb: "Ran", object: "sleep 150" },
    },
    title: "Bash",
    toolName: "Bash",
    beekeeperToolName: null,
    status: "completed",
    args: { command: "sleep 150", run_in_background: true },
    result: BACKGROUND_RESULT,
    isError: false,
    timestamp: t0,
    startedAt: t0,
    completedAt: t0,
    turnId,
    ...overrides,
  };
}

function turnResult(id, turnId, durationMs, timestamp = t1) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "",
    outcome: "success",
    durationMs,
    timestamp,
    turnId,
  };
}

function continuity(id, timestamp) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Session continuity",
    text: "Restarted without its history",
    timestamp,
  };
}

/** The audit session's first turn: background a sleep, answer, end. */
function endedTurn() {
  return [
    message("prompt-1", "user", "Run steps 1-8", "turn-1"),
    tool("bg-1", "turn-1"),
    message(
      "answer-1",
      "assistant",
      "I'll be notified when it finishes.",
      "turn-1",
      t1,
    ),
    turnResult("result-1", "turn-1", 41_000),
  ];
}

/** The turn the notification woke, with no prompt from anyone. */
function wokenTurn(status = "completed") {
  return [
    message("wake-2", "user", notificationText(TASK, status), "turn-2", t2),
    tool("step-6", "turn-2", { result: "ok", args: { command: "echo a" } }),
    message("answer-2", "assistant", "Steps 6-8 done.", "turn-2", t3),
    turnResult("result-2", "turn-2", 69_000, t3),
  ];
}

function turns(items) {
  return deriveCodingSessionTranscriptModel(items, {
    isWorking: false,
  }).blocks.filter((block) => block.kind === "turn");
}

async function render(items, isWorking = false) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionTranscript, {
        generationId: "generation-1",
        isWorking,
        items,
      }),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("SV-78: a notification block parses into its id, status and summary", () => {
  assert.deepEqual(parseCodingSessionTaskNotifications(notificationText()), [
    {
      taskId: TASK,
      status: "completed",
      summary: 'Background command "sleep 150" completed (exit code 0)',
    },
  ]);
  assert.deepEqual(
    parseCodingSessionTaskNotifications("<task-notification>"),
    [],
  );
  assert.deepEqual(
    parseCodingSessionTaskNotifications(
      "<task-notification><status>completed</status></task-notification>",
    ),
    [],
    "a block naming no task reports nothing",
  );
});

test("SV-78: only a prompt that is wholly a notification is the runtime's", () => {
  assert.equal(
    isCodingSessionTaskNotificationItem(
      message("n", "user", notificationText(), "t"),
    ),
    true,
  );
  assert.equal(
    isCodingSessionTaskNotificationItem(
      message("q", "user", `What does this mean?\n${notificationText()}`, "t"),
    ),
    false,
    "a person quoting a notification is still a person",
  );
  assert.equal(
    isCodingSessionTaskNotificationItem(
      message("a", "assistant", notificationText(), "t"),
    ),
    false,
  );
});

test("SV-78: a turn that ended with its background task unreported says the task is running", () => {
  const [turn] = turns(endedTurn());
  assert.equal(turn.completion?.state, "completed");
  assert.deepEqual(turn.backgroundTasks, [
    { id: TASK, state: "running", status: null },
  ]);
  assert.equal(
    formatCodingSessionBackgroundTasks(turn.backgroundTasks),
    "1 background task running",
  );
});

test("SV-78: the notification clears it, with the notification's own status", () => {
  const [ended, woken] = turns([...endedTurn(), ...wokenTurn("failed")]);
  assert.deepEqual(ended.backgroundTasks, [
    { id: TASK, state: "reported", status: "failed" },
  ]);
  assert.equal(formatCodingSessionBackgroundTasks(ended.backgroundTasks), null);
  assert.deepEqual(woken.backgroundTasks, []);
});

test("SV-78: a notification before the start, or for another id, clears nothing", () => {
  const [, ended] = turns([
    message("wake-0", "user", notificationText(), "turn-0"),
    turnResult("result-0", "turn-0", 1),
    ...endedTurn(),
    message("wake-x", "user", notificationText("other99"), "turn-3", t2),
  ]);
  assert.equal(ended.backgroundTasks[0]?.state, "running");
});

test("SV-78: a later execution start, or the session ending, says never reported finished — never finished", () => {
  const [turn] = turns([...endedTurn(), continuity("cont-1", t2)]);
  assert.deepEqual(turn.backgroundTasks, [
    { id: TASK, state: "unreported", status: null },
  ]);
  assert.equal(
    formatCodingSessionBackgroundTasks(turn.backgroundTasks),
    "1 background task never reported finished",
  );
  const running = [{ id: "a", state: "running", status: null }];
  assert.equal(
    formatCodingSessionBackgroundTasks(running, true),
    "1 background task never reported finished",
  );
  assert.equal(
    formatCodingSessionBackgroundTasks([
      ...running,
      { id: "b", state: "running", status: null },
      { id: "c", state: "reported", status: "completed" },
    ]),
    "2 background tasks running",
  );
});

test("SV-78: an unprompted wake with no notification text stops saying running, and never says finished", () => {
  // The live shape (claude-agent-acp 0.84.0): the adapter forwards the
  // agent's reply but not the <task-notification> that woke it, so the only
  // trace is the provider's status rows around the unprompted turn (SV-77).
  const wakeRow = (id, text) => ({
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Status",
    text,
    timestamp: t2,
    turnId: "turn-auto",
  });
  const [ended] = turns([
    ...endedTurn(),
    wakeRow(
      "auto-start",
      "autonomous_turn_started: the agent began a turn nobody prompted",
    ),
    message("auto-reply", "assistant", "FINISHED", "turn-auto", t2),
    turnResult("auto-result", "turn-auto", 1000, t3),
  ]);
  assert.deepEqual(ended.backgroundTasks, [
    { id: TASK, state: "woke", status: null },
  ]);
  const label = formatCodingSessionBackgroundTasks(ended.backgroundTasks, true);
  assert.equal(label, "1 background task, then the agent woke on its own");
  assert.doesNotMatch(label, /running|finished/);
});

test("SV-78: the stable model replaces a turn whose background task changed state", () => {
  const first = endedTurn();
  const before = deriveCodingSessionTranscriptModel(first, {
    isWorking: false,
  });
  const items = [...first, ...wokenTurn()];
  const after = deriveCodingSessionTranscriptModel(items, { isWorking: false });
  const stable = stabilizeCodingSessionTranscriptModel(before, after);
  assert.notEqual(stable.blocks[0], before.blocks[0]);
  assert.equal(stable.blocks[0].backgroundTasks[0].state, "reported");
  const again = stabilizeCodingSessionTranscriptModel(
    stable,
    deriveCodingSessionTranscriptModel(items, { isWorking: false }),
  );
  assert.equal(again, stable, "an unchanged transcript keeps its model");
});

test("SV-78: the ended turn's Worked-for row names the running background task", async () => {
  const markup = await render(endedTurn());
  const fold = markup.match(
    /data-testid="coding-session-worked-fold"[\s\S]*?<\/button>/,
  );
  assert.ok(fold, markup);
  assert.match(fold[0], /Worked for 41s/);
  assert.match(
    fold[0],
    /data-testid="coding-session-turn-background"[^>]*>·<svg[\s\S]*?<\/svg>1 background task running/,
  );
  assert.match(
    fold[0],
    new RegExp(
      `title="Background task ${TASK}: no completion shown in this transcript"`,
    ),
  );
  // Said once: the fold row carries it, so the line under the answer does not.
  assert.equal(markup.match(/coding-session-turn-background"/g)?.length, 1);
});

test("SV-78: a turn with no fold still says it on the line under the answer", async () => {
  // A failed turn never folds; its line says both things.
  const markup = await render([
    ...endedTurn().slice(0, 3),
    { ...turnResult("result-1", "turn-1", 41_000), renderClass: "error" },
  ]);
  assert.doesNotMatch(markup, /coding-session-worked-fold"/);
  assert.match(
    markup,
    /data-testid="coding-session-turn-completion"[\s\S]*?Failed[\s\S]*?data-testid="coding-session-turn-background"[^>]*><svg[\s\S]*?<\/svg>1 background task running/,
  );
});

test("SV-78: the woken turn renders under a wake row, not a prompt bubble, and the clause clears", async () => {
  const markup = await render([...endedTurn(), ...wokenTurn()]);
  assert.doesNotMatch(markup, /background task running/);
  assert.doesNotMatch(markup, /&lt;task-notification&gt;/);
  const wake = markup.match(
    /<div[^>]*data-testid="coding-session-background-wake"[\s\S]*?<\/div>/,
  );
  assert.ok(wake, markup);
  assert.match(wake[0], /data-opens-turn=""/);
  assert.match(wake[0], new RegExp(`Woke on background task ${TASK}`));
  assert.match(wake[0], /· completed/);
  assert.match(wake[0], /Background command &quot;sleep 150&quot; completed/);
  // Two turns, the second its own section.
  assert.equal(markup.match(/data-testid="coding-session-turn"/g)?.length, 2);
  assert.match(markup, /data-turn-id="turn-2"/);
});

test("SV-78: a notification inside a prompted turn reads as reported, not woke", async () => {
  const markup = await render([
    message("prompt-1", "user", "Keep going", "turn-1"),
    tool("bg-1", "turn-1"),
    message("wake-1", "user", notificationText(), "turn-1", t2),
    message("answer-1", "assistant", "It finished.", "turn-1", t3),
    turnResult("result-1", "turn-1", 90_000, t3),
  ]);
  assert.match(markup, new RegExp(`Background task ${TASK} reported`));
  assert.doesNotMatch(markup, /Woke on/);
  assert.doesNotMatch(markup, /background task running/);
});

test("SV-78: a live turn shows no background clause — the working line speaks for it", async () => {
  const markup = await render(endedTurn().slice(0, 2), true);
  assert.doesNotMatch(markup, /coding-session-turn-background/);
});
