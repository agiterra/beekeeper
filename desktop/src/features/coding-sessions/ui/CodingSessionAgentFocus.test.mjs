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

test("identity accents never wear an attention hue", () => {
  // A seat that hashes to amber wore the same colour the state vocabulary
  // uses for caution, so a perfectly healthy seat read as one needing
  // attention — and the Mission card grammar turned that thin rule into a
  // whole bordered block. `PARTICIPANT_ACCENTS` already excludes attention
  // hues and says why; this palette now matches it.
  const seen = new Set();
  for (let index = 0; index < 4096; index += 1) {
    const accent = codingSessionAgentAccent(`execution-${index}`);
    for (const value of Object.values(accent)) seen.add(value);
  }
  for (const value of seen) {
    assert.doesNotMatch(value, /amber|destructive|\bred-/);
  }
  // Not vacuous: the palette still distinguishes seats, and it stays the same
  // width as `PARTICIPANT_ACCENTS` so a seat's turn block and its chip cannot
  // collide differently.
  assert.ok(seen.size >= 8);
  assert.equal(
    new Set(
      Array.from(
        { length: 4096 },
        (_, index) => codingSessionAgentAccent(`execution-${index}`).dot,
      ),
    ).size,
    4,
  );
});
