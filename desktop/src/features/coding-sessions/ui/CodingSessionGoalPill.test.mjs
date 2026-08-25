import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionGoalPill } from "./CodingSessionGoalPill.tsx";

const FOUNDER = "a".repeat(64);

function render(goal) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionGoalPill, {
      channelId: "channel-1",
      currentUserPubkey: FOUNDER,
      founderPubkey: FOUNDER,
      goal,
      sessionRef: "session-1",
      workspaceExpanded: true,
    }),
  );
}

test("an empty goal is a small action, not the largest object in the session", () => {
  const markup = render(null);
  assert.match(markup, /coding-session-goal-add-workspace/);
  assert.match(markup, />Add goal</);
  assert.doesNotMatch(markup, /Add a goal for this session/);
  assert.doesNotMatch(markup, /border-primary\/20/);
});

test("a real goal keeps the shared objective visible", () => {
  const markup = render({
    content: "Make the full-screen session feel like one narrative",
  });
  assert.match(markup, /Goal:/);
  assert.match(markup, /Make the full-screen session feel like one narrative/);
  assert.match(markup, /coding-session-goal-edit-workspace/);
});
