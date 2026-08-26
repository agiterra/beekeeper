import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionActiveWorkDock } from "./CodingSessionActiveWorkDock.tsx";

test("active work shows every working agent but only signed active tasks", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionActiveWorkDock, {
      agents: [
        {
          executionKey: "codex",
          label: "Codex",
          turnKey: "codex:turn-1",
          model: {
            sourceItemId: "plan-1",
            turnId: "turn-1",
            timestamp: "2026-08-25T12:00:00Z",
            completedCount: 1,
            explanation: null,
            copyText: null,
            state: "active",
            tasks: [
              { id: "1", text: "Inspect the composer", status: "completed" },
              { id: "2", text: "Build the focus mode", status: "in_progress" },
            ],
          },
        },
        {
          executionKey: "claude",
          label: "Claude",
          turnKey: "claude:turn-2",
          model: null,
        },
      ],
      focusedExecutionKey: "codex",
      onFocusAgent() {},
    }),
  );
  assert.match(markup, /Active work/);
  assert.match(markup, /Codex/);
  assert.match(markup, /Claude/);
  assert.match(markup, /Build the focus mode/);
  assert.doesNotMatch(markup, /completed plan/i);
});

test("active work disappears when there are no working executions", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionActiveWorkDock, {
        agents: [],
        focusedExecutionKey: null,
        onFocusAgent() {},
      }),
    ),
    "",
  );
});
