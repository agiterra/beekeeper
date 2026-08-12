import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionChangedFiles,
  deriveCodingSessionTranscriptModel,
  formatCodingSessionDuration,
  isCodingSessionTranscriptError,
  parseCodingSessionTurnResult,
  stabilizeCodingSessionTranscriptModel,
} from "./codingSessionTranscriptModel.ts";

const timestamp = "2026-07-30T12:00:00.000Z";

function message({ id, role, text, turnId = "turn-1" }) {
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

function lifecycle({
  id,
  title,
  text,
  renderClass = "status",
  turnId = "turn-1",
}) {
  return {
    id,
    type: "lifecycle",
    renderClass,
    title,
    text,
    timestamp,
    turnId,
  };
}

function tool({
  id,
  renderClass = "shell",
  isError = false,
  status = isError ? "failed" : "completed",
  turnId = "turn-1",
}) {
  return {
    id,
    type: "tool",
    renderClass: isError ? "error" : renderClass,
    descriptor: {
      renderClass: isError ? "error" : renderClass,
      label: isError ? "Command failed" : "Ran command",
      preview: `command-${id}`,
    },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status,
    args: { command: `command-${id}` },
    result: isError ? "exit 1" : "ok",
    isError,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId,
  };
}

test("keeps user and assistant content while omitting valid system init metadata", () => {
  const systemInit = lifecycle({
    id: "system",
    title: "System Init",
    text: [
      "provider: claude",
      "model: sonnet",
      "tools: Bash, Edit",
      "mcpServers: chrome (connected), fetch (failed)",
    ].join("\n"),
    turnId: null,
  });
  const model = deriveCodingSessionTranscriptModel(
    [
      systemInit,
      message({ id: "prompt", role: "user", text: "Fix the reconnect bug" }),
      lifecycle({
        id: "context",
        title: "Context Window Updated",
        text: "inputTokens: 66546",
      }),
      message({
        id: "answer",
        role: "assistant",
        text: "The reconnect path is fixed.",
      }),
    ],
    { isWorking: false },
  );

  assert.equal(isCodingSessionTranscriptError(systemInit), false);
  assert.deepEqual(model.diagnostics, []);
  assert.equal(model.blocks.length, 1);
  assert.equal(model.blocks[0].kind, "turn");
  assert.deepEqual(
    model.blocks[0].entries.map((entry) =>
      entry.kind === "item" ? [entry.item.type, entry.item.text] : entry.kind,
    ),
    [
      ["message", "Fix the reconnect bug"],
      ["message", "The reconnect path is fixed."],
    ],
  );
  assert.deepEqual(model.blocks[0].diagnostics, []);
});

test("deduplicates result echoes without dropping completion duration and cost", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Say hello" }),
      message({ id: "answer", role: "assistant", text: "Hey Brian." }),
      lifecycle({
        id: "result",
        title: "Turn result",
        text: "Hey Brian. (3557ms) ($0.3209)",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.equal(
    turn.entries.filter(
      (entry) =>
        entry.kind === "item" &&
        entry.item.type === "message" &&
        entry.item.role === "assistant",
    ).length,
    1,
  );
  assert.deepEqual(turn.completion, {
    durationMs: 3557,
    costUsd: 0.3209,
    outcome: null,
    timestamp,
    state: "completed",
  });
});

test("coalesces an unscoped signed terminal cycle and removes its result echo", () => {
  const sessionId = "generation-2";
  const answer = {
    ...message({
      id: "answer",
      role: "assistant",
      text: "Hey Brian.",
      turnId: null,
    }),
    sessionId,
  };
  const context = {
    ...lifecycle({
      id: "context",
      title: "Context Window Updated",
      text: "inputTokens: 66546",
      turnId: null,
    }),
    sessionId,
  };
  const result = {
    ...lifecycle({
      id: "result",
      title: "Turn result",
      text: "Hey Brian. (3557ms) ($0.3209)",
      turnId: null,
    }),
    outcome: "success",
    sessionId,
  };

  const model = deriveCodingSessionTranscriptModel([answer, context, result], {
    isWorking: false,
  });
  const turn = model.blocks[0];

  assert.equal(model.blocks.length, 1);
  assert.equal(turn.kind, "turn");
  assert.equal(turn.id, "settled:result");
  assert.deepEqual(
    turn.entries.flatMap((entry) =>
      entry.kind === "item" &&
      entry.item.type === "message" &&
      entry.item.role === "assistant"
        ? [entry.item.text]
        : [],
    ),
    ["Hey Brian."],
  );
  assert.deepEqual(turn.diagnostics, []);
  assert.deepEqual(turn.completion, {
    durationMs: 3557,
    costUsd: 0.3209,
    outcome: "success",
    timestamp,
    state: "completed",
  });
});

test("does not coalesce unscoped activity across session generations", () => {
  const earlier = {
    ...message({
      id: "earlier",
      role: "assistant",
      text: "Earlier generation.",
      turnId: null,
    }),
    sessionId: "generation-1",
  };
  const result = {
    ...lifecycle({
      id: "result",
      title: "Turn result",
      text: "Current generation. (900ms)",
      turnId: null,
    }),
    sessionId: "generation-2",
  };

  const model = deriveCodingSessionTranscriptModel([earlier, result], {
    isWorking: false,
  });

  assert.equal(model.blocks.length, 2);
  assert.equal(model.blocks[0].kind, "standalone");
  assert.equal(model.blocks[0].entry.item.id, "earlier");
  assert.equal(model.blocks[1].kind, "turn");
  assert.equal(model.blocks[1].id, "settled:result");
  assert.equal(model.blocks[1].entries[0].kind, "item");
  assert.equal(model.blocks[1].entries[0].item.type, "message");
  assert.equal(model.blocks[1].entries[0].item.text, "Current generation.");
});

test("deduplicates only the result echo, preserving same-text assistant identities", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Repeat the update" }),
      message({ id: "answer-1", role: "assistant", text: "Still working." }),
      message({ id: "answer-2", role: "assistant", text: "Still working." }),
      lifecycle({
        id: "result",
        title: "Turn result",
        text: "Still working. (1200ms)",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.entries.flatMap((entry) =>
      entry.kind === "item" &&
      entry.item.type === "message" &&
      entry.item.role === "assistant"
        ? [entry.item.id]
        : [],
    ),
    ["answer-1", "answer-2"],
  );
  assert.deepEqual(turn.diagnostics, []);
});

test("uses a non-duplicate result body as assistant content", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Give me the result" }),
      lifecycle({
        id: "result",
        title: "Turn result",
        text: "Finished successfully. (1695ms) ($0.0712)",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.equal(turn.entries[1].kind, "item");
  assert.equal(turn.entries[1].item.type, "message");
  assert.equal(turn.entries[1].item.role, "assistant");
  assert.equal(turn.entries[1].item.text, "Finished successfully.");
});

test("collapses only the prefix of adjacent successful tools", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Run checks" }),
      tool({ id: "tool-1" }),
      tool({ id: "tool-2" }),
      tool({ id: "tool-failed", isError: true }),
      tool({ id: "tool-3" }),
    ],
    { isWorking: true },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.equal(turn.entries[1].kind, "tool-group");
  assert.equal(turn.entries[1].label, "Ran 1 command");
  assert.deepEqual(
    turn.entries[1].items.map((item) => item.id),
    ["tool-1"],
  );
  assert.equal(turn.entries[2].kind, "item");
  assert.equal(turn.entries[2].item.id, "tool-2");
  assert.equal(turn.entries[3].kind, "item");
  assert.equal(turn.entries[3].item.id, "tool-failed");
  assert.equal(turn.entries[4].kind, "item");
  assert.equal(turn.entries[4].item.id, "tool-3");
  assert.equal(turn.isWorking, true);
});

test("never groups pending, executing, permission, or error rows", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Run checks" }),
      tool({ id: "complete-1" }),
      tool({ id: "executing", status: "executing" }),
      tool({ id: "pending", status: "pending" }),
      lifecycle({
        id: "permission",
        title: "Permission requested",
        text: "Allow Bash?",
        renderClass: "permission",
      }),
      lifecycle({
        id: "error",
        title: "Provider error",
        text: "Connection failed",
        renderClass: "error",
      }),
      tool({ id: "complete-2" }),
    ],
    { isWorking: true },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.entries
      .slice(1)
      .map((entry) =>
        entry.kind === "item"
          ? [entry.item.id, entry.item.type === "tool" && entry.item.status]
          : entry.kind,
      ),
    [
      ["complete-1", "completed"],
      ["executing", "executing"],
      ["pending", "pending"],
      ["permission", false],
      ["error", false],
      ["complete-2", "completed"],
    ],
  );
});

