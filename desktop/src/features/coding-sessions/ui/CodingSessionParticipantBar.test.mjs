import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { codingSessionParticipantAccent } from "../lib/codingSessionParticipantAccent.ts";
import { CodingSessionParticipantBar } from "./CodingSessionParticipantBar.tsx";

test("the Mission roster gives participant identity and status real weight", () => {
  const key = "f0".repeat(32);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: "builder",
      items: [
        {
          executionKey: "lead",
          label: "Helios · Lead",
          secondaryLabel: "Codex · gpt-5.6-sol",
          role: "lead",
          status: { kind: "idle", label: "Idle" },
          disposition: "idle",
          activity: null,
          lastTurnLabel: "last turn 4m ago",
        },
        {
          executionKey: "builder",
          label: "Bob · Builder",
          secondaryLabel: "Claude Code · sonnet",
          role: "builder",
          status: { kind: "working", label: "Working" },
          disposition: "live",
          activity: "implementing the stream foundation",
          lastTurnLabel: "last turn just now",
        },
      ],
      onFocus() {},
    }),
  );
  assert.match(markup, /aria-label="Session participants"/);
  assert.match(markup, /Helios · Lead/);
  assert.match(markup, /Bob · Builder/);
  assert.match(markup, /implementing the stream foundation/);
  assert.match(markup, /aria-pressed="true"/);
  assert.match(markup, /coding-session-agent-breathe/);
  assert.doesNotMatch(markup, new RegExp(key));
});

test("an empty Mission roster adds no chrome to Conversation", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionParticipantBar, {
        focusedExecutionKey: null,
        items: [],
        onFocus() {},
      }),
    ),
    "",
  );
});

test("identity accents never consume attention colors", () => {
  const forbidden = /amber|yellow|red|destructive/;
  for (const executionKey of ["lead", "seat-1", "verifier", "builder"]) {
    const accent = codingSessionParticipantAccent(executionKey);
    assert.doesNotMatch(Object.values(accent).join(" "), forbidden);
  }

  const waiting = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "verifier",
          label: "Parallax · Verifier",
          secondaryLabel: "Codex · gpt-5.6-sol",
          role: "verifier",
          status: { kind: "waiting", label: "Waiting" },
          disposition: "waiting for you",
          activity: null,
          lastTurnLabel: "last turn just now",
        },
      ],
      onFocus() {},
    }),
  );
  // Amber remains a state word: waiting is actionable, not an identity hue.
  assert.match(waiting, /bg-amber-500/);
});

test("only No provider answering consumes destructive styling", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "failed",
          label: "Bob · Builder",
          secondaryLabel: null,
          role: "builder",
          status: {
            kind: "unknown",
            label: "Needs attention",
            attention: "failed",
          },
          disposition: "needs attention",
          activity: null,
          lastTurnLabel: "last turn 1m ago",
        },
        {
          executionKey: "unreachable",
          label: "Parallax · Verifier",
          secondaryLabel: null,
          role: "verifier",
          status: {
            kind: "unknown",
            label: "No provider answering",
            attention: "unreachable",
          },
          disposition: "no provider answering",
          activity: null,
          lastTurnLabel: "last turn 2m ago",
        },
      ],
      onFocus() {},
    }),
  );
  const chips = markup.match(/<button[\s\S]*?<\/button>/g) ?? [];
  assert.equal(chips.length, 2);
  assert.doesNotMatch(chips[0], /destructive/);
  assert.match(chips[0], /amber/);
  assert.match(chips[1], /destructive/);
});
