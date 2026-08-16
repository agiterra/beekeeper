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
  CODING_SESSION_VIRTUALIZATION_THRESHOLD,
  CodingSessionTranscript,
} from "./CodingSessionTranscript.tsx";

const timestamp = "2026-07-30T12:00:00.000Z";
const bridgeSource = {
  label: "keystone hive-session bridge (ceremony key)",
  pubkey: "d7d05d957388",
};

function message(id, role, text, turnId = "turn-1") {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId,
    bridgeSource,
  };
}

function tool(id, status = "completed") {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: {
      renderClass: "shell",
      label: "Ran command",
      preview: "bun test",
      action: { verb: "Ran", object: "bun test" },
    },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status,
    args: { command: "bun test" },
    result: "10 pass",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId: "turn-1",
    bridgeSource,
  };
}

function fileEdit(id, path, result) {
  return {
    id,
    type: "tool",
    renderClass: "file-edit",
    descriptor: {
      renderClass: "file-edit",
      label: "Edited file",
      preview: path,
      action: { verb: "Edited", object: path },
      object: path,
    },
    title: "Edit",
    toolName: "Edit",
    buzzToolName: null,
    status: "completed",
    args: { file_path: path },
    result,
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId: "turn-1",
    bridgeSource,
  };
}

async function renderTranscript(props) {
  const rootRoute = createRootRoute({
    component: () => React.createElement(CodingSessionTranscript, props),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("renders a settled turn with recent work visible in the narrative", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Fix the reconnect bug"),
      tool("tool-1"),
      tool("tool-2"),
      message("answer", "assistant", "Reconnect now recovers cleanly."),
      {
        id: "result",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Reconnect now recovers cleanly. (3557ms) ($0.3209)",
        timestamp,
        turnId: "turn-1",
        bridgeSource,
      },
    ],
  });

  assert.match(markup, /coding-session-user-message/);
  assert.match(markup, />You</);
  assert.match(markup, /bg-muted/);
  assert.match(markup, /data-transcript-renderer="static"/);
  assert.match(markup, /Fix the reconnect bug/);
  assert.match(markup, /coding-session-assistant-message/);
  assert.match(markup, /Reconnect now recovers cleanly/);
  assert.doesNotMatch(markup, /coding-session-worked-fold/);
  assert.equal(markup.match(/data-testid="transcript-tool-item"/g)?.length, 2);
  assert.match(markup, /Worked for 3\.6s/);
  assert.match(markup, /\$0\.32/);
  assert.doesNotMatch(markup, /ceremony key|d7d05d957388/);
  assert.equal(markup.match(/Reconnect now recovers cleanly/g)?.length, 1);
  assert.equal(markup.match(/3\.6s/g)?.length, 1);
});

test("folds bounded assistant commentary while leaving terminal Markdown visible", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Investigate the failure"),
      message("commentary", "assistant", "I’m checking the build logs."),
      tool("tool-1"),
      message("answer", "assistant", "**Fixed** the build."),
      {
        id: "result",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "**Fixed** the build. (2200ms)",
        timestamp,
        turnId: "turn-1",
        bridgeSource,
      },
    ],
  });

  const foldIndex = markup.indexOf("coding-session-worked-fold");
  const commentaryIndex = markup.indexOf("I’m checking the build logs.");
  const terminalIndex = markup.lastIndexOf("Fixed");
  assert.ok(foldIndex >= 0 && foldIndex < commentaryIndex);
  assert.ok(commentaryIndex < terminalIndex);
  assert.ok(markup.lastIndexOf("</details>") < terminalIndex);
  assert.equal(markup.match(/Fixed/g)?.length, 1);
});

test("renders an unscoped signed terminal cycle once with compact worked metadata", async () => {
  const sessionId = "generation-2";
  const markup = await renderTranscript({
    generationId: sessionId,
    isWorking: false,
    items: [
      {
        ...message("answer", "assistant", "Hey Brian.", null),
        sessionId,
      },
      {
        id: "context",
        type: "lifecycle",
        renderClass: "status",
        title: "Context Window Updated",
        text: "inputTokens: 66546",
        timestamp,
        turnId: null,
        sessionId,
        bridgeSource,
      },
      {
        id: "result",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Hey Brian. (3557ms) ($0.3209)",
        outcome: "success",
        timestamp,
        turnId: null,
        sessionId,
        bridgeSource,
      },
    ],
  });

  assert.equal(markup.match(/Hey Brian\./g)?.length, 1);
  assert.doesNotMatch(markup, /Turn result/);
  assert.match(markup, /Worked for 3\.6s/);
  assert.match(markup, /\$0\.32/);
  assert.doesNotMatch(markup, /Turn details|Context Window Updated/);
  assert.doesNotMatch(markup, />Completed</);
});

test("omits valid system init metadata from the transcript timeline", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      {
        id: "system-init",
        type: "lifecycle",
        renderClass: "status",
        title: "System Init",
        text: [
          "provider: claude",
          "model: sonnet",
          "tools: Bash, Edit",
          "mcpServers: chrome (connected), fetch (failed)",
        ].join("\n"),
        timestamp,
        bridgeSource,
      },
      message("prompt", "user", "Hello"),
    ],
  });

  assert.match(markup, /coding-session-user-message/);
  assert.match(markup, /Hello/);
  assert.doesNotMatch(markup, /coding-session-diagnostics/);
  assert.doesNotMatch(markup, /System Init|provider: claude|fetch \(failed\)/);
  assert.doesNotMatch(markup, /coding-session-error/);
  assert.doesNotMatch(markup, /ceremony key|d7d05d957388/);
});