test("derives unique changed files only from successful signed file-edit tools", () => {
  const first = tool({ id: "edit-1", renderClass: "file-edit" });
  first.descriptor = {
    renderClass: "file-edit",
    label: "Edited file",
    preview: "src/app.ts",
    object: "src/app.ts",
  };
  first.args = { file_path: "./src/app.ts" };
  first.result = [
    "diff --git a/src/app.ts b/src/app.ts",
    "--- a/src/app.ts",
    "+++ b/src/app.ts",
    "@@ -1 +1 @@",
    "-old",
    "+new",
  ].join("\n");

  const second = {
    ...first,
    id: "edit-2",
    args: { file_path: "src/app.ts" },
    result: [
      "diff --git a/src/app.ts b/src/app.ts",
      "--- a/src/app.ts",
      "+++ b/src/app.ts",
      "@@ -3,0 +4 @@",
      "+another",
    ].join("\n"),
  };
  const failed = {
    ...first,
    id: "edit-failed",
    args: { file_path: "src/failed.ts" },
    isError: true,
    status: "failed",
  };

  assert.deepEqual(deriveCodingSessionChangedFiles([first, second, failed]), [
    {
      path: "src/app.ts",
      filename: "app.ts",
      additions: 2,
      deletions: 1,
      editCount: 2,
      diffs: [
        {
          id: "edit-1",
          path: "src/app.ts",
          filename: "app.ts",
          additions: 1,
          deletions: 1,
          lines: [
            { kind: "meta", text: "diff --git a/src/app.ts b/src/app.ts" },
            { kind: "meta", text: "--- a/src/app.ts" },
            { kind: "meta", text: "+++ b/src/app.ts" },
            { kind: "meta", text: "@@ -1 +1 @@" },
            { kind: "remove", text: "-old" },
            { kind: "add", text: "+new" },
          ],
        },
        {
          id: "edit-2",
          path: "src/app.ts",
          filename: "app.ts",
          additions: 1,
          deletions: 0,
          lines: [
            { kind: "meta", text: "diff --git a/src/app.ts b/src/app.ts" },
            { kind: "meta", text: "--- a/src/app.ts" },
            { kind: "meta", text: "+++ b/src/app.ts" },
            { kind: "meta", text: "@@ -3,0 +4 @@" },
            { kind: "add", text: "+another" },
          ],
        },
      ],
    },
  ]);
});

