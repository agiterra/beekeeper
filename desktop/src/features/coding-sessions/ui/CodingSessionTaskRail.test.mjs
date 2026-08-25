import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHeader } from "./CodingSessionHeader.tsx";
import {
  codingSessionTaskRailPreferenceKey,
  CODING_SESSION_TASK_RAIL_ID,
  CodingSessionTaskRail,
  deriveCodingSessionTaskRailOpen,
} from "./CodingSessionTaskRail.tsx";

const activeModel = {
  sourceItemId: "plan-1",
  timestamp: "2026-07-30T12:00:00.000Z",
  completedCount: 1,
  explanation: "Rendering the latest signed plan from this session.",
  copyText:
    "Rendering the latest signed plan from this session.\n\n- [x] Read the signed transcript\n- [ ] Build the Buzz rail (in progress)",
  state: "active",
  tasks: [
    {
      id: "task-complete",
      text: "Read the signed transcript",
      status: "completed",
    },
    {
      id: "task-current",
      text: "Build the Buzz rail",
      status: "in_progress",
    },
    {
      id: "task-unknown",
      text: "Await a provider state",
      status: "blocked",
    },
  ],
};

test("empty plan rail stays closed until a plan exists or the exact session is opened", () => {
  assert.equal(
    deriveCodingSessionTaskRailOpen({
      hasPlan: false,
      isNarrow: false,
      preference: null,
    }),
    false,
  );
  assert.equal(
    deriveCodingSessionTaskRailOpen({
      hasPlan: true,
      isNarrow: false,
      preference: null,
    }),
    true,
  );
  assert.equal(
    deriveCodingSessionTaskRailOpen({
      hasPlan: false,
      isNarrow: false,
      preference: "open",
    }),
    true,
  );
  assert.equal(
    deriveCodingSessionTaskRailOpen({
      hasPlan: true,
      isNarrow: false,
      preference: "closed",
    }),
    false,
  );
  assert.equal(
    deriveCodingSessionTaskRailOpen({
      hasPlan: true,
      isNarrow: true,
      preference: null,
    }),
    false,
  );

  assert.notEqual(
    codingSessionTaskRailPreferenceKey("channel-1", "generation-1"),
    codingSessionTaskRailPreferenceKey("channel-1", "generation-2"),
  );
  assert.notEqual(
    codingSessionTaskRailPreferenceKey("channel-1", "generation-1"),
    codingSessionTaskRailPreferenceKey("channel-2", "generation-1"),
  );
});

test("task rail renders bounded provider-neutral states and completion summary", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, { model: activeModel }),
  );

  assert.match(markup, /aria-label="Session plan"/);
  assert.match(markup, /id="coding-session-task-rail"/);
  assert.match(markup, />Plan</);
  assert.match(markup, /1\/3/);
  assert.match(markup, /Read the signed transcript/);
  assert.match(markup, /Build the Buzz rail/);
  assert.match(markup, /Await a provider state/);
  assert.match(markup, /data-status="completed"/);
  assert.match(markup, /data-status="in_progress"/);
  assert.match(markup, /data-status="blocked"/);
  assert.match(markup, /Rendering the latest signed plan/);
  assert.match(markup, /role="progressbar"/);
  assert.match(markup, /aria-valuenow="33"/);
  assert.match(markup, /aria-label="Copy session plan"/);
  assert.match(markup, /aria-label="Copy task: Build the Buzz rail"/);
  assert.match(markup, /<details/);
  assert.match(markup, />Completed</);
  assert.match(markup, /Live from signed session/);
  assert.doesNotMatch(markup, /Codex|Claude|TodoWrite|update_plan/);
});

test("task rail distinguishes no signed plan from an explicit empty plan", () => {
  const noPlanMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, { model: null }),
  );
  const emptyPlanMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      model: {
        sourceItemId: "empty-plan",
        timestamp: "2026-07-30T12:00:00.000Z",
        completedCount: 0,
        explanation: null,
        copyText: null,
        state: "empty",
        tasks: [],
      },
    }),
  );

  assert.match(noPlanMarkup, /No plan published/);
  assert.match(noPlanMarkup, /signed session/);
  assert.match(emptyPlanMarkup, /Plan is empty/);
  assert.match(emptyPlanMarkup, /intentionally contains no tasks/);
});

