import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionChangedFiles,
  deriveCodingSessionObservedChanges,
  deriveCodingSessionTranscriptModel,
  formatCodingSessionDuration,
  isCodingSessionTranscriptError,
  parseCodingSessionTurnResult,
  stabilizeCodingSessionTranscriptModel,
  summarizeCodingSessionTools,
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

function planTool(id, plan) {
  return {
    ...tool({ id, renderClass: "status" }),
    descriptor: {
      renderClass: "status",
      label: "Updated plan",
      preview: "",
    },
    title: "Update plan",
    toolName: "functions.update_plan",
    args: { plan },
  };
}

test("coalesces replacement plan snapshots at their first narrative position", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Ship it" }),
      planTool("plan-1", [
        { step: "Inspect", status: "in_progress" },
        { step: "Verify", status: "pending" },
      ]),
      tool({ id: "read" }),
      planTool("plan-2", [
        { step: "Inspect", status: "completed" },
        { step: "Verify", status: "in_progress" },
      ]),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.entries.map((entry) =>
      entry.kind === "item" ? entry.item.id : entry.id,
    ),
    ["prompt", "plan-2", "read"],
  );
});

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
    costBasis: null,
    outcome: null,
    timestamp,
    state: "completed",
  });
});

test("structured duration and cost win over the legacy text-baked suffixes", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Say hello" }),
      message({ id: "answer", role: "assistant", text: "Hey Brian." }),
      // A structured item: clean text, metrics as fields — what the builder
      // now emits for every event that carries them on the wire.
      {
        ...lifecycle({
          id: "result",
          title: "Turn result",
          text: "Hey Brian.",
        }),
        durationMs: 3557,
        costUsd: 0.3209,
      },
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(turn.completion, {
    durationMs: 3557,
    costUsd: 0.3209,
    costBasis: null,
    outcome: null,
    timestamp,
    state: "completed",
  });
  // Presentation identical to the legacy path: the echo is deduplicated and
  // no metric suffix leaks into any visible row.
  assert.equal(
    turn.entries.filter(
      (entry) =>
        entry.kind === "item" &&
        entry.item.type === "message" &&
        entry.item.role === "assistant",
    ).length,
    1,
  );
  for (const entry of turn.entries) {
    if (entry.kind === "item") {
      assert.doesNotMatch(entry.item.text, /\(3557ms\)|\(\$0\.3209\)/);
    }
  }
});

test("structured fields beat mismatched legacy suffixes on the same item", () => {
  // A hybrid should never occur, but if it does the structured claim is the
  // authoritative one and the text suffix is treated as prose to strip.
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Say hello" }),
      {
        ...lifecycle({
          id: "result",
          title: "Turn result",
          text: "Done. (999ms) ($0.9999)",
        }),
        durationMs: 3557,
        costUsd: 0.3209,
      },
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.completion.durationMs, 3557);
  assert.equal(turn.completion.costUsd, 0.3209);
});

test("a structured error result keeps its clean text and fails the turn", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Build it" }),
      {
        ...lifecycle({
          id: "result",
          title: "Turn result",
          text: "Compiler exited with status 1",
          renderClass: "error",
        }),
        durationMs: 900,
        costUsd: 0.004,
      },
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.completion.state, "failed");
  assert.equal(turn.completion.durationMs, 900);
  assert.equal(turn.completion.costUsd, 0.004);
  const errorRow = turn.entries.find(
    (entry) => entry.kind === "item" && entry.item.type === "lifecycle",
  );
  assert.equal(errorRow.item.text, "Compiler exited with status 1");
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
    costBasis: null,
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

