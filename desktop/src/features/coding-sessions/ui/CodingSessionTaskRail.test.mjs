import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHeader } from "./CodingSessionHeader.tsx";
import {
  CODING_SESSION_TASK_RAIL_ID,
  CodingSessionTaskRail,
  codingSessionTaskDockSummary,
} from "./CodingSessionTaskRail.tsx";

const activeModel = {
  sourceItemId: "plan-1",
  turnId: "turn-1",
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

test("desktop tasks dock above the composer as one collapsed line", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionTaskRail, {
      model: activeModel,
      onClose() {},
      variant: "dock",
    }),
  );

  assert.match(markup, /data-testid="coding-session-task-dock"/);
  assert.match(markup, /data-variant="dock"/);
  // SESSION_VIEW_UX_PLAN L4: mounted collapsed — no auto-open over the
  // transcript — yet the line still says how far along and what is in hand.
  assert.match(markup, /data-expanded="false"/);
  assert.match(markup, /aria-expanded="false"/);
  assert.match(markup, /aria-label="Expand session tasks"/);
  assert.match(markup, />Tasks</);
  assert.match(markup, /1\/3/);
  assert.match(
    markup,
    /data-testid="coding-session-task-dock-summary"[^>]*>Blocked: Await a provider state</,
  );
  assert.doesNotMatch(markup, /<ol/);
  assert.doesNotMatch(markup, />now</);
  assert.match(markup, /aria-label="Close session tasks"/);
  assert.doesNotMatch(markup, /w-80 border-l/);
  assert.doesNotMatch(markup, /role="progressbar"/);
});

test("the collapsed line states the plan honestly in every case", () => {
  const line = (model, loadState = "ready") =>
    codingSessionTaskDockSummary({ loadState, model });
  assert.equal(line(null, "loading"), "Loading plan…");
  assert.equal(line(null, "error"), "Plan unavailable");
  assert.equal(line(null), "No tasks yet");
  assert.equal(line({ ...activeModel, tasks: [] }), "No tasks");
  // Attention leads, even over the task in hand: activeModel has one
  // in-progress task and one blocked one.
  assert.equal(line(activeModel), "Blocked: Await a provider state");
  assert.equal(
    line({
      ...activeModel,
      tasks: activeModel.tasks.filter((task) => task.status !== "blocked"),
    }),
    "Build the Buzz rail",
  );
  // A failed task leads the line, ahead of blocked and in-progress ones.
  assert.equal(
    line({
      ...activeModel,
      tasks: [
        ...activeModel.tasks,
        { id: "task-failed", text: "Run the gate", status: "failed" },
      ],
    }),
    "Failed: Run the gate",
  );
  assert.equal(
    line({
      ...activeModel,
      tasks: [{ id: "p", text: "Write tests", status: "pending" }],
      completedCount: 0,
    }),
    "Next: Write tests",
  );
  assert.equal(
    line({
      ...activeModel,
      tasks: [{ id: "c", text: "Done", status: "completed" }],
      completedCount: 1,
    }),
    "All complete",
  );
});

test("header offers Plan only while the plan is not already on screen", () => {
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

  // The docked rail (or the sheet) is showing: a second Plan control would be
  // the plan on screen twice.
  assert.doesNotMatch(openMarkup, /coding-session-task-rail-toggle/);
  assert.match(
    closedMarkup,
    new RegExp(`aria-controls="${CODING_SESSION_TASK_RAIL_ID}"`),
  );
  assert.match(closedMarkup, /aria-expanded="false"/);
  assert.match(closedMarkup, /aria-label="Show session plan"/);
  assert.match(closedMarkup, />Plan</);
  assert.match(closedMarkup, /3 tasks/);
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
  assert.match(markup, /aria-label="Show session details"/);
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
  assert.match(markup, /aria-label="Show session details"/);
  // Pop out is a `⋯` item now; the menu's trigger is labelled.
  assert.match(markup, /aria-label="Session actions"/);
  assert.doesNotMatch(markup, />Pop out</);
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
