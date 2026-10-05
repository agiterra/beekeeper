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
  buildCodingSessionTranscriptRows,
  CODING_SESSION_VIRTUALIZATION_THRESHOLD,
  CodingSessionTranscript,
  createCodingSessionDisclosureStore,
} from "./CodingSessionTranscript.tsx";
import { deriveCodingSessionTranscriptModel } from "../lib/codingSessionTranscriptModel.ts";
import { deriveCodingSessionMinimapItemsFromModel } from "../lib/codingSessionTranscriptMinimapItems.ts";

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

function planTool(id, plan, explanation = null) {
  return {
    ...tool(id),
    renderClass: "status",
    descriptor: {
      renderClass: "status",
      label: "Updated plan",
      preview: "",
    },
    title: "Update plan",
    toolName: "functions.update_plan",
    args: { explanation, plan },
  };
}

test("renders the latest plan snapshot as a compact expandable narrative row", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Ship it"),
      planTool("plan-1", [
        { step: "Inspect", status: "in_progress" },
        { step: "Implement", status: "pending" },
        { step: "Verify", status: "pending" },
      ]),
      planTool(
        "plan-2",
        [
          { step: "Inspect", status: "completed" },
          { step: "Implement", status: "in_progress" },
          { step: "Verify", status: "pending" },
        ],
        "Working in order",
      ),
    ],
  });

  assert.equal(
    markup.match(/data-testid="coding-session-inline-plan"/g)?.length,
    1,
  );
  assert.match(markup, /Implement/);
  assert.match(markup, />1\/3</);
  assert.match(markup, /data-plan-state="active"/);
  assert.match(markup, /Working for/);
  // The plan is its own row; no "Work Log" heading or "Plan updated" echo
  // repeats it.
  assert.doesNotMatch(markup, /Work Log|Plan updated/);
  assert.doesNotMatch(markup, /transcript-tool-item/);
  assert.doesNotMatch(markup, /Working in order/);
});

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

async function renderTranscript({ open, ...props }) {
  // `open` seeds the disclosure store, standing in for clicks a static
  // render cannot make.
  const transcriptProps = open
    ? { ...props, disclosureStore: createCodingSessionDisclosureStore(open) }
    : props;
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionTranscript, transcriptProps),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("folds a settled turn's work behind one Worked row and keeps the answer", async () => {
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
  // This fixture's prompt carries no operator stamp. It used to read "You";
  // item 10c (batch 2026-09-01) says that was a guess, not a fact.
  assert.match(markup, />Operator not recorded</);
  assert.match(markup, /bg-muted/);
  assert.match(markup, /data-transcript-renderer="static"/);
  // SV-08: no rule above each turn; the hairline sits under the fold row.
  assert.doesNotMatch(markup, /border-t/);
  assert.match(
    markup,
    /class="border-b border-border\/60[^"]*" data-testid="coding-session-worked-fold-row"/,
  );
  assert.match(markup, /Fix the reconnect bug/);
  assert.match(markup, /coding-session-assistant-message/);
  assert.match(markup, /Reconnect now recovers cleanly/);
  // The work is one row: its duration and a sentence of what it did.
  assert.match(markup, /data-testid="coding-session-worked-fold"/);
  assert.match(markup, /aria-expanded="false"/);
  assert.match(markup, /Worked for 3\.6s/);
  assert.match(markup, /Ran 2 commands/);
  assert.doesNotMatch(markup, /data-testid="transcript-tool-item"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-tool-group"/);
  // Cost and copy sit in the quiet completion line, in the document but
  // shown on hover; the duration is not said twice.
  assert.match(markup, /class="coding-session-turn-meta/);
  assert.match(markup, /\$0\.32 estimate/);
  assert.match(markup, /data-testid="coding-session-turn-copy"/);
  assert.equal(markup.match(/3\.6s/g)?.length, 1);
  assert.doesNotMatch(markup, /ceremony key|d7d05d957388/);
  assert.equal(markup.match(/Reconnect now recovers cleanly/g)?.length, 1);
  // The fold row sits before the answer.
  assert.ok(
    markup.indexOf("coding-session-worked-fold") <
      markup.indexOf("coding-session-assistant-message"),
  );
});

test("opening the fold puts every folded entry back in place", async () => {
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
    open: ["fold:turn-1"],
  });

  assert.match(markup, /aria-expanded="true"/);
  assert.match(markup, /data-testid="coding-session-tool-group"/);
  assert.match(markup, /Ran 2 commands/);
  const fold = markup.indexOf("coding-session-worked-fold");
  const group = markup.indexOf("coding-session-tool-group");
  const answer = markup.indexOf("coding-session-assistant-message");
  assert.ok(fold < group && group < answer, "folded work returns in order");
});