test("deduplicates only the result echo, preserving every assistant word", () => {
  // Consecutive assistant items are slices of one stream and now read as one
  // message; the echo check must still drop only the result body, never a
  // piece of the prose itself.
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Repeat the update" }),
      message({ id: "answer-1", role: "assistant", text: "Still working. " }),
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
  const assistant = turn.entries.flatMap((entry) =>
    entry.kind === "item" &&
    entry.item.type === "message" &&
    entry.item.role === "assistant"
      ? [entry.item]
      : [],
  );
  assert.deepEqual(
    assistant.map((item) => [item.id, item.text]),
    [["answer-1", "Still working. Still working."]],
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

test("suppresses an exact ceremonial result body while keeping turn completion", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Acknowledge" }),
      lifecycle({
        id: "result",
        title: "Turn result",
        text: "completed",
      }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.entries.map((entry) =>
      entry.kind === "item" ? entry.item.id : entry.id,
    ),
    ["prompt"],
  );
  assert.equal(turn.completion.state, "completed");
});

test("groups each run of settled successful tools into one sentence row", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Run checks" }),
      tool({ id: "tool-1" }),
      tool({ id: "tool-2" }),
      tool({ id: "tool-3", renderClass: "file-read" }),
      tool({ id: "tool-4" }),
      tool({ id: "tool-5", renderClass: "file-edit" }),
      tool({ id: "tool-failed", isError: true }),
      tool({ id: "tool-6" }),
    ],
    { isWorking: true },
  );
  const turn = model.blocks[0];

  assert.equal(turn.kind, "turn");
  assert.deepEqual(
    turn.entries.map((entry) => [
      entry.kind,
      entry.kind === "item" ? entry.item.id : entry.id,
    ]),
    [
      ["item", "prompt"],
      ["tool-group", "tools:tool-1"],
      // A failure is never grouped, and it ends the run.
      ["item", "tool-failed"],
      // A run of one stays an ordinary row.
      ["item", "tool-6"],
    ],
  );
  // T3's sentence names two kinds — commands and edits outrank reads — and
  // counts the rest, so nothing goes uncounted.
  assert.equal(
    turn.entries[1].label,
    "Ran 3 commands, changed 1 file, and performed 1 other action",
  );
  assert.equal(turn.entries[1].items.length, 5);
  assert.equal(turn.isWorking, true);
  assert.equal(turn.fold, null, "nothing folds while the turn is live");
});

test("tool sentence counts files once each and calls otherwise", () => {
  const read = (id, path) => ({
    ...tool({ id, renderClass: "file-read" }),
    args: { file_path: path },
  });
  assert.equal(
    summarizeCodingSessionTools([
      read("r1", "src/a.ts"),
      read("r2", "src/a.ts"),
      read("r3", "src/b.ts"),
      tool({ id: "s1" }),
      tool({ id: "s2" }),
    ]),
    "Read 2 files and ran 2 commands",
  );
  const generic = (id) => ({
    ...tool({ id, renderClass: "generic" }),
    title: "lookup_weather",
    toolName: "lookup_weather",
  });
  assert.equal(
    summarizeCodingSessionTools([generic("g1"), generic("g2")]),
    "Used 2 tools",
  );
  assert.equal(summarizeCodingSessionTools([]), "");
});

test("a single settled tool stays an ordinary row", () => {
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "Run checks" }),
      tool({ id: "tool-1" }),
    ],
    { isWorking: true },
  );
  const turn = model.blocks[0];

  assert.deepEqual(
    turn.entries.map((entry) =>
      entry.kind === "item" ? entry.item.id : entry.id,
    ),
    ["prompt", "tool-1"],
  );
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