test("keeps a changed filename but omits partial stats when any edit is uncounted", () => {
  const edit = tool({ id: "edit", renderClass: "file-edit" });
  edit.descriptor = {
    renderClass: "file-edit",
    label: "Edited file",
    preview: "src/app.ts",
    object: "src/app.ts",
  };
  edit.args = { file_path: "src/app.ts" };
  edit.result = "File updated successfully.";

  assert.deepEqual(deriveCodingSessionChangedFiles([edit]), [
    {
      path: "src/app.ts",
      filename: "app.ts",
      additions: null,
      deletions: null,
      editCount: 1,
      diffs: [],
    },
  ]);
});

test("failed Turn result produces a failed completion and keeps its error visible", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Build" }),
      lifecycle({
        id: "result",
        title: "Turn result",
        text: "Compiler exited with status 1 (900ms) ($0.0040)",
        renderClass: "error",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.equal(turn.completion.state, "failed");
  assert.equal(turn.completion.durationMs, 900);
  assert.equal(turn.entries[1].kind, "item");
  assert.equal(turn.entries[1].item.id, "result");
  assert.equal(turn.entries[1].item.renderClass, "error");
});

test("never hides lifecycle errors even when their title is normally diagnostic", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Build" }),
      lifecycle({
        id: "failed-status",
        title: "Status",
        text: "failed: compiler exited 1",
        renderClass: "error",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.equal(turn.entries[1].kind, "item");
  assert.equal(turn.entries[1].item.id, "failed-status");
  assert.deepEqual(turn.diagnostics, []);
});

test("keeps meaningful non-error diagnostics after suppressing context telemetry", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Build" }),
      lifecycle({ id: "status", title: "Status", text: "Retrying provider" }),
    ],
    { isWorking: true },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.diagnostics.map((item) => item.id),
    ["status"],
  );
});

test("parses result metadata and formats compact T3-style elapsed time", () => {
  assert.deepEqual(parseCodingSessionTurnResult("Done (65000ms) ($0.0100)"), {
    body: "Done",
    durationMs: 65000,
    costUsd: 0.01,
  });
  assert.equal(formatCodingSessionDuration(320), "320ms");
  assert.equal(formatCodingSessionDuration(3557), "3.6s");
  assert.equal(formatCodingSessionDuration(65000), "1m 5s");
});

test("reuses settled turn objects when a massive transcript appends a new tail", () => {
  const firstTurnItems = [
    message({
      id: "prompt-1",
      role: "user",
      text: "First turn",
      turnId: "turn-1",
    }),
    message({
      id: "answer-1",
      role: "assistant",
      text: "First answer",
      turnId: "turn-1",
    }),
    lifecycle({
      id: "result-1",
      title: "Turn result",
      text: "First answer (1000ms)",
      turnId: "turn-1",
    }),
  ];
  const previous = deriveCodingSessionTranscriptModel(firstTurnItems, {
    isWorking: false,
  });
  const next = deriveCodingSessionTranscriptModel(
    [
      ...firstTurnItems,
      message({
        id: "prompt-2",
        role: "user",
        text: "Second turn",
        turnId: "turn-2",
      }),
    ],
    { isWorking: true },
  );
  const stable = stabilizeCodingSessionTranscriptModel(previous, next);

  assert.equal(stable.blocks.length, 2);
  assert.equal(stable.blocks[0], previous.blocks[0]);
  assert.notEqual(stable.blocks[1], previous.blocks[0]);
  assert.equal(stable.blocks[1].isWorking, true);
});