test("shows errors and a streaming working affordance in the primary turn", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Run the build"),
      {
        id: "error",
        type: "lifecycle",
        renderClass: "error",
        title: "Build failed",
        text: "Compiler exited with status 1",
        timestamp,
        turnId: "turn-1",
      },
    ],
  });

  assert.match(markup, /coding-session-error/);
  assert.match(markup, /Build failed/);
  assert.match(markup, /Compiler exited with status 1/);
  assert.match(markup, /coding-session-working/);
  assert.match(markup, /Working for/);
});

test("keeps three successful tools visible after the previous-tool disclosure", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Run the checks"),
      tool("tool-1"),
      tool("tool-2"),
      tool("tool-3"),
      tool("tool-4"),
      tool("tool-5"),
    ],
  });

  assert.match(markup, /coding-session-tool-group/);
  assert.match(markup, /\+2 previous tool calls/);
  assert.doesNotMatch(markup, /coding-session-worked-fold/);
  assert.match(markup, /Show fewer tool calls/);
  assert.equal(markup.match(/data-testid="transcript-tool-item"/g)?.length, 5);
  assert.doesNotMatch(
    markup,
    /coding-session-tool-group[^>]+border border-border/,
  );
});

test("renders truthful per-turn changed files and inline signed diffs", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Update the header"),
      fileEdit(
        "edit-1",
        "src/header.tsx",
        [
          "diff --git a/src/header.tsx b/src/header.tsx",
          "--- a/src/header.tsx",
          "+++ b/src/header.tsx",
          "@@ -1 +1 @@",
          "-const title = 'Old';",
          "+const title = 'Buzz';",
        ].join("\n"),
      ),
      message("answer", "assistant", "Updated the header."),
    ],
  });

  assert.match(markup, /coding-session-changed-files/);
  assert.match(markup, /1 changed file/);
  assert.match(markup, /src\/header\.tsx/);
  assert.match(markup, /\+1/);
  assert.match(markup, /-1/);
  assert.match(markup, /const title =/);
  assert.doesNotMatch(markup, /Open diff/);
});

test("omits invented diff stats when a signed edit has only a path", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Touch the header"),
      fileEdit("edit-1", "src/header.tsx", "File updated successfully."),
      message("answer", "assistant", "Updated the header."),
    ],
  });

  assert.match(markup, /1 changed file/);
  assert.match(markup, /src\/header\.tsx/);
  assert.doesNotMatch(markup, /text-emerald-600|text-rose-600|Open diff/);
});

test("renders a failed Turn result as Failed without a successful completion check", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Run the build"),
      {
        id: "result",
        type: "lifecycle",
        renderClass: "error",
        title: "Turn result",
        text: "Compiler exited with status 1 (900ms) ($0.0040)",
        timestamp,
        turnId: "turn-1",
      },
    ],
  });

  assert.match(markup, /data-turn-state="failed"/);
  assert.match(markup, />Failed</);
  assert.match(markup, /coding-session-error/);
  assert.match(markup, /Compiler exited with status 1/);
  assert.doesNotMatch(markup, />Completed</);
});

test("keeps executing and pending tools as individual active rows", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Run both checks"),
      tool("executing-1", "executing"),
      tool("pending-1", "pending"),
    ],
  });

  assert.equal(markup.match(/coding-session-active-tool/g)?.length, 2);
  assert.match(markup, /data-tool-status="executing"/);
  assert.match(markup, /data-tool-status="pending"/);
  assert.match(markup, /Running/);
  assert.match(markup, /Queued/);
  assert.doesNotMatch(markup, /coding-session-tool-group|Ran 2 commands/);
});

test("renders failed tools as compact semantic rows while preserving lifecycle errors", async () => {
  const failedTool = {
    ...tool("failed-tool", "failed"),
    isError: true,
    result: "Exit code 1\ncompiler failed",
  };
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items: [
      message("prompt", "user", "Run the build"),
      failedTool,
      {
        id: "provider-error",
        type: "lifecycle",
        renderClass: "error",
        title: "Provider error",
        text: "Connection dropped",
        timestamp,
        turnId: "turn-1",
      },
    ],
  });

  assert.equal(markup.match(/data-testid="transcript-tool-item"/g)?.length, 1);
  assert.equal(markup.match(/data-testid="coding-session-error"/g)?.length, 1);
  assert.match(markup, /Tool call failed/);
  assert.match(markup, /lucide-circle-x/);
  assert.match(markup, /Exit code 1/);
  assert.match(markup, /Provider error/);
});

test("virtualizes large turn histories instead of mounting the entire transcript", async () => {
  const turnCount = CODING_SESSION_VIRTUALIZATION_THRESHOLD + 60;
  const items = Array.from({ length: turnCount }, (_, index) => {
    const turnId = `turn-${index}`;
    return [
      message(`prompt-${index}`, "user", `Prompt number ${index}`, turnId),
      message(`answer-${index}`, "assistant", `Answer number ${index}`, turnId),
    ];
  }).flat();

  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items,
    scrollRef: { current: null },
  });

  assert.match(markup, /data-transcript-renderer="virtualized"/);
  assert.match(markup, /role="log"/);
  assert.match(markup, /aria-live="off"/);
  assert.match(markup, /data-testid="coding-session-live-status"/);
  assert.match(markup, /Coding session working/);
  assert.ok(
    (markup.match(/data-testid="coding-session-turn"/g)?.length ?? 0) <
      turnCount,
    "the virtual window must not mount every historical turn",
  );
});
