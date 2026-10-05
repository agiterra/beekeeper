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
  CodingSessionTranscript,
  createCodingSessionDisclosureStore,
} from "./CodingSessionTranscript.tsx";
import { CodingSessionUmbrellaTurnBlock } from "./CodingSessionUmbrellaTurnBlock.tsx";
import {
  compactToolGroupIcon,
  compactToolRowIcon,
} from "../../agents/ui/AgentSessionToolItem/ToolItemRowIcon.tsx";

/**
 * Session-view parity, Wave B audit of the Wave A transcript presentation
 * (lane audit-transcript-view): SV-01, SV-02, SV-05, SV-06, SV-07, SV-08,
 * SV-15, measured against T3 Code's `WorkLog.tsx` and `MessagesTimeline.tsx`.
 */

const timestamp = "2026-07-30T12:00:00.000Z";

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
  };
}

function tool(id, overrides = {}) {
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
    status: "completed",
    args: { command: "bun test" },
    result: "10 pass",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId: "turn-1",
    ...overrides,
  };
}

function result(text = "Done. (3557ms)") {
  return {
    id: "result",
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text,
    timestamp,
    turnId: "turn-1",
  };
}

async function renderInRouter(element) {
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

function renderTranscript({ open, ...props }) {
  return renderInRouter(
    React.createElement(CodingSessionTranscript, {
      generationId: "generation-1",
      ...props,
      ...(open
        ? { disclosureStore: createCodingSessionDisclosureStore(open) }
        : {}),
    }),
  );
}

test("SV-01/SV-06: a tool row leads with its kind glyph in T3's 24px icon box", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: [message("prompt", "user", "Run it"), tool("tool-1")],
  });
  const summary = markup.match(
    /data-testid="transcript-tool-item"[\s\S]*?<summary class="([^"]*)"[^>]*>([\s\S]*?)<\/summary>/,
  );
  assert.ok(summary, markup);
  // T3 WorkLogLine geometry: px-0.5, gap-1.5, icon box 24px (mx-1 + 16px).
  assert.match(summary[1], /\bpx-0\.5\b/);
  assert.match(summary[1], /\bgap-1\.5\b/);
  assert.match(summary[1], /hover:bg-accent\/30/);
  assert.match(
    summary[2],
    /lucide-square-terminal mx-1 size-4 shrink-0 text-muted-foreground opacity-70" aria-hidden="true" data-testid="transcript-tool-row-icon"/,
  );
});

test("SV-06: the row glyph follows T3's choice by kind, a hammer for a mix", () => {
  const shell = { kind: "shell", action: { verb: "Ran", object: "ls" } };
  const read = { kind: "file-read", action: { verb: "Read", object: "a" } };
  const search = { kind: "generic", action: { verb: "Searched", object: "x" } };
  assert.equal(compactToolRowIcon(shell).displayName, "SquareTerminal");
  assert.equal(compactToolRowIcon(read).displayName, "Eye");
  assert.equal(compactToolRowIcon(search).displayName, "Search");
  assert.equal(
    compactToolRowIcon({ kind: "file-edit", action: null }).displayName,
    "SquarePen",
  );
  assert.equal(
    compactToolRowIcon({ kind: "generic", action: null }).displayName,
    "Wrench",
  );
  assert.equal(
    compactToolGroupIcon([shell, shell]).displayName,
    "SquareTerminal",
  );
  assert.equal(compactToolGroupIcon([shell, read]).displayName, "Hammer");
});

test("SV-02: a failed step in an opened fold keeps its glyph, dimmed red, announced", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Run it"),
      tool("bad", {
        status: "failed",
        isError: true,
        args: { command: "python3 check.py" },
        result: JSON.stringify({ stdout: "", stderr: "boom", exitCode: 2 }),
      }),
      message("answer", "assistant", "Fixed it another way."),
      result("Fixed it another way. (1200ms)"),
    ],
    open: ["fold:turn-1"],
  });
  const icon = markup.match(
    /<svg[^>]*data-testid="transcript-tool-row-icon"[^>]*>/,
  )?.[0];
  assert.ok(icon, markup);
  assert.match(icon, /aria-label="Failed"/);
  assert.match(icon, /role="img"/);
  assert.match(icon, /data-failure-tone="quiet"/);
  assert.match(icon, /text-destructive\/40/);
  assert.doesNotMatch(icon, /opacity-70/);
  // The summary row drops its own CircleX (`hasLeadingIcon`), so the failure
  // mark is drawn once, on the glyph; its words stay.
  assert.doesNotMatch(markup, /lucide-circle-x/);
  assert.equal(markup.match(/aria-label="Failed"/g)?.length, 1);
  assert.match(markup, /· exit 2/);
});

