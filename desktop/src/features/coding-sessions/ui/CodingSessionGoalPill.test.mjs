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

function renderInspectorVariant(goal, currentUserPubkey) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionGoalPill, {
      channelId: "channel-1",
      currentUserPubkey,
      founderPubkey: FOUNDER,
      goal,
      sessionRef: "session-1",
      variant: "inspector",
    }),
  );
}

test("U-T6: the inspector variant is the edit control and nothing else", () => {
  const withGoal = renderInspectorVariant(
    { content: "Ship the portable team loop" },
    FOUNDER,
  );
  assert.match(withGoal, /coding-session-goal-edit-inspector/);
  // Finding 23: the control says which of the two things it does, so a goal
  // that is already published never sits under a button reading `Set goal`.
  assert.match(withGoal, />Change goal</);
  assert.doesNotMatch(withGoal, />Set goal</);
  assert.doesNotMatch(withGoal, /Goal:/);
  assert.doesNotMatch(withGoal, /Ship the portable team loop/);
  assert.doesNotMatch(withGoal, /coding-session-goal-workspace/);

  const withoutGoal = renderInspectorVariant(null, FOUNDER);
  assert.match(withoutGoal, /coding-session-goal-edit-inspector/);
  assert.match(withoutGoal, />Set goal</);
});

test("U-T6: a viewer who cannot publish the goal gets no control at all", () => {
  assert.equal(
    renderInspectorVariant(
      { content: "Ship the portable team loop" },
      "b".repeat(64),
    ),
    "",
  );
  assert.equal(renderInspectorVariant(null, "b".repeat(64)), "");
});

test("the workspace goal is one line by default; a multi-line goal offers a chevron", () => {
  const oneLine = render({ content: "Fix the reconnect bug." });
  assert.match(oneLine, /line-clamp-1/);
  assert.match(oneLine, /data-goal-expanded="false"/);
  assert.doesNotMatch(oneLine, /whitespace-pre-wrap/);
  // A static render cannot measure overflow, so only a goal with a line
  // break is known to need the chevron before the browser measures.
  assert.doesNotMatch(oneLine, /coding-session-goal-expand/);
  const multi = render({ content: "Line one.\nLine two, the detail." });
  assert.match(multi, /line-clamp-1/);
  assert.match(multi, /coding-session-goal-expand/);
  assert.match(multi, /aria-label="Show the whole goal"/);
});
