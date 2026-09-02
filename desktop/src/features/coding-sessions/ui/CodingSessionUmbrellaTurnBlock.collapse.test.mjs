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
import { missionRowClass } from "../lib/codingSessionMissionRowGrammar.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { buildUmbrellaTimeline } from "../lib/codingSessionUmbrellaTimeline.ts";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";
import {
  CodingSessionUmbrellaTurnBlock,
  codingSessionCollapsedTurnBlockLine,
  hasCodingSessionMissionAttentionItem,
} from "./CodingSessionUmbrellaTurnBlock.tsx";

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const LEAD_ACTOR = "ede63017".repeat(8);
const BUILDER_ACTOR = "efefd4e5".repeat(8);
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const FIRST_SENTENCE = "Lane A is done on both sides.";
const REST = "The gates ran clean and the branch is pushed.";
const TURN_RESULT_TEXT = "Completed in 2s";

function itemId(seq) {
  return encodeStructuredKey(
    "coding-session-transcript-item/v1",
    "generation-scope",
    "target-key",
    String(seq),
  );
}

function toolItem(seq, turnId, overrides = {}) {
  return {
    id: itemId(seq),
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: "shell", preview: null },
    title: `tool-${seq}`,
    toolName: `tool-${seq}`,
    buzzToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: "2026-08-29T06:09:30.000Z",
    turnId,
    ...overrides,
  };
}

function seat({ sessionId, agentRef, role, status = "completed", extra = [] }) {
  const turnId = `turn-${sessionId}`;
  return {
    generationId: `gen-${sessionId}`,
    label: "claude-agent-acp · generation 1",
    title: "Singularity",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: "2026-08-29T06:10:00.000Z",
    status,
    statusAt: null,
    statusEventId: null,
    transcript: [
      {
        id: itemId(`${sessionId}-1`),
        type: "message",
        renderClass: "message",
        role: "assistant",
        title: "Assistant",
        text: `${FIRST_SENTENCE} ${REST}`,
        timestamp: "2026-08-29T06:09:00.000Z",
        turnId,
      },
      toolItem(`${sessionId}-2`, turnId),
      toolItem(`${sessionId}-3`, turnId),
      toolItem(`${sessionId}-4`, turnId),
      ...extra.map((overrides, index) =>
        toolItem(`${sessionId}-x${index}`, turnId, overrides),
      ),
      // The block's own terminator. `isCompletedCodingSessionTurnBlock` is the
      // settled test — the same one the footer and the stream's ordering use —
      // so a turn with no `Turn result` row has not settled and never
      // collapses, however quiet it has gone.
      {
        id: itemId(`${sessionId}-end`),
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: TURN_RESULT_TEXT,
        timestamp: "2026-08-29T06:09:40.000Z",
        turnId,
        durationMs: 2_000,
        costUsd: null,
      },
    ],
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "claude-instance",
      sessionId,
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-cc-1",
    runtime: "claude-agent-acp",
    model: "claude-opus-5",
    agentRef,
    role,
    turnBudget: null,
    routing: null,
    capabilities: null,
  };
}

function umbrella({ builderStatus = "completed", builderExtra = [] } = {}) {
  const umbrellas = groupCodingSessionCatalog([
    seat({
      sessionId: "11111111-1111-1111-1111-111111111111",
      agentRef: LEAD_ACTOR,
      role: "lead",
    }),
    seat({
      sessionId: "11111111-1111-1111-1111-111111111112",
      agentRef: BUILDER_ACTOR,
      role: "builder",
      status: builderStatus,
      extra: builderExtra,
    }),
  ]);
  assert.equal(umbrellas[0].executions.length, 2);
  return umbrellas[0];
}

async function renderInRouter(element) {
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

function renderStream(density, options = {}) {
  return renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: density,
      onHandoff: () => {},
      umbrella: umbrella(options),
    }),
  );
}

function collapsedCounts(markup) {
  return [
    ...markup.matchAll(
      /data-count="(\d+)"[^>]*data-testid="coding-session-umbrella-turn-collapsed"/g,
    ),
  ].map((match) => Number(match[1]));
}

test("A4.3: a settled block opens at one line in Mission Live", async () => {
  const markup = await renderStream("live");
  const counts = collapsedCounts(markup);
  assert.equal(counts.length, 2, "both settled blocks collapse");
  // F5: the number is the rows Mission Live actually reveals — the assistant
  // message, the one execution-bundle row standing for the three tool items,
  // and the turn result. Counting raw items promised 5 and revealed 3.
  assert.deepEqual(counts, [3, 3]);
  assert.match(markup, /data-collapsed="true"/);
  // The line is the first sentence of the first assistant message, verbatim —
  // never a summary this client wrote.
  assert.ok(markup.includes(FIRST_SENTENCE));
  assert.ok(
    !markup.includes(REST),
    "the collapsed line stops at the first sentence",
  );
  assert.ok(
    !markup.includes('data-testid="coding-session-transcript"'),
    "a collapsed block renders no transcript at all",
  );
  assert.match(markup, /3 rows/);
});

