import assert from "node:assert/strict";
import { test } from "node:test";

import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  SessionNameOriginMarker,
  sessionNameOriginAccessibleLabel,
  sessionNameOriginSentence,
} from "@/features/coding-sessions/ui/SessionNameOriginMarker";
import { AgentProgressLaneRow } from "@/features/agent-progress/ui/AgentProgressLaneRow";

const PROVIDER = "d4".repeat(32);
const GENERATED = {
  origin: "generated",
  model: "claude-haiku-4-5",
  signerPubkey: PROVIDER,
};
const PERSON = { origin: "person", model: null, signerPubkey: null };

function marker(origin) {
  return renderToStaticMarkup(
    React.createElement(SessionNameOriginMarker, {
      origin,
      testId: "row-title-origin",
    }),
  );
}

test("a generated title is marked Auto-named; a person's name and an unknown origin are not", () => {
  assert.match(marker(GENERATED), /data-testid="row-title-origin"/);
  assert.match(marker(GENERATED), /Auto-named/);
  assert.equal(marker(PERSON), "");
  assert.equal(marker(null), "");
  assert.equal(marker(undefined), "");
});

test("the tooltip names the provider by profile and key, and the model", () => {
  assert.equal(
    sessionNameOriginSentence(GENERATED, "Brian's Mac"),
    "Named automatically from the first message by Brian's Mac (d4d4d4d4…d4d4) · claude-haiku-4-5",
  );
  // No profile yet: the key alone, never a guessed name.
  assert.equal(
    sessionNameOriginSentence({ ...GENERATED, model: null }, "  "),
    "Named automatically from the first message by d4d4d4d4…d4d4",
  );
});

test("keyboard and screen-reader users get the attribution, not only hover", () => {
  const label =
    "Auto-named: Named automatically from the first message by d4d4d4d4…d4d4 · claude-haiku-4-5";
  assert.equal(sessionNameOriginAccessibleLabel(GENERATED), label);
  const html = marker(GENERATED);
  assert.match(html, /role="img"/);
  assert.ok(html.includes(`aria-label="${label}"`), html);
  // It sits inside row buttons, so it never takes focus itself.
  assert.doesNotMatch(html, /tabindex/i);
  // Visible text is unchanged.
  assert.match(html, />Auto-named</);
});

function lane(labelOrigin) {
  return {
    laneId: "lane-1",
    sessionKey: "session-1",
    sessionRef: "session-1",
    channelId: "chan-1",
    label: "Login redirect fix",
    labelOrigin,
    goal: null,
    runtimeLabel: null,
    coordination: "open_unverified",
    reportedStatus: null,
    reportedAgeSeconds: null,
    activity: null,
    activityIsError: false,
    executionCount: 1,
    leaseExpiresAt: null,
    openTarget: null,
  };
}

test("an Agent Progress lane marks a generated title and leaves a person's name bare", () => {
  const generated = renderToStaticMarkup(
    React.createElement(AgentProgressLaneRow, { lane: lane(GENERATED) }),
  );
  assert.match(generated, /data-testid="agent-progress-lane-title-origin"/);
  const person = renderToStaticMarkup(
    React.createElement(AgentProgressLaneRow, { lane: lane(PERSON) }),
  );
  assert.doesNotMatch(person, /Auto-named/);
});
