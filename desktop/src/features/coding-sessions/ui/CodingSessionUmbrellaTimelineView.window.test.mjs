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
import { resetCodingSessionNarrativeMemory } from "../hooks/useCodingSessionBottomAnchor.ts";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SIGNER = "a".repeat(64);
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

function item(seq, fields) {
  return {
    id: encodeStructuredKey(
      "coding-session-transcript-item/v1",
      "generation-scope",
      "target-key",
      String(seq),
    ),
    ...fields,
  };
}

/** One execution with `turns` settled turns, a minute apart. */
function umbrellaWithTurns(turns) {
  const transcript = [];
  for (let n = 0; n < turns; n += 1) {
    const minute = String(n).padStart(2, "0");
    const turnId = `turn-${n}`;
    transcript.push(
      item(n * 3 + 1, {
        type: "message",
        renderClass: "message",
        role: "user",
        title: "Brian",
        text: `prompt number ${n}`,
        timestamp: `2026-08-12T10:${minute}:00.000Z`,
        turnId,
      }),
      item(n * 3 + 2, {
        type: "message",
        renderClass: "message",
        role: "assistant",
        title: "Assistant",
        text: `answer number ${n}`,
        timestamp: `2026-08-12T10:${minute}:20.000Z`,
        turnId,
      }),
      item(n * 3 + 3, {
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Done",
        timestamp: `2026-08-12T10:${minute}:40.000Z`,
        turnId,
      }),
    );
  }
  const umbrellas = groupCodingSessionCatalog([
    {
      generationId: "gen-aaaa",
      label: "claude-agent-acp · generation 1",
      title: "Long session",
      providerAuthorityPubkey: SIGNER,
      metadataAuthorityPubkey: SIGNER,
      lastEventAt: "2026-08-12T11:00:00.000Z",
      status: "completed",
      transcript,
      conflictCount: 0,
      commandTarget: TARGET,
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      provider: null,
      runtime: "claude",
      model: "claude-opus-5",
      capabilities: null,
    },
  ]);
  return umbrellas[0];
}

async function render(umbrella) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionUmbrellaTimelineView, {
        channelId: CHANNEL_ID,
        laneMessages: [],
        onHandoff: () => {},
        scrollMemoryKey: `${CHANNEL_ID}:${SESSION_REF}`,
        umbrella,
      }),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("a long umbrella renders its last ten turns under a Load earlier control", async () => {
  resetCodingSessionNarrativeMemory();
  const markup = await render(umbrellaWithTurns(14));
  const blocks =
    markup.match(/data-testid="coding-session-umbrella-turn-block"/g) ?? [];
  assert.equal(blocks.length, 10);
  assert.match(markup, /data-testid="coding-session-umbrella-load-earlier"/);
  assert.match(markup, /4 earlier turns not shown/);
  assert.match(markup, /answer number 13/);
  assert.match(markup, /answer number 4/);
  assert.doesNotMatch(markup, /answer number 3\b/);
  // The control sits above the first rendered turn.
  assert.ok(
    markup.indexOf("coding-session-umbrella-load-earlier") <
      markup.indexOf("coding-session-umbrella-turn-block"),
  );
});

test("a short umbrella renders every turn and no control", async () => {
  resetCodingSessionNarrativeMemory();
  const markup = await render(umbrellaWithTurns(3));
  const blocks =
    markup.match(/data-testid="coding-session-umbrella-turn-block"/g) ?? [];
  assert.equal(blocks.length, 3);
  assert.doesNotMatch(markup, /coding-session-umbrella-load-earlier/);
});