test("task rail distinguishes loading and error states without synthetic tasks", () => {
  const loadingMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      loadState: "loading",
      model: null,
    }),
  );
  const errorMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      loadState: "error",
      model: null,
    }),
  );

  assert.match(loadingMarkup, /aria-label="Loading session plan"/);
  assert.match(loadingMarkup, /Loading session plan/);
  assert.doesNotMatch(loadingMarkup, /Implement|Build/);
  assert.match(errorMarkup, /Plan unavailable/);
  assert.match(errorMarkup, /signed plan/);
  assert.doesNotMatch(errorMarkup, /Implement|Build/);
});

test("task rail fills a focus-managed sheet without retaining desktop width", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      model: activeModel,
      variant: "sheet",
    }),
  );

  assert.match(markup, /data-variant="sheet"/);
  assert.match(markup, /h-full w-full/);
  assert.doesNotMatch(markup, /w-80 border-l/);
});

test("desktop tasks dock cleanly above the composer instead of taking a side rail", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      model: activeModel,
      onClose() {},
      variant: "dock",
    }),
  );

  assert.match(markup, /data-testid="coding-session-task-dock"/);
  assert.match(markup, /data-variant="dock"/);
  assert.match(markup, />Tasks</);
  assert.match(markup, /1\/3/);
  assert.match(markup, /Build the Buzz rail/);
  assert.match(markup, />now</);
  assert.match(markup, /aria-label="Close session tasks"/);
  assert.doesNotMatch(markup, /w-80 border-l/);
  assert.doesNotMatch(markup, /role="progressbar"/);
});

test("header exposes an accessible Plan toggle tied to the exact rail", () => {
  const openMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Generation 2",
      onBack() {},
      onToggleTaskRail() {},
      status: { kind: "working", label: "Working" },
      taskCount: 3,
      taskRailOpen: true,
    }),
  );
  const closedMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "Generation 2",
      onBack() {},
      onToggleTaskRail() {},
      status: { kind: "idle", label: "Idle" },
      taskCount: 3,
      taskRailOpen: false,
    }),
  );

  assert.match(
    openMarkup,
    new RegExp(`aria-controls="${CODING_SESSION_TASK_RAIL_ID}"`),
  );
  assert.match(openMarkup, /aria-expanded="true"/);
  assert.match(openMarkup, /aria-label="Hide session plan"/);
  assert.match(openMarkup, />Plan</);
  assert.match(openMarkup, /3 tasks/);
  assert.match(closedMarkup, /aria-expanded="false"/);
  assert.match(closedMarkup, /aria-label="Show session plan"/);
});

test("header keeps signed generation provenance subordinate to session orientation", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      generationLabel: "caf82e4c-generation-2",
      model: "claude-sonnet-4-6",
      onBack() {},
      projectName: "amas-redux",
      repoName: "buzz",
      runtimeLabel: "Claude Code",
      sessionTitle: "Advance Buzz live sessions",
      status: { kind: "working", label: "Working" },
    }),
  );

  assert.match(markup, />Advance Buzz live sessions</);
  assert.match(
    markup,
    />amas-redux · buzz · Claude Code · claude-sonnet-4-6 · caf82e4c-generation-2</,
  );
  assert.match(markup, /aria-label="Show session provenance"/);
  assert.doesNotMatch(markup, /<h1[^>]*>caf82e4c-generation-2<\/h1>/);
});

test("compact header keeps icon-only controls keyboard labeled", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: "Hive Sessions",
      compact: true,
      generationLabel: "Generation 2",
      onBack() {},
      onPopout() {},
      onToggleTaskRail() {},
      sessionTitle: "Responsive session",
      status: { kind: "idle", label: "Idle" },
      taskRailOpen: false,
    }),
  );

  assert.match(markup, /data-compact="true"/);
  assert.match(markup, /aria-label="Session status: Idle"/);
  assert.match(markup, /aria-label="Show session plan"/);
  assert.match(markup, /aria-label="Show session provenance"/);
  assert.match(markup, />Pop out</);
});

test("header omits the Plan control in non-ready workspace states", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeader, {
      channelName: null,
      generationLabel: "missing-generation",
      onBack() {},
      status: { kind: "unknown", label: "Status unknown" },
    }),
  );

  assert.doesNotMatch(markup, /coding-session-task-rail-toggle/);
});
