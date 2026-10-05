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

import { deriveCodingSessionTranscriptModel } from "../lib/codingSessionTranscriptModel.ts";
import { CodingSessionSubagentEntry } from "./CodingSessionSubagentEntry.tsx";
import {
  buildCodingSessionTranscriptRows,
  CodingSessionTranscript,
  createCodingSessionDisclosureStore,
  estimateCodingSessionTranscriptRowSize,
} from "./CodingSessionTranscript.tsx";
import { CodingSessionWorkedFold } from "./CodingSessionTranscriptCompletion.tsx";
import {
  codingSessionRowGap,
  codingSessionTurnTailGap,
  formatCodingSessionBlockTime,
} from "./CodingSessionTranscriptRhythm.ts";
import { shouldClampCodingSessionUserMessage } from "./CodingSessionTranscriptUserMessage.tsx";

/**
 * Session-view parity with T3 Code, Wave A presentation (lane A3): SV-01,
 * SV-05, SV-06 (visuals), SV-07, SV-08, SV-15. Each test names the ID whose
 * visible behaviour it pins.
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

function thought(id, title = "Reasoning") {
  return {
    id,
    type: "thought",
    renderClass: "thought",
    title,
    text: "Considering the reconnect path in depth.",
    timestamp,
    turnId: "turn-1",
  };
}

function result(text = "Done. (3557ms) ($0.3209)") {
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

const settledTurn = [
  message("prompt", "user", "Fix the reconnect bug"),
  tool("tool-1"),
  tool("tool-2"),
  message("answer", "assistant", "Reconnect now recovers cleanly."),
  result(),
];

test("SV-07: the fold row's time waits on its right; copy, time and cost wait under the answer", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: settledTurn,
  });

  // The fold row's time is inside the row, at its right, hover-revealed.
  const foldRow = markup.match(
    /data-testid="coding-session-worked-fold-row">([\s\S]*?)<\/button><\/div>/,
  )?.[1];
  assert.ok(foldRow, markup);
  assert.match(
    foldRow,
    /<time class="coding-session-row-time ms-auto[^"]*" data-testid="coding-session-worked-fold-time" dateTime="2026-07-30T12:00:00.000Z"/,
  );

  // The answer, and the line under it, are one hover block.
  const answerBlock = markup.match(
    /data-testid="coding-session-answer-block">([\s\S]*)<\/section>/,
  )?.[1];
  assert.ok(answerBlock, markup);
  assert.match(
    answerBlock,
    /class="group\/answer|coding-session-assistant-message/,
  );
  assert.match(markup, /class="group\/answer flex flex-col"/);
  const meta = answerBlock.match(
    /<span class="coding-session-turn-meta[^"]*" data-testid="coding-session-turn-meta">([\s\S]*?)<\/span><\/div>/,
  )?.[1];
  assert.ok(meta, answerBlock);
  // Copy first, then the time, then the cost — all in the document.
  const copy = meta.indexOf('data-testid="coding-session-turn-copy"');
  const time = meta.indexOf('data-testid="coding-session-turn-time"');
  const cost = meta.indexOf("$0.32 estimate");
  assert.ok(copy >= 0 && copy < time && time < cost, meta);
  assert.match(meta, /aria-label="Copy response"/);
});

test("SV-07: a live turn withholds the fold row, its time and the answer line", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: settledTurn.slice(0, 4),
  });
  assert.doesNotMatch(markup, /coding-session-worked-fold/);
  assert.doesNotMatch(markup, /coding-session-row-time/);
  assert.doesNotMatch(markup, /coding-session-turn-meta/);
  assert.doesNotMatch(markup, /coding-session-answer-block/);
  assert.match(markup, /Working for|Working…/);
});

test("SV-07: an unparseable time is not said, rather than said wrong", () => {
  assert.equal(formatCodingSessionBlockTime("not a time"), null);
  assert.equal(formatCodingSessionBlockTime(null), null);
  const now = Date.parse("2026-07-30T18:00:00.000Z");
  const today = formatCodingSessionBlockTime("2026-07-30T16:00:00.000Z", now);
  assert.ok(today);
  assert.doesNotMatch(today.label, / at /, "today reads as a bare time");
  const yesterday = formatCodingSessionBlockTime(
    "2026-07-29T16:00:00.000Z",
    now,
  );
  assert.match(yesterday?.label ?? "", /^Yesterday at /);
});

test("SV-08: no rule above a turn, a hairline under the fold row, gaps by row kind", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      ...settledTurn,
      message("prompt-2", "user", "And the tests?", "turn-2"),
      message("answer-2", "assistant", "They pass.", "turn-2"),
    ],
  });
  assert.doesNotMatch(markup, /border-t/);
  assert.equal(markup.match(/data-testid="coding-session-turn"/g)?.length, 2);
  assert.match(markup, /border-b border-border\/60 pb-1\.5/);
  // A prompt is followed by generous space; the fold row by a small gap.
  assert.match(markup, /<div class="mt-6"><div class="border-b/);
  assert.match(markup, /<div class="mt-2" data-row-kind="prose">/);
  assert.match(markup, /<div class="flex flex-col gap-6">/);
});

test("SV-08: the gap table is one source for classes and the size estimate", () => {
  const kinds = ["prompt", "prose", "activity", "alert", "fold"];
  for (const previous of [null, ...kinds]) {
    for (const current of kinds) {
      const gap = codingSessionRowGap(previous, current);
      const step = gap.className.match(/^mt-(\d+(?:\.\d+)?)$/)?.[1];
      assert.equal(
        gap.px,
        step === undefined ? 0 : Number(step) * 4,
        `${previous} → ${current}`,
      );
    }
  }
  assert.equal(codingSessionRowGap("activity", "activity").px, 2);
  assert.equal(codingSessionRowGap("prompt", "activity").px, 24);
  assert.equal(codingSessionTurnTailGap(null, "meta").px, 0);

  // A settled, folded turn's estimate grows with what it shows, and a
  // clamped prompt is estimated clamped.
  const model = deriveCodingSessionTranscriptModel(settledTurn, {
    isWorking: false,
  });
  const [row] = buildCodingSessionTranscriptRows(model);
  const short = estimateCodingSessionTranscriptRowSize(row);
  const longModel = deriveCodingSessionTranscriptModel(
    [message("prompt", "user", "x".repeat(5_000)), ...settledTurn.slice(1)],
    { isWorking: false },
  );
  const [longRow] = buildCodingSessionTranscriptRows(longModel);
  const long = estimateCodingSessionTranscriptRowSize(longRow);
  assert.ok(long > short);
  assert.ok(long < short + 300, "a clamped prompt is not estimated in full");
});

test("SV-15: a long prompt clamps with Show full message, and keeps every word", async () => {
  const longText = `${"Line of a long prompt.\n".repeat(12)}The end.`;
  const items = [
    message("prompt", "user", longText),
    message("a", "assistant", "Ok."),
  ];
  const clamped = await renderTranscript({ isWorking: false, items });
  assert.match(clamped, /data-user-message-clamped="true"/);
  assert.match(clamped, /max-h-44 overflow-hidden/);
  assert.match(
    clamped,
    /aria-expanded="false" class="[^"]*" data-testid="coding-session-user-message-toggle" type="button">Show full message</,
  );
  // Clamped is not cut: the last line is still in the document.
  assert.match(clamped, /The end\./);

  // The open state is the transcript's disclosure store, so it survives the
  // virtualizer unmounting the row.
  const opened = await renderTranscript({
    isWorking: false,
    items,
    open: ["prompt:prompt"],
  });
  assert.match(opened, /data-user-message-clamped="false"/);
  assert.match(opened, />Show less</);
  assert.doesNotMatch(opened, /max-h-44/);
});

test("SV-15: the clamp threshold is eight lines or 600 characters", () => {
  assert.equal(shouldClampCodingSessionUserMessage("short"), false);
  assert.equal(shouldClampCodingSessionUserMessage("x".repeat(600)), false);
  assert.equal(shouldClampCodingSessionUserMessage("x".repeat(601)), true);
  assert.equal(
    shouldClampCodingSessionUserMessage(`${"a\n".repeat(7)}a`),
    false,
  );
  assert.equal(
    shouldClampCodingSessionUserMessage(`${"a\n".repeat(8)}a`),
    true,
  );
  assert.equal(shouldClampCodingSessionUserMessage("   \n".repeat(20)), false);
});

test("SV-15: a short prompt has no toggle", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Short"),
      message("a", "assistant", "Ok."),
    ],
  });
  assert.match(markup, /data-user-message-clamped="false"/);
  assert.doesNotMatch(markup, /coding-session-user-message-toggle/);
});

test("SV-05: a thought is a brain and the word Thought, dimmed and closed", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [message("prompt", "user", "Think"), thought("t-1")],
  });
  const row = markup.match(
    /<details[^>]*data-testid="transcript-thought-item"[\s\S]*?<\/details>/,
  )?.[0];
  assert.ok(row, markup);
  assert.doesNotMatch(row, / open=""/);
  assert.match(
    row,
    /lucide-brain mx-1 size-4 shrink-0 text-muted-foreground opacity-70/,
  );
  assert.match(row, /data-testid="transcript-thought-label">Thought<\/span>/);
  // The producer's own word stays reachable; the reasoning is not built.
  assert.match(row, /title="Reasoning · /);
  assert.doesNotMatch(row, /Considering the reconnect path/);
  // SV-01: full width with a soft fill on hover.
  assert.match(row, /<summary class="[^"]*w-full[^"]*hover:bg-accent\/30/);
});

test("SV-05: the live turn's last thought reads Thinking, with the shimmer", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: [message("prompt", "user", "Think"), thought("t-1")],
  });
  const row = markup.match(
    /<details[^>]*data-testid="transcript-thought-item"[\s\S]*?<\/details>/,
  )?.[0];
  assert.ok(row, markup);
  assert.match(row, /data-active=""/);
  assert.match(row, /data-testid="transcript-thought-label">Thinking</);
  assert.match(row, /data-testid="transcript-thought-shimmer"/);
  assert.match(row, /coding-session-live-activity-focus/);
});

test("SV-05: a thought reads Thought once anything follows it", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: [
      message("prompt", "user", "Think"),
      thought("t-1"),
      message("a", "assistant", "Looking at the reconnect path."),
    ],
  });
  const row = markup.match(
    /<details[^>]*data-testid="transcript-thought-item"[\s\S]*?<\/details>/,
  )?.[0];
  assert.ok(row, markup);
  assert.doesNotMatch(row, /data-active/);
  assert.match(row, /data-testid="transcript-thought-label">Thought<\/span>/);
  assert.doesNotMatch(row, /transcript-thought-shimmer/);
});

test("SV-05: an opened thought shows its reasoning", async () => {
  const markup = await renderTranscript({
    isWorking: true,
    items: [message("prompt", "user", "Think"), thought("t-1")],
    open: ["item:t-1"],
  });
  assert.match(markup, /Considering the reconnect path/);
});

function toolSummaryClass(markup) {
  return markup.match(
    /data-testid="transcript-tool-item"[\s\S]*?<summary class="([^"]*)"/,
  )?.[1];
}

test("SV-01/SV-02: a failed step in an opened fold fills on hover and reads quiet, still marked", async () => {
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
  // The fold row still names the failure while it is open.
  assert.match(markup, /data-testid="coding-session-worked-fold-failures"/);
  const summary = toolSummaryClass(markup);
  assert.ok(summary, markup);
  assert.match(summary, /w-full/);
  assert.match(summary, /rounded-md/);
  assert.match(summary, /hover:bg-accent\/30/);
  // Quiet (ref sv02-mockup-quiet-failure): not the loud red row …
  assert.doesNotMatch(summary, /text-destructive/);
  assert.doesNotMatch(markup, /Tool call failed/);
  // … but never unmarked: the folded entry is flagged, and the row carries
  // a "Failed" mark and its suffix.
  assert.match(markup, /data-folded=""/);
  assert.match(markup, /Failed/);
});

test("SV-02: a failure after the answer is kept, and stays loud", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Run it"),
      message("answer", "assistant", "Running the check now."),
      tool("bad", {
        status: "failed",
        isError: true,
        result: "Exit code 1",
      }),
      result("Done. (1200ms)"),
    ],
  });
  const summary = toolSummaryClass(markup);
  assert.ok(summary, markup);
  assert.match(summary, /text-destructive/);
  assert.match(markup, /Tool call failed/);
  assert.doesNotMatch(markup, /data-folded=""[^>]*>[\s\S]*Tool call failed/);
});

test("SV-07: the answer block starts at the agent's answer, not the provider's result body", async () => {
  const markup = await renderTranscript({
    isWorking: false,
    items: [
      message("prompt", "user", "Fix it"),
      tool("tool-1"),
      message("answer", "assistant", "Agent answer text."),
      // A result body that repeats nothing is synthesized as its own row.
      result("Provider closing words. (1200ms)"),
    ],
  });
  const block = markup.match(
    /data-testid="coding-session-answer-block">([\s\S]*)$/,
  )?.[1];
  assert.ok(block, markup);
  assert.match(block, /Agent answer text\./);
});

test("SV-01/SV-06: a subagent row is a full-width activity row with a 16px muted icon", () => {
  const entry = {
    kind: "subagents",
    id: "subagents-1",
    label: "Ran 2 subagents",
    spawns: [
      {
        call: tool("task-1", { result: "Report one" }),
        status: "done",
        children: [],
      },
      {
        call: tool("task-2", { result: "Report two" }),
        status: "failed",
        children: [],
      },
    ],
  };
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSubagentEntry, {
      disclosureId: "subagents:subagents-1",
      entry,
      onOpenChange: () => {},
      open: false,
      renderChild: () => null,
    }),
  );
  const summary = markup.match(/<summary[^>]*class="([^"]*)"/)?.[1];
  assert.ok(summary, markup);
  assert.match(summary, /w-full/);
  assert.match(summary, /hover:bg-accent\/30/);
  assert.match(
    markup,
    /lucide-bot mx-1 size-4 shrink-0 text-muted-foreground opacity-70/,
  );
  assert.match(
    markup,
    /<span class="min-w-0 truncate text-muted-foreground">Ran 2 subagents<\/span>/,
  );
  // The group's failure is still on the row, in its own colour.
  assert.match(
    markup,
    /aria-label="Failed"[^>]*text-destructive|text-destructive[^>]*aria-label="Failed"/,
  );
});

test("SV-07 with A2: a fold summary naming a failed step is shown, muted, and never truncated away", () => {
  const render = (fold) =>
    renderToStaticMarkup(
      React.createElement(CodingSessionWorkedFold, {
        fold: {
          anchorIndex: 1,
          hiddenIndexes: [1, 2],
          durationMs: 4_000,
          failedCount: 1,
          unfinishedCount: 0,
          ...fold,
        },
        onToggle: () => {},
        open: false,
        startedAt: timestamp,
      }),
    );
  const markup = render({
    summary:
      "Edited 4 files, ran 12 commands, read 6 files and searched 3 times · 1 step failed",
    workSummary:
      "Edited 4 files, ran 12 commands, read 6 files and searched 3 times",
    failureSummary: "1 step failed",
  });
  assert.match(markup, /Worked for 4(\.0)?s/);
  // Only the work summary may ellipsize.
  assert.match(
    markup,
    /<span class="min-w-0 truncate text-muted-foreground\/70" data-testid="coding-session-worked-fold-summary">· Edited 4 files, ran 12 commands, read 6 files and searched 3 times<\/span>/,
  );
  // The failure clause is its own span that cannot shrink or truncate, so a
  // narrow row clips the work summary first and the failure stays whole.
  const failures = markup.match(
    /<span class="([^"]*)" data-testid="coding-session-worked-fold-failures">(.*?)<\/span>/,
  );
  assert.ok(failures, markup);
  assert.match(failures[1], /\bshrink-0\b/);
  assert.match(failures[1], /whitespace-nowrap/);
  assert.doesNotMatch(failures[1], /truncate|min-w-0/);
  assert.match(failures[2], /1 step failed$/);
  assert.match(failures[2], /text-destructive\/60/);
  assert.doesNotMatch(
    markup.match(/data-testid="coding-session-worked-fold-summary">[^<]*/)[0],
    /failed/,
  );
  // The summary is never behind hover; only the time is.
  assert.doesNotMatch(markup, /coding-session-row-time[^>]*>[^<]*step failed/);

  // No work summary: only the failure clause.
  const onlyFailure = render({
    summary: "1 step failed",
    workSummary: "",
    failureSummary: "1 step failed",
  });
  assert.doesNotMatch(onlyFailure, /coding-session-worked-fold-summary/);
  assert.match(onlyFailure, /coding-session-worked-fold-failures/);

  // No failure: no failure span.
  const clean = render({
    summary: "Ran 2 commands",
    workSummary: "Ran 2 commands",
    failureSummary: "",
    failedCount: 0,
  });
  assert.doesNotMatch(clean, /coding-session-worked-fold-failures/);
});