test("counts an ACP edit the name classifier does not recognize", () => {
  // claude-agent-acp calls its editor `Edit`, which no developer-harness name
  // rule matches, so the descriptor lands on "generic". ACP's own `edit`
  // discriminant is the authoritative signal and the fold reads it.
  const edit = tool({ id: "edit", renderClass: "generic" });
  edit.descriptor = {
    renderClass: "generic",
    label: "Tool",
    preview: null,
    object: null,
  };
  edit.toolKind = "edit";
  edit.args = { file_path: "src/app.ts" };
  edit.result = "";

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

test("names a file from the provider's published edit locations", () => {
  const edit = tool({ id: "edit", renderClass: "generic" });
  edit.descriptor = {
    renderClass: "generic",
    label: "Tool",
    preview: null,
    object: null,
  };
  edit.toolKind = "edit";
  edit.args = {};
  edit.editPaths = ["desktop/src/App.tsx"];
  edit.result = "";

  const observed = deriveCodingSessionObservedChanges([edit]);
  assert.equal(observed.unreportedEditCount, 0);
  assert.deepEqual(observed.files, [
    {
      path: "desktop/src/App.tsx",
      filename: "App.tsx",
      additions: null,
      deletions: null,
      editCount: 1,
      diffs: [],
    },
  ]);
});

test("counts edits that named no file instead of reporting none", () => {
  // The 2026-08-29 walk: 19 signed edits, every one with an empty payload. The
  // surface said "No observed changes yet" over all of them.
  const items = [0, 1, 2].map((index) => {
    const edit = tool({ id: `edit-${index}`, renderClass: "generic" });
    edit.descriptor = {
      renderClass: "generic",
      label: "Tool",
      preview: null,
      object: null,
    };
    edit.toolKind = "edit";
    edit.args = {};
    edit.result = "";
    return edit;
  });

  const observed = deriveCodingSessionObservedChanges(items);
  assert.deepEqual(observed.files, []);
  assert.equal(observed.unreportedEditCount, 3);
});

test("reports no unnamed edits when there were no edits at all", () => {
  const shell = tool({ id: "shell" });
  const observed = deriveCodingSessionObservedChanges([shell]);
  assert.deepEqual(observed.files, []);
  assert.equal(observed.unreportedEditCount, 0);
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

/**
 * L4.2, found live 2026-09-01 at 12:36: `FILES` printed
 * `[elided private context: 183 bytes, sha256:b35397…]` in the path slot and
 * `CHANGES` counted it as a second named edit. A marker is the provider saying
 * it had a path and chose not to publish it — which is the definition of an
 * edit with no reported file name, not a file.
 */
test("a redaction marker in the path slot is an unreported edit, not a file", () => {
  const named = tool({ id: "edit-named", renderClass: "generic" });
  named.descriptor = {
    renderClass: "generic",
    label: "Tool",
    preview: null,
    object: null,
  };
  named.toolKind = "edit";
  named.args = { file_path: "crates/buzz-core/src/kind.rs" };
  named.result = "";

  const elided = tool({ id: "edit-elided", renderClass: "generic" });
  elided.descriptor = {
    renderClass: "generic",
    label: "Tool",
    preview: null,
    object: null,
  };
  elided.toolKind = "edit";
  elided.args = {
    file_path: `[elided private context: 183 bytes, sha256:${"b3".repeat(32)}]`,
  };
  elided.result = "";

  const observed = deriveCodingSessionObservedChanges([named, elided]);
  assert.equal(observed.files.length, 1);
  assert.equal(observed.files[0].path, "crates/buzz-core/src/kind.rs");
  assert.equal(observed.unreportedEditCount, 1);
  // The bytes and the digest are not re-surfaced: a redaction disclosed as a
  // redaction is the whole point.
  assert.ok(!JSON.stringify(observed).includes("sha256:"));
  assert.ok(!JSON.stringify(observed).includes("183 bytes"));
});

test("a real path that merely contains the word elided still names a file", () => {
  const edit = tool({ id: "edit-elided-name", renderClass: "generic" });
  edit.descriptor = {
    renderClass: "generic",
    label: "Tool",
    preview: null,
    object: null,
  };
  edit.toolKind = "edit";
  edit.args = { file_path: "src/elided.rs" };
  edit.result = "";

  const observed = deriveCodingSessionObservedChanges([edit]);
  assert.equal(observed.unreportedEditCount, 0);
  assert.deepEqual(
    observed.files.map((file) => file.path),
    ["src/elided.rs"],
  );
});