test("folds bounded assistant commentary while leaving terminal Markdown visible", async () => {
  const items = [
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
  ];
  const folded = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items,
  });
  assert.match(folded, /Worked for 2\.2s/);
  assert.doesNotMatch(folded, /I’m checking the build logs\./);
  assert.equal(folded.match(/Fixed/g)?.length, 1);

  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    items,
    open: ["fold:turn-1"],
  });
  const foldIndex = markup.indexOf("coding-session-worked-fold");
  const commentaryIndex = markup.indexOf("I’m checking the build logs.");
  const terminalIndex = markup.lastIndexOf("Fixed");
  assert.ok(foldIndex >= 0 && foldIndex < commentaryIndex);
  assert.ok(commentaryIndex < terminalIndex);
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
  assert.doesNotMatch(markup, /coding-session-thinking/);
});

test("a signed running turn animates Thinking only before visible work begins", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [message("prompt", "user", "Inspect the session")],
  });

  assert.match(markup, /data-testid="coding-session-thinking"/);
  assert.match(markup, /coding-session-live-activity-focus/);
  assert.match(markup, />Thinking</);
});

test("a live run of tools reads as one sentence row that opens in place", async () => {
  const items = [
    message("prompt", "user", "Run the checks"),
    tool("tool-1"),
    tool("tool-2"),
    tool("tool-3"),
    tool("tool-4"),
    tool("tool-5"),
  ];
  const closed = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items,
  });

  assert.equal(
    closed.match(/data-testid="coding-session-tool-group"/g)?.length,
    1,
  );
  assert.match(closed, />Ran 5 commands</);
  // Nothing folds while the turn is live, and closed calls are not built.
  assert.doesNotMatch(closed, /coding-session-worked-fold/);
  assert.doesNotMatch(closed, /data-testid="transcript-tool-item"/);
  // The working line follows the work.
  assert.ok(
    closed.indexOf("coding-session-tool-group") <
      closed.indexOf("coding-session-working"),
  );

  const opened = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items,
    open: ["group:tools:tool-1"],
  });
  assert.equal(opened.match(/data-testid="transcript-tool-item"/g)?.length, 5);
  // A closed call row builds only its summary, never its output.
  assert.doesNotMatch(opened, /10 pass/);
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
  assert.match(markup, /\+1/);
  assert.match(markup, /-1/);
  assert.doesNotMatch(markup, /Open diff/);
  // The card is never folded into "Worked for"; its file list and diffs are
  // built when opened.
  assert.doesNotMatch(markup, /const title =/);

  const opened = await renderTranscript({
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
    open: ["changed-files:turn-1", "changed-files:turn-1:src/header.tsx"],
  });
  assert.match(opened, /src\/header\.tsx/);
  assert.match(opened, /const title =/);
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

test("a settled turn's unfinished call reads 'Did not finish', never Running", async () => {
  // The session is known to have stopped while one call never reported an
  // end: it must not show a spinner and "Running"/"Queued" over work that
  // stopped.
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    restingStatus: "stopped",
    items: [
      message("prompt", "user", "Run both checks"),
      tool("executing-1", "executing"),
      tool("pending-1", "pending"),
      message("answer", "assistant", "Both checks were started."),
    ],
    open: ["fold:turn-1"],
  });

  assert.equal(markup.match(/data-tool-unfinished=""/g)?.length, 2);
  assert.equal(markup.match(/Did not finish/g)?.length, 2);
  assert.doesNotMatch(markup, /animate-spin/);
  assert.doesNotMatch(markup, />Running</);
  assert.doesNotMatch(markup, />Queued</);
  assert.doesNotMatch(markup, /role="status"[^>]*coding-session-active-tool/);
});

function turnResult(id, turnId = "turn-1") {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Completed in 2s",
    timestamp,
    turnId,
    durationMs: 2_000,
    costUsd: null,
  };
}

const UNFINISHED = /data-tool-unfinished=""/g;
const STATUS_UNKNOWN = /data-tool-status-unknown=""/g;

