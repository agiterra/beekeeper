import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionActiveTool } from "./CodingSessionTranscriptParts.tsx";
import {
  CodingSessionLastTranscriptEventContext,
  CodingSessionWorking,
} from "./CodingSessionTranscriptWorking.tsx";
import { CodingSessionWaitingStripView } from "./CodingSessionWaitingStrip.tsx";

// SV-104: live action text (the running tool's label, "Thinking", the
// working line) shimmers only while live and fresh; a quiet provider, a
// settled turn or a queued call is plain text. SV-99: the strip's markup.

function withLastEvent(lastEventAt, node) {
  return React.createElement(
    CodingSessionLastTranscriptEventContext.Provider,
    { value: lastEventAt },
    node,
  );
}

function runningTool(overrides = {}) {
  return {
    id: "tool-1",
    type: "tool",
    renderClass: "shell",
    descriptor: {
      renderClass: "shell",
      label: "Ran command",
      preview: "rm -rf target/tmp",
      action: { verb: "Ran", object: "rm -rf target/tmp" },
    },
    title: "Bash",
    toolName: "Bash",
    beekeeperToolName: null,
    status: "executing",
    args: {},
    result: "",
    isError: false,
    timestamp: new Date().toISOString(),
    startedAt: new Date().toISOString(),
    turnId: "turn-1",
    ...overrides,
  };
}

function renderTool(lastEventAt, settlement = "live", overrides = {}) {
  return renderToStaticMarkup(
    withLastEvent(
      lastEventAt,
      React.createElement(CodingSessionActiveTool, {
        disclosureId: "item:tool-1",
        item: runningTool(overrides),
        onOpenChange: () => {},
        open: false,
        settlement,
      }),
    ),
  );
}

test("a running tool in a live turn with a fresh provider shimmers its label", () => {
  const html = renderTool(Date.now());
  assert.match(html, /data-live-shimmer="on"/);
  assert.match(html, /coding-session-live-shimmer-overlay/);
  assert.match(html, /Run rm -rf target\/tmp/);
});

test("a quiet provider, a settled turn, a queued call or an unknown time stop the shimmer", () => {
  assert.doesNotMatch(
    renderTool(Date.now() - 5 * 60_000),
    /coding-session-live-shimmer-overlay/,
  );
  assert.doesNotMatch(
    renderTool(Date.now(), "settled"),
    /coding-session-live-shimmer-overlay/,
  );
  assert.doesNotMatch(
    renderTool(Date.now(), "unknown"),
    /coding-session-live-shimmer-overlay/,
  );
  assert.doesNotMatch(
    renderTool(Date.now(), "live", { status: "pending" }),
    /coding-session-live-shimmer-overlay/,
  );
  assert.doesNotMatch(renderTool(null), /coding-session-live-shimmer-overlay/);
});

test("Thinking shimmers while fresh, and the working label stays still beside it", () => {
  const html = renderToStaticMarkup(
    withLastEvent(
      Date.now(),
      React.createElement(CodingSessionWorking, {
        showThinking: true,
        startedAt: new Date(Date.now() - 30_000).toISOString(),
      }),
    ),
  );
  const thinking = html.slice(html.indexOf("coding-session-thinking"));
  assert.match(thinking, /data-live-shimmer="on"/);
  // Exactly one shimmer on the line: Thinking's, not the timer's too.
  assert.equal(html.match(/coding-session-live-shimmer-overlay/g)?.length, 1);
});

test("the working label shimmers when Thinking is not shown, and stops when quiet", () => {
  const fresh = renderToStaticMarkup(
    withLastEvent(
      Date.now(),
      React.createElement(CodingSessionWorking, {
        startedAt: new Date(Date.now() - 30_000).toISOString(),
      }),
    ),
  );
  assert.match(fresh, /data-live-shimmer="on"/);
  assert.match(fresh, /Working for/);
  const quiet = renderToStaticMarkup(
    withLastEvent(
      Date.now() - 3 * 60_000,
      React.createElement(CodingSessionWorking, {
        showThinking: true,
        startedAt: new Date(Date.now() - 5 * 60_000).toISOString(),
      }),
    ),
  );
  assert.doesNotMatch(quiet, /coding-session-live-shimmer-overlay/);
  assert.match(quiet, /no update for 3m/);
});

function waiting(overrides = {}) {
  return {
    lines: [
      {
        kind: "subagent",
        key: "subagent:exec-1:s1",
        executionKey: "exec-1",
        executionLabel: "Lead",
        brief: "D13 fast batch: owned lanes",
        itemId: "s1",
      },
      {
        kind: "subagent",
        key: "subagent:exec-1:s2",
        executionKey: "exec-1",
        executionLabel: "Lead",
        brief: "Second lane",
        itemId: "s2",
      },
    ],
    subagents: 2,
    backgroundTasks: 0,
    gates: 0,
    headline: "Waiting on 2 subagents",
    brief: "D13 fast batch: owned lanes",
    quietMs: null,
    fresh: true,
    ...overrides,
  };
}

test("the strip names what it waits on and pulses only when told", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionWaitingStripView, {
      pulse: true,
      waiting: waiting(),
    }),
  );
  assert.match(html, /Waiting on 2 subagents/);
  assert.match(html, /D13 fast batch: owned lanes/);
  assert.match(html, /data-pulse="on"/);
  assert.match(html, /animate-pulse/);
  assert.match(html, /What this session is waiting on/);
  assert.doesNotMatch(html, />Stop</);
});

test("a quiet strip holds still and says so", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionWaitingStripView, {
      pulse: false,
      waiting: waiting({ fresh: false, quietMs: 4 * 60_000 }),
    }),
  );
  assert.match(html, /data-pulse="off"/);
  assert.doesNotMatch(html, /animate-pulse/);
  assert.match(html, /no update for 4m/);
});
