import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  codingSessionAgentAccent,
  CodingSessionAgentFocus,
} from "./CodingSessionAgentFocus.tsx";

test("agent accents are stable and the compact focus control exposes aggregate liveness", () => {
  assert.deepEqual(
    codingSessionAgentAccent("codex-primary"),
    codingSessionAgentAccent("codex-primary"),
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionAgentFocus, {
      focusedExecutionKey: "codex-primary",
      items: [
        {
          executionKey: "codex-primary",
          label: "Codex · GPT-5.6",
          status: { kind: "working", label: "Working" },
        },
        {
          executionKey: "claude-review",
          label: "Claude · Sonnet",
          status: { kind: "idle", label: "Idle" },
        },
      ],
      onFocus() {},
    }),
  );
  assert.match(markup, /2 agents · 1 working/);
  assert.match(markup, /Viewing Codex · GPT-5.6/);
  assert.match(markup, /coding-session-agent-breathe/);
  assert.match(markup, /data-working="true"/);
  assert.doesNotMatch(markup, /Claude · Sonnet/);
});