test("settled is a fact: a completed turn's unended call did not finish, whatever the session says", async () => {
  for (const restingStatus of ["running", "unknown", "stopped"]) {
    const markup = await renderTranscript({
      generationId: "generation-1",
      isWorking: false,
      restingStatus,
      items: [
        message("prompt", "user", "Run the check"),
        tool("executing-1", "executing"),
        turnResult("result"),
      ],
      open: ["fold:turn-1"],
    });
    assert.equal(markup.match(UNFINISHED)?.length, 1, restingStatus);
    assert.doesNotMatch(markup, />Running</, restingStatus);
    assert.doesNotMatch(markup, />Status unknown</, restingStatus);
  }
});

test("settled is a fact: a turn a later turn followed did not finish, even while the session works", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Run the check"),
      tool("executing-1", "executing"),
      message("prompt-2", "user", "Now do the next thing", "turn-2"),
      { ...tool("executing-2", "executing"), turnId: "turn-2" },
    ],
  });
  // turn-1 is over; turn-2 is the one being worked on.
  assert.equal(markup.match(UNFINISHED)?.length, 1);
  assert.equal(markup.match(/>Running</g)?.length, 1);
  assert.doesNotMatch(markup, STATUS_UNKNOWN);
});

test("an unended call in the working turn reads Running", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    items: [
      message("prompt", "user", "Run the check"),
      tool("executing-1", "executing"),
    ],
  });
  assert.match(markup, />Running</);
  assert.doesNotMatch(markup, UNFINISHED);
  assert.doesNotMatch(markup, STATUS_UNKNOWN);
});

test("with no completion and a status nobody vouches for, an unended call says its status is unknown", async () => {
  // Waiting, disconnected or unread: neither a spinner nor a verdict. Omitted
  // `restingStatus` is the same as `unknown`.
  for (const restingStatus of [undefined, "unknown"]) {
    const markup = await renderTranscript({
      generationId: "generation-1",
      isWorking: false,
      ...(restingStatus ? { restingStatus } : {}),
      items: [
        message("prompt", "user", "Run both checks"),
        tool("executing-1", "executing"),
        tool("pending-1", "pending"),
      ],
    });
    assert.equal(markup.match(STATUS_UNKNOWN)?.length, 2);
    assert.equal(markup.match(/>Status unknown</g)?.length, 2);
    assert.doesNotMatch(markup, UNFINISHED);
    assert.doesNotMatch(markup, />Running</);
    assert.doesNotMatch(markup, />Queued</);
    assert.doesNotMatch(markup, /animate-spin/);
    assert.match(markup, />Coding session status unknown</);
    assert.doesNotMatch(markup, />Coding session idle</);
  }
});

test("a resting status of running keeps an unended call Running", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: false,
    restingStatus: "running",
    items: [
      message("prompt", "user", "Run the check"),
      tool("executing-1", "executing"),
    ],
  });
  assert.match(markup, />Running</);
  assert.doesNotMatch(markup, UNFINISHED);
  assert.doesNotMatch(markup, STATUS_UNKNOWN);
});