test("SV-02: a kept failure stays loud on the glyph too", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Run it"),
      message("answer", "assistant", "Running the check now."),
      tool("bad", { status: "failed", isError: true, result: "Exit code 1" }),
      result("Done. (1200ms)"),
    ],
  });
  const icon = markup.match(
    /<svg[^>]*data-testid="transcript-tool-row-icon"[^>]*>/,
  )?.[0];
  assert.ok(icon, markup);
  assert.match(icon, /data-failure-tone="alarm"/);
  assert.match(icon, /\btext-destructive\b(?!\/)/);
  assert.match(markup, /Tool call failed/);
});

test("SV-01/SV-06: a run of calls is one row led by the calls' glyph, no chevron, members unindented", async () => {
  const items = [
    message("prompt", "user", "Run them"),
    tool("tool-1"),
    tool("tool-2"),
    tool("tool-3"),
  ];
  const closed = await renderTranscript({ isWorking: true, items });
  const group = closed.match(
    /data-testid="coding-session-tool-group"[\s\S]*?<\/button>/,
  )?.[0];
  assert.ok(group, closed);
  assert.match(group, /lucide-square-terminal mx-1 size-4/);
  assert.doesNotMatch(group, /lucide-chevron/);
  const open = await renderTranscript({
    isWorking: true,
    items,
    open: ["group:tools:tool-1"],
  });
  assert.match(open, /data-open="" data-testid="coding-session-tool-group"/);
  // Opened, the calls sit on the header's icon column (T3's WorkLogList).
  assert.equal(
    open.match(/data-testid="transcript-tool-row-icon"/g)?.length,
    3,
  );
  assert.doesNotMatch(open, /border-l border-border\/60 pl-3/);
});

test("SV-05: an opened thought's text starts under its label, 32px in", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: [
      message("prompt", "user", "Think"),
      {
        id: "t-1",
        type: "thought",
        renderClass: "thought",
        title: "Reasoning",
        text: "Considering the reconnect path.",
        timestamp,
        turnId: "turn-1",
      },
    ],
    open: ["item:t-1"],
  });
  assert.match(
    markup,
    /<div class="ps-8 pe-1 pt-1 pb-1\.5 text-sm leading-5 text-muted-foreground">/,
  );
});

test("SV-07: a prompt's author is always shown; its time and copy wait for hover", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Fix the reconnect bug"),
      message("answer", "assistant", "Done."),
      result(),
    ],
  });
  const prompt = markup.match(
    /data-testid="coding-session-user-message"[\s\S]*?data-testid="coding-session-user-message-copy"[\s\S]*?<\/button>/,
  )?.[0];
  assert.ok(prompt, markup);
  assert.match(prompt, /^data-testid="coding-session-user-message"/);
  assert.match(markup, /class="group\/prompt flex flex-col items-end gap-1"/);
  // Author outside the hover cluster.
  const meta = prompt.match(
    /<span class="coding-session-prompt-meta[^"]*"[^>]*>([\s\S]*)$/,
  )?.[1];
  assert.ok(meta, prompt);
  assert.doesNotMatch(meta, /coding-session-user-message-author/);
  assert.match(prompt, /coding-session-user-message-author/);
  // Time first, then copy, as T3's prompt footer.
  assert.match(
    meta,
    /<time data-testid="coding-session-user-message-time" dateTime="2026-07-30T12:00:00.000Z"[^>]*>[^<]+<\/time><button aria-label="Copy message"/,
  );
  // The answer's copy icon is T3's 12px.
  assert.match(
    markup,
    /data-testid="coding-session-turn-copy"[^>]*><svg[^>]*class="lucide lucide-copy size-3"/,
  );
});

function umbrellaBlock(items) {
  return {
    kind: "turn",
    executionKey: "exec-1",
    generationId: "generation-1",
    generation: 1,
    signerPubkey: "a".repeat(64),
    turnId: "turn-1",
    items,
  };
}

test("SV-08: an umbrella turn block draws no rule above itself", async () => {
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      block: umbrellaBlock([
        message("prompt", "user", "Fix it"),
        message("answer", "assistant", "Done."),
      ]),
      blockKey: "block-1",
      channelId: "channel-1",
      currentUserPubkey: null,
      isHighlighted: false,
      isFolded: false,
      isWorking: false,
      label: null,
      labelsByExecutionKey: new Map(),
      onHandoff: () => {},
      onRegisterNode: () => {},
      onRevealFact: () => {},
      operatorProfiles: undefined,
      record: null,
      resolveFactLocation: () => null,
      showProvenance: false,
      stickyProvenance: false,
      umbrella: {
        executions: [],
        founderPubkey: null,
        sessionRef: "session-1",
      },
    }),
  );
  const article = markup.match(
    /<article class="([^"]*)"[^>]*data-testid="coding-session-umbrella-turn-block"/,
  )?.[1];
  assert.ok(article, markup);
  assert.doesNotMatch(article, /\bborder-t\b/);
  // The identity rail stays: it says whose turn this is.
  assert.match(article, /\bborder-l-2\b/);
});