test("A4.3 reversibility: the count equals the rows expanding reveals in Live", async () => {
  // The same block, rendered twice: shut, then open — **with the execution
  // bundle on**, which is how Mission Live renders it. The lane's first
  // version of this test omitted `missionExecutionBundle` and so proved a
  // configuration the product never ships (REVIEW-A4 F5).
  const record = umbrella().executions[0].activeGeneration;
  const block = buildUmbrellaTimeline(umbrella(), []).find(
    (entry) => entry.kind === "turn-block",
  );
  assert.ok(block, "the fixture must produce a turn block");
  const props = {
    block,
    blockKey: "block-under-test",
    channelId: CHANNEL_ID,
    currentUserPubkey: null,
    isHighlighted: false,
    isFolded: false,
    isWorking: false,
    label: "Keystone · Lead",
    labelsByExecutionKey: new Map(),
    missionExecutionBundle: true,
    missionRowClassName: missionRowClass("standard", {
      className: "border-l-2",
    }),
    onHandoff: () => {},
    onRegisterNode: () => {},
    onRevealFact: () => {},
    operatorProfiles: undefined,
    record,
    resolveFactLocation: () => null,
    showProvenance: true,
    stickyProvenance: false,
    umbrella: umbrella(),
  };
  const shut = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      ...props,
      missionCollapseSettled: true,
    }),
  );
  const open = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      ...props,
      missionCollapseSettled: false,
    }),
  );
  const promised = collapsedCounts(shut);
  assert.deepEqual(promised, [3]);
  const count = (markup, needle) => markup.split(needle).length - 1;
  // Three rows, each with its own witness: the assistant message, the single
  // bundle row standing for the tools, and the turn result.
  assert.ok(open.includes(FIRST_SENTENCE) && open.includes(REST));
  assert.equal(
    count(open, 'data-testid="coding-session-mission-execution-bundle"'),
    1,
  );
  assert.equal(count(open, 'data-testid="coding-session-turn-completion"'), 1);
  // The bundle's own count carries the tools one level down — C2's existing
  // reversibility contract, which this one composes with rather than repeats.
  const bundleCount = open.match(
    /data-count="(\d+)" data-testid="coding-session-mission-execution-bundle-toggle"/,
  );
  assert.ok(bundleCount, "the bundle must state its own count");
  assert.equal(Number(bundleCount[1]), 3);
  assert.equal(1 + 1 + 1, promised[0], "the collapsed count over-promised");
  // The tool rows are behind the bundle, and nothing at all is on screen while
  // the block is shut.
  assert.equal(count(open, 'data-testid="transcript-tool-item"'), 0);
  assert.equal(count(shut, 'data-testid="transcript-tool-item"'), 0);
  assert.equal(count(shut, 'data-role="assistant-message"'), 0);
  assert.equal(count(shut, 'data-testid="coding-session-turn-completion"'), 0);
  assert.ok(!shut.includes(REST));
});

test("A4.3: a working block never collapses", async () => {
  const markup = await renderStream("live", { builderStatus: "running" });
  const counts = collapsedCounts(markup);
  assert.equal(counts.length, 1, "only the settled lead block collapses");
  assert.match(markup, /data-testid="coding-session-transcript"/);
});

test("A4.3: a block carrying an attention item never collapses", async () => {
  const markup = await renderStream("live", {
    builderExtra: [{ isError: true, status: "failed", title: "tool-failed" }],
  });
  assert.equal(
    collapsedCounts(markup).length,
    1,
    "the failed tool keeps its block open",
  );
});

test("A4.3: Brief, Trace and Conversation are untouched", async () => {
  for (const density of ["brief", "trace"]) {
    assert.equal(
      collapsedCounts(await renderStream(density)).length,
      0,
      `${density} must not collapse`,
    );
  }
  const conversation = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      onHandoff: () => {},
      umbrella: umbrella(),
    }),
  );
  assert.equal(collapsedCounts(conversation).length, 0);
  assert.ok(!conversation.includes("data-collapsed"));
});

test("the collapsed line and the attention predicate answer on their own", () => {
  const long = `${"a".repeat(200)}. tail`;
  const line = codingSessionCollapsedTurnBlockLine([
    {
      id: "1",
      type: "message",
      renderClass: "message",
      role: "assistant",
      title: "Assistant",
      text: long,
      timestamp: "2026-08-29T06:09:00.000Z",
    },
  ]);
  assert.equal(line.itemCount, 1);
  assert.equal(line.sentence.length, 140);
  assert.ok(line.sentence.endsWith("…"));

  // A turn with no assistant message says so rather than inventing a sentence.
  assert.deepEqual(
    codingSessionCollapsedTurnBlockLine([
      {
        id: "2",
        type: "message",
        renderClass: "message",
        role: "user",
        title: "You",
        text: "Do the thing.",
        timestamp: "2026-08-29T06:09:00.000Z",
      },
    ]),
    { sentence: null, itemCount: 1 },
  );

  assert.equal(
    hasCodingSessionMissionAttentionItem([
      {
        id: "3",
        type: "lifecycle",
        renderClass: "error",
        title: "x",
        text: "",
      },
    ]),
    true,
  );
  assert.equal(
    hasCodingSessionMissionAttentionItem([
      {
        id: "4",
        type: "lifecycle",
        renderClass: "permission",
        title: "x",
        text: "",
      },
    ]),
    true,
  );
  assert.equal(
    hasCodingSessionMissionAttentionItem([
      {
        id: "5",
        type: "lifecycle",
        renderClass: "status",
        title: "x",
        text: "",
      },
    ]),
    false,
  );
});
