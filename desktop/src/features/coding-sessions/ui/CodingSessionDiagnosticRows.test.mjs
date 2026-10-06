import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { describeCodingSessionStatusText } from "../lib/codingSessionTranscriptModelBackground.ts";
import { CodingSessionDiagnosticRows } from "./CodingSessionTranscriptParts.tsx";

// SV-93: "Turn details" says an autonomous turn in the words of the turn's
// own "Woke on its own · background task" marker, not the provider's codes.

const STARTED =
  "autonomous_turn_started: the agent began a turn nobody prompted";
const WOKE = "autonomous_turn: the agent woke on task-notification";

test("known autonomous-turn codes read as the marker does", () => {
  assert.equal(
    describeCodingSessionStatusText(STARTED),
    "Woke on its own · began a turn nobody prompted",
  );
  assert.equal(
    describeCodingSessionStatusText(WOKE),
    "Woke on its own · background task",
  );
  assert.equal(
    describeCodingSessionStatusText("autonomous_turn: the agent woke"),
    "Woke on its own",
  );
});

test("an unknown code stays verbatim", () => {
  for (const text of [
    "compaction_started: context is being compacted",
    "autonomous_turn_paused: something new",
    "plain words",
  ]) {
    assert.equal(describeCodingSessionStatusText(text), text);
  }
});

function status(id, text, title = "Status") {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title,
    text,
    timestamp: "2026-10-06T10:00:00.000Z",
    turnId: "turn-1",
  };
}

test("Turn details rows show the wording, with the raw code one hover away", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionDiagnosticRows, {
      diagnostics: [
        status("s1", STARTED),
        status("s2", WOKE),
        status("s3", "mystery_code: kept as sent"),
        status("s4", "autonomous_turn_started: not a status row", "Other"),
      ],
    }),
  );
  assert.match(html, />Woke on its own · began a turn nobody prompted</);
  assert.match(html, />Woke on its own · background task</);
  assert.match(html, new RegExp(`title="${STARTED}"`));
  assert.match(html, />mystery_code: kept as sent</);
  // Only Status rows are reworded.
  assert.match(html, />autonomous_turn_started: not a status row</);
  assert.doesNotMatch(html, />autonomous_turn: the agent woke/);
});