test("a fragment transcript draws no second working line or live status", async () => {
  const markup = await renderTranscript({
    generationId: "generation-1",
    isWorking: true,
    showWorkingIndicator: false,
    items: [
      message("prompt", "user", "Run the check"),
      tool("executing-1", "executing"),
    ],
  });
  assert.match(markup, />Running</);
  assert.doesNotMatch(markup, /data-testid="coding-session-live-status"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-working"/);
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
  assert.match(markup, /^(?!.*lucide-circle-x).*data-failure-tone="alarm"/s);
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

/**
 * Operator attribution. Sessions are multi-operator, so a user message is not
 * necessarily the reader's own — before this, a granted operator's turn showed
 * as "You" in every member's client at once.
 */
const LOCAL_OPERATOR = "a".repeat(64);
const FOREIGN_OPERATOR = `b${"c".repeat(63)}`;

function prompt(operatorPubkey) {
  return {
    ...message("prompt", "user", "Ship it"),
    ...(operatorPubkey === undefined ? {} : { operatorPubkey }),
  };
}

function authorLabel(markup) {
  return markup.match(
    /data-testid="coding-session-user-message-author"[^>]*>([^<]*)</,
  )?.[1];
}

test("a prompt the viewer sent is still labelled You", async () => {
  const markup = await renderTranscript({
    currentUserPubkey: LOCAL_OPERATOR,
    generationId: "generation-1",
    isWorking: false,
    items: [prompt(LOCAL_OPERATOR)],
  });

  assert.equal(authorLabel(markup), "You");
});

test("another operator's prompt is labelled with their resolved name", async () => {
  const markup = await renderTranscript({
    currentUserPubkey: LOCAL_OPERATOR,
    generationId: "generation-1",
    isWorking: false,
    items: [prompt(FOREIGN_OPERATOR)],
    operatorProfiles: {
      [FOREIGN_OPERATOR]: { displayName: "Dana", nip05Handle: null },
    },
  });

  assert.equal(authorLabel(markup), "Dana");
});

test("an unresolved foreign operator falls back to a truncated pubkey", async () => {
  const markup = await renderTranscript({
    currentUserPubkey: LOCAL_OPERATOR,
    generationId: "generation-1",
    isWorking: false,
    items: [prompt(FOREIGN_OPERATOR)],
  });

  // Truncated, never the bare hex: a name we do not have is not invented, and
  // a full 64-char key is not a label.
  assert.equal(authorLabel(markup), "bccccccc…cccc");
});

test("item 10c: a prompt published before attribution existed says so", async () => {
  // Was: "…is still labelled You". Brian saw session messages that looked
  // like they came from him, and this was one of the three causes. A person's
  // own typed turn always carries their stamp, so an unstamped prompt is
  // unknown — and unknown is not the reader.
  const markup = await renderTranscript({
    currentUserPubkey: LOCAL_OPERATOR,
    generationId: "generation-1",
    isWorking: false,
    items: [prompt(undefined)],
  });

  assert.equal(authorLabel(markup), "Operator not recorded");
});

test("wide content scrolls inside its block instead of being clipped by the column", async () => {
  const html = await renderTranscript({
    currentUserPubkey: null,
    generationId: "gen-1",
    isWorking: false,
    items: [
      message("m-1", "user", "a".repeat(400)),
      message("m-2", "assistant", "Ran it."),
      tool("t-1"),
    ],
    operatorProfiles: null,
    scrollRef: { current: null },
  });

  // The prompt bubble is shrink-to-fit inside a flex column; without min-w-0 a
  // wide `pre` inside it pushes the bubble past the section's overflow-hidden
  // and the text is cut mid-word instead of scrolling.
  assert.match(
    html,
    /data-testid="coding-session-user-message"[\s\S]{0,200}?class="[^"]*\bmin-w-0\b/,
  );

  // Markdown roots stay inside their parent so a `pre`'s own overflow-x-auto
  // is what engages, rather than an ancestor clipping the block.
  const markdownRoots = html.match(/class="[^"]*message-markdown[^"]*"/g) ?? [];
  assert.ok(markdownRoots.length > 0, "expected a rendered markdown root");
  for (const cls of markdownRoots) {
    assert.ok(
      cls.includes("min-w-0") && cls.includes("w-full"),
      `markdown root must carry w-full min-w-0: ${cls}`,
    );
  }
});

test("tool output wraps rather than scrolling sideways, and unbroken tokens still wrap", async () => {
  const blob = "x".repeat(300);

  // Two distinct renderers own tool output: an executing/pending tool draws its
  // own `pre`, a settled one goes through the shared agents ToolItem panel.
  const running = await renderTranscript({
    currentUserPubkey: null,
    generationId: "gen-1",
    isWorking: true,
    items: [{ ...tool("t-1", "executing"), result: blob }],
    open: ["item:t-1"],
    operatorProfiles: null,
    scrollRef: { current: null },
  });
  const pres = running.match(/<pre class="[^"]*"/g) ?? [];
  assert.ok(pres.length > 0, `expected an active-tool pre: ${running}`);
  for (const pre of pres) {
    assert.ok(pre.includes("whitespace-pre-wrap"), `must wrap: ${pre}`);
    // whitespace-pre-wrap alone will not break a single 300-char token — a long
    // path, hash, or minified payload would still blow past the column.
    assert.ok(pre.includes("wrap-anywhere"), `must break long tokens: ${pre}`);
    assert.ok(pre.includes("max-w-full"), `must stay in the column: ${pre}`);
  }

  const settled = await renderTranscript({
    currentUserPubkey: null,
    generationId: "gen-1",
    isWorking: false,
    items: [{ ...tool("t-1"), result: blob }],
    open: ["item:t-1"],
    operatorProfiles: null,
    scrollRef: { current: null },
  });
  assert.match(settled, /data-testid="transcript-shell-command"/);
  assert.match(settled, /class="[^"]*whitespace-pre-wrap wrap-break-word/);
  assert.doesNotMatch(
    settled,
    /data-testid="transcript-shell-command"[\s\S]*?overflow-x-auto/,
    "settled tool output should wrap, not scroll sideways",
  );
});

test("a steered prompt shows a visible marker beside its author; an ordinary one does not", async () => {
  const markup = await renderTranscript({
    currentUserPubkey: LOCAL_OPERATOR,
    generationId: "generation-1",
    isWorking: true,
    items: [
      prompt(LOCAL_OPERATOR),
      {
        ...prompt(LOCAL_OPERATOR),
        id: "steer",
        text: "Also the tests",
        steered: true,
      },
    ],
  });
  const markers = markup.match(
    /data-testid="coding-session-user-message-steered"[^>]*>([^<]*)</g,
  );
  assert.equal(markers?.length, 1, markup);
  assert.match(markers[0], />steered</);
  // Beside the author, in the same byline, not hidden in a title attribute.
  assert.match(
    markup,
    /data-testid="coding-session-user-message-author"[^>]*>You<\/span><span[^>]*data-testid="coding-session-user-message-steered"/,
  );
});

function resultWithOutcome(outcome) {
  return {
    id: "result",
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Partial answer. (1200ms)",
    outcome,
    timestamp,
    turnId: "turn-1",
    bridgeSource,
  };
}

test("an abnormal outcome is always on screen, outside the hover metadata", async () => {
  for (const outcome of ["max_tokens", "refusal"]) {
    const markup = await renderTranscript({
      generationId: "generation-1",
      isWorking: false,
      items: [
        message("prompt", "user", "Write it all"),
        message("answer", "assistant", "Partial answer."),
        resultWithOutcome(outcome),
      ],
    });
    const words = outcome.replace("_", " ");
    assert.match(
      markup,
      new RegExp(
        `data-testid="coding-session-turn-outcome"[^>]*>.*Ended: ${words}<`,
      ),
    );
    // Not inside the opacity-0 metadata span.
    const meta = markup.match(
      /<span class="coding-session-turn-meta[\s\S]*?data-testid="coding-session-turn-meta">([\s\S]*?)<\/span><\/div>/,
    );
    assert.ok(meta, markup);
    assert.doesNotMatch(meta[1], new RegExp(words));
  }
});

test("a normal end of turn shows no outcome badge", async () => {
  for (const outcome of ["end_turn", "success", undefined]) {
    const markup = await renderTranscript({
      generationId: "generation-1",
      isWorking: false,
      items: [
        message("prompt", "user", "Write it"),
        message("answer", "assistant", "Partial answer."),
        resultWithOutcome(outcome),
      ],
    });
    assert.doesNotMatch(markup, /coding-session-turn-outcome/);
    assert.doesNotMatch(markup, /end turn/);
  }
});

test("SV-26: a minimap item's rowIndex is its virtualizer row, keyed alike", () => {
  // Past the threshold the minimap jumps through `scrollToIndex(rowIndex)`,
  // so each item's index must be the row the virtualizer draws its turn at.
  const items = [];
  for (let n = 1; n <= CODING_SESSION_VIRTUALIZATION_THRESHOLD + 2; n += 1) {
    const turnId = `turn-${n}`;
    if (n % 7 !== 0)
      items.push(message(`p${n}`, "user", `Prompt ${n}`, turnId));
    items.push(message(`a${n}`, "assistant", `Reply ${n}`, turnId));
  }
  const model = deriveCodingSessionTranscriptModel(items, { isWorking: false });
  const rows = buildCodingSessionTranscriptRows(model);
  const minimap = deriveCodingSessionMinimapItemsFromModel(model, null);
  assert.ok(rows.length > CODING_SESSION_VIRTUALIZATION_THRESHOLD);
  // Every seventh turn has no prompt and gets no dash.
  assert.equal(
    minimap.length,
    CODING_SESSION_VIRTUALIZATION_THRESHOLD +
      2 -
      Math.floor((CODING_SESSION_VIRTUALIZATION_THRESHOLD + 2) / 7),
  );
  for (const item of minimap) {
    assert.equal(rows[item.rowIndex]?.key, item.key, item.id);
  }
});
