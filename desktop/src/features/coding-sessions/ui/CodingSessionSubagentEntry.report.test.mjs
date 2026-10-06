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

import { CodingSessionSubagentSpawnDetail } from "./CodingSessionSubagentEntry.tsx";

// SV-97: a subagent's last answer is published twice — as prose attributed
// to its call, and as the call's result — and the expanded detail showed both.

function call(overrides = {}) {
  return {
    id: "item-task-1",
    type: "tool",
    renderClass: "subagent",
    title: "Task",
    toolName: "Task",
    buzzToolName: null,
    status: "completed",
    args: { description: "Review the parser", subagent_type: "Explore" },
    result: "Found two bugs",
    isError: false,
    toolCallId: "toolu_01",
    timestamp: "2026-10-05T12:00:00.000Z",
    startedAt: "2026-10-05T12:00:00.000Z",
    completedAt: "2026-10-05T12:01:02.000Z",
    ...overrides,
  };
}

function message(id, text) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "",
    text,
    timestamp: "2026-10-05T12:00:30.000Z",
    parentToolId: "toolu_01",
  };
}

/** Static markup inside a router: the report box renders Markdown. */
async function renderDetail(spawn) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionSubagentSpawnDetail, {
        renderChild: (item) =>
          React.createElement(
            "p",
            { "data-testid": "child" },
            item.type === "message" ? item.text : item.id,
          ),
        showHeader: false,
        spawn,
      }),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

const childTexts = (html) =>
  [...html.matchAll(/<p data-testid="child">([^<]*)<\/p>/g)].map((m) => m[1]);

test("SV-97: the subagent's final answer shows once, as the report", async () => {
  const answer = "Found two bugs\nmore detail";
  const html = await renderDetail({
    call: call({ result: answer }),
    children: [
      message("m-1", "Reading the parser"),
      // Published as prose, then again as the call's result.
      message("m-2", `  ${answer}\n`),
    ],
    status: "done",
  });
  assert.deepEqual(childTexts(html), ["Reading the parser"]);
  assert.match(html, /data-testid="coding-session-subagent-report"/);
  assert.match(html, /Found two bugs/);
});

test("SV-97: prose that differs from the report, or no report yet, stays", async () => {
  for (const [status, result] of [
    ["done", "Found two bugs, and fixed one"],
    // A running spawn shows no report, so nothing is a repeat of it.
    ["running", "Found two bugs"],
  ]) {
    const html = await renderDetail({
      call: call({ result }),
      children: [message("m-1", "Found two bugs")],
      status,
    });
    assert.deepEqual(childTexts(html), ["Found two bugs"], status);
  }
});
