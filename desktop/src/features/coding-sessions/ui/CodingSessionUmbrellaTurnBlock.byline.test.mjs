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

import { encodeStructuredKey } from "../lib/codingSessionKeys.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaWorkspace.tsx";

// Walk finding 4 (docs/design/singularity/WALK-2026-08-29.md:185): all three
// seats' blocks carried this one key, because one provider signs for every
// seat in the session.
const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const LEAD_ACTOR = "ede63017".repeat(8);
const DESIGNER_ACTOR = "efefd4e5".repeat(8);
const FOUNDER = "3d3b7169".repeat(8);
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";

function itemId(seq) {
  return encodeStructuredKey(
    "coding-session-transcript-item/v1",
    "generation-scope",
    "target-key",
    String(seq),
  );
}

function seat({ sessionId, agentRef, role, generation = 1 }) {
  const target = {
    driver: "claude-agent-acp",
    instanceId: "claude-instance",
    sessionId,
    generation,
  };
  return {
    generationId: `gen-${sessionId}`,
    label: `claude-agent-acp · generation ${generation}`,
    title: "Singularity",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: "2026-08-29T06:10:00.000Z",
    status: "completed",
    statusAt: null,
    transcript: [
      {
        id: itemId(`${sessionId}-1`),
        type: "message",
        renderClass: "message",
        role: "assistant",
        title: "Assistant",
        text: `Reporting as ${role}.`,
        timestamp: "2026-08-29T06:09:00.000Z",
        turnId: `turn-${sessionId}`,
      },
    ],
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-cc-1",
    runtime: "claude-agent-acp",
    model: "claude-opus-5",
    agentRef,
    role,
    turnBudget: null,
    capabilities: null,
  };
}

function umbrellaOfTwoSeats() {
  const umbrellas = groupCodingSessionCatalog([
    seat({
      sessionId: "11111111-1111-1111-1111-111111111111",
      agentRef: LEAD_ACTOR,
      role: "lead",
    }),
    seat({
      sessionId: "22222222-2222-2222-2222-222222222222",
      agentRef: DESIGNER_ACTOR,
      role: "designer",
    }),
  ]);
  assert.equal(umbrellas.length, 1);
  assert.equal(umbrellas[0].executions.length, 2);
  return umbrellas[0];
}

async function renderTimeline(props) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionUmbrellaTimelineView, props),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("each seat's byline names its own actor, and the provider key appears nowhere", async () => {
  const markup = await renderTimeline({
    actorNames: (actorPubkey) =>
      actorPubkey === LEAD_ACTOR
        ? "Keystone"
        : actorPubkey === DESIGNER_ACTOR
          ? "Banksy"
          : null,
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: umbrellaOfTwoSeats(),
  });

  assert.match(markup, />Keystone · Lead</);
  assert.match(markup, />Banksy · Designer</);
  // The one key that used to be the headline on every seat.
  assert.doesNotMatch(markup, /1958c6c4…c6c4/);
  assert.doesNotMatch(markup, /signer 1958c6c4/);
  assert.match(
    markup,
    /Response from Keystone, Lead, generation 1, via claude-cc-1\./,
  );
  assert.match(
    markup,
    /Response from Banksy, Designer, generation 1, via claude-cc-1\./,
  );
  // The provider is provenance, not identity: hover and screen reader only.
  assert.match(markup, /title="via claude-cc-1"/);
});

test("a seat with no resolved profile reads role and runtime, not a key", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: umbrellaOfTwoSeats(),
  });

  assert.match(markup, />Lead</);
  assert.match(markup, />Designer</);
  assert.match(markup, /Claude Code · claude-opus-5/);
  assert.doesNotMatch(markup, /1958c6c4…c6c4/);
  assert.match(markup, /Response from Lead, generation 1, via claude-cc-1\./);
});

test("the founder's own lane message is named, not stamped with a key", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    currentUserPubkey: FOUNDER,
    laneMessages: [
      {
        eventId: "e".repeat(64),
        sessionRef: SESSION_REF,
        channelId: CHANNEL_ID,
        authorPubkey: FOUNDER,
        content: "Walk the ten-minute path and report.",
        timestampMs: Date.parse("2026-08-29T06:07:00.000Z"),
      },
    ],
    onHandoff: () => {},
    umbrella: umbrellaOfTwoSeats(),
  });

  assert.match(markup, /Walk the ten-minute path and report\./);
  assert.match(markup, />You</);
  assert.doesNotMatch(markup, /3d3b7169…7169/);
});
