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
import { buildUmbrellaTimeline } from "../lib/codingSessionUmbrellaTimeline.ts";
import { missionRowClass } from "../lib/codingSessionMissionRowGrammar.ts";
import { codingSessionAgentAccent } from "./CodingSessionAgentFocus.tsx";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";
import { CodingSessionUmbrellaTurnBlock } from "./CodingSessionUmbrellaTurnBlock.tsx";

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const LEAD_ACTOR = "ede63017".repeat(8);
const DESIGNER_ACTOR = "efefd4e5".repeat(8);
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

function toolItem(seq, renderClass, turnId) {
  return {
    id: itemId(seq),
    type: "tool",
    renderClass,
    descriptor: { renderClass, label: renderClass, preview: null },
    title: `tool-${seq}`,
    toolName: `tool-${seq}`,
    beekeeperToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: "2026-08-29T06:09:30.000Z",
    turnId,
  };
}

function seat({ sessionId, agentRef, role }) {
  return {
    generationId: `gen-${sessionId}`,
    label: `claude-agent-acp · generation 1`,
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
      toolItem(`${sessionId}-2`, "shell", `turn-${sessionId}`),
      toolItem(`${sessionId}-3`, "shell", `turn-${sessionId}`),
      toolItem(`${sessionId}-4`, "file-read", `turn-${sessionId}`),
      toolItem(`${sessionId}-5`, "file-edit", `turn-${sessionId}`),
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
      sessionId: "11111111-1111-1111-1111-111111111112",
      agentRef: DESIGNER_ACTOR,
      role: "designer",
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

/** Every turn-block shell class attribute, in render order. */
function shellClasses(markup) {
  return [
    ...markup.matchAll(
      /<article class="([^"]*)"[^>]*data-testid="coding-session-umbrella-turn-block"/g,
    ),
  ].map((match) => match[1]);
}

test("U-F1: the Mission shell keeps each seat's own identity accent", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  const shells = shellClasses(markup);
  assert.equal(shells.length, 2, "two seats, two turn blocks");
  const accents = umbrella.executions.map(
    (execution) => codingSessionAgentAccent(execution.executionKey).border,
  );
  assert.notEqual(
    accents[0],
    accents[1],
    "the fixture must give the two seats different accents",
  );
  // The card grammar is on both...
  for (const shell of shells) {
    assert.match(shell, /rounded-xl/);
    assert.match(shell, /px-3/);
  }
  // ...and each still wears its own rail, so the reader can tell them apart.
  assert.notEqual(
    shells[0],
    shells[1],
    "Mission must distinguish the seats too",
  );
  for (const [index, shell] of shells.entries()) {
    assert.ok(
      shell.includes(accents[index]),
      `identity accent ${accents[index]} missing from: ${shell}`,
    );
    assert.doesNotMatch(
      shell,
      /border-border\/60(?!\S)/,
      `the grey grammar border outranks the accent in: ${shell}`,
    );
  }
});

test("U-F1: Conversation's shell is unchanged and still carries the accent", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      onHandoff: () => {},
      umbrella,
    }),
  );
  const shells = shellClasses(markup);
  assert.equal(shells.length, 2);
  assert.notEqual(shells[0], shells[1]);
  for (const [index, shell] of shells.entries()) {
    assert.doesNotMatch(shell, /rounded-xl/);
    assert.match(shell, /border-l-2/);
    assert.ok(
      shell.includes(
        codingSessionAgentAccent(umbrella.executions[index].executionKey)
          .border,
      ),
    );
  }
});

test("U-F1: a highlighted Mission block keeps its focus tint", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const entries = buildUmbrellaTimeline(umbrella, []);
  const block = entries.find((entry) => entry.kind === "turn-block");
  assert.ok(block, "the fixture must produce a turn block");
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      block,
      blockKey: "block-under-test",
      channelId: CHANNEL_ID,
      currentUserPubkey: null,
      isHighlighted: true,
      isFolded: false,
      isWorking: false,
      label: "Keystone · Lead",
      labelsByExecutionKey: new Map(),
      missionRowClassName: missionRowClass("standard", {
        className: "border-l-2",
      }),
      onHandoff: () => {},
      onRegisterNode: () => {},
      onRevealFact: () => {},
      operatorProfiles: undefined,
      record: umbrella.executions[0].activeGeneration,
      resolveFactLocation: () => null,
      showProvenance: true,
      stickyProvenance: false,
      umbrella,
    }),
  );
  const shell = shellClasses(markup)[0];
  assert.ok(shell, "the block renders a shell");
  assert.match(shell, /bg-primary\/5/, `focus tint lost in: ${shell}`);
  assert.match(shell, /ring-primary\/60/);
  assert.doesNotMatch(
    shell,
    /bg-background(?!\S)/,
    `the grammar background outranks the focus tint in: ${shell}`,
  );
});

test("R2 C2: Live collapses a turn's signed tool items into one bundle row", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  const toggles = [
    ...markup.matchAll(
      /data-count="(\d+)" data-testid="coding-session-mission-execution-bundle-toggle"/g,
    ),
  ];
  assert.equal(toggles.length, 2, "one bundle per turn block");
  for (const toggle of toggles) assert.equal(toggle[1], "4");
  assert.match(markup, />4 execution events</);
  assert.match(markup, />Terminal 2 · Read 1 · Edit 1</);
  // Collapsed by default, and announced as such.
  assert.match(markup, /aria-expanded="false"/);
  // The prose the turn actually said is still inline.
  assert.match(markup, /Reporting as lead\./);
});

test("R2 C2: Trace and Brief leave the items where they are", async () => {
  const umbrella = umbrellaOfTwoSeats();
  for (const missionDensity of ["trace", "brief"]) {
    const markup = await renderInRouter(
      React.createElement(CodingSessionUmbrellaTimelineView, {
        channelId: CHANNEL_ID,
        laneMessages: [],
        missionDensity,
        onHandoff: () => {},
        umbrella,
      }),
    );
    assert.doesNotMatch(
      markup,
      /coding-session-mission-execution-bundle/,
      `${missionDensity} must not collapse execution`,
    );
  }
});

test("R2 C2: the bundle is double-gated — Conversation cannot reach it", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const entries = buildUmbrellaTimeline(umbrella, []);
  const block = entries.find((entry) => entry.kind === "turn-block");
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
    onHandoff: () => {},
    onRegisterNode: () => {},
    onRevealFact: () => {},
    operatorProfiles: undefined,
    record: umbrella.executions[0].activeGeneration,
    resolveFactLocation: () => null,
    showProvenance: true,
    stickyProvenance: false,
    umbrella,
  };
  const conversation = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, props),
  );
  // The flag alone, without Mission's grammar, changes nothing.
  const flagOnly = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      ...props,
      missionExecutionBundle: true,
    }),
  );
  assert.equal(flagOnly, conversation);
  assert.doesNotMatch(conversation, /coding-session-mission-execution-bundle/);
  // Both together do.
  const mission = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTurnBlock, {
      ...props,
      missionExecutionBundle: true,
      missionRowClassName: missionRowClass("standard", {
        className: "border-l-2",
      }),
    }),
  );
  assert.match(mission, /coding-session-mission-execution-bundle/);
});

test("R2 C2: Conversation's DOM is byte-identical with every Mission prop set", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const base = {
    channelId: CHANNEL_ID,
    currentUserPubkey: null,
    laneMessages: [
      {
        eventId: "e".repeat(64),
        sessionRef: SESSION_REF,
        channelId: CHANNEL_ID,
        authorPubkey: "3d3b7169".repeat(8),
        content: "Walk the ten-minute path and report.",
        timestampMs: Date.parse("2026-08-29T06:07:00.000Z"),
      },
    ],
    onHandoff: () => {},
    umbrella,
  };
  const bare = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, base),
  );
  const loaded = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      ...base,
      missionDeliveries: [
        {
          sourceEventId: "d".repeat(64),
          operationType: "report",
          sourceActorPubkey: LEAD_ACTOR,
          leadTargetKey: "lead-target",
          kind: "failed",
          owningCommandId: "cmd-1",
          duplicateRefusedCommandIds: [],
          failures: [],
          reArmCount: 1,
          observedAtMs: 1,
          detail: "Wake delivery failed",
        },
      ],
      missionFounderPubkey: "3d3b7169".repeat(8),
      missionTransactions: [
        {
          sourceEventId: "a".repeat(64),
          type: "report",
          authorPubkey: LEAD_ACTOR,
          createdAt: 1_800_000_000,
          counterpartyPubkey: DESIGNER_ACTOR,
          parentEventId: null,
          summary: "Dispatch extraction",
          decision: null,
          requiredAction: null,
          fileCount: 3,
          testCount: 3,
          unseated: true,
        },
      ],
      resolveMissionActor: () => ({ label: "Bob", executionKey: "builder" }),
      // REVIEW-A3 I8: the reviewer's own cross-tree check passed this prop and
      // the lane's in-tree one did not. It is a Mission prop like the rest —
      // Conversation must ignore it just as completely.
      missionLiveness: new Map([
        ["lead", { word: "live", live: true }],
        ["designer", { word: "no provider answering", live: false }],
      ]),
    }),
  );
  assert.ok(bare.length > 2000, "the fixture must be substantial");
  assert.equal(loaded.length, bare.length);
  assert.equal(loaded, bare);
  assert.doesNotMatch(bare, /coding-session-mission-transaction-row/);
  assert.doesNotMatch(bare, /coding-session-mission-execution-bundle/);
});

/** The turn block on its own, in Mission, with the liveness the caller states. */
function missionBlockProps(
  umbrella,
  block,
  liveness,
  { isWorking = true } = {},
) {
  return {
    block,
    blockKey: "block-under-test",
    channelId: CHANNEL_ID,
    currentUserPubkey: null,
    isHighlighted: false,
    isFolded: false,
    isWorking,
    label: "Keystone · Lead",
    labelsByExecutionKey: new Map(),
    liveness,
    missionRowClassName: missionRowClass("standard", {
      className: "border-l-2",
    }),
    onHandoff: () => {},
    onRegisterNode: () => {},
    onRevealFact: () => {},
    operatorProfiles: undefined,
    record: umbrella.executions[0].activeGeneration,
    resolveFactLocation: () => null,
    showProvenance: true,
    stickyProvenance: false,
    umbrella,
  };
}

function firstTurnBlock(umbrella) {
  return buildUmbrellaTimeline(umbrella, []).find(
    (entry) => entry.kind === "turn-block",
  );
}

/**
 * F3: `isWorking` is the raw wire status; the W1 word is that status **after**
 * reachability demotion. A seat whose provider stopped answering is still
 * `running` on the wire and reads `no provider answering` on screen — and the
 * roster chip for it does not breathe (`CodingSessionParticipantBar.tsx:69`).
 * The block must make the same call, or one seat gets two answers.
 */
test("A3.2/F3: a demoted seat says its word and does not breathe", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(
      CodingSessionUmbrellaTurnBlock,
      missionBlockProps(umbrella, firstTurnBlock(umbrella), {
        word: "no provider answering",
        live: false,
      }),
    ),
  );
  assert.match(markup, />no provider answering</);
  assert.doesNotMatch(markup, /coding-session-agent-breathe/);
});

test("A3.2/F3: a seat W1 calls working breathes", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(
      CodingSessionUmbrellaTurnBlock,
      missionBlockProps(umbrella, firstTurnBlock(umbrella), {
        word: "live",
        live: true,
      }),
    ),
  );
  assert.match(markup, />live</);
  assert.match(markup, /coding-session-agent-breathe/);
});

/**
 * F7: DESIGN-SPEC §3 C1/C1a puts the animation on the block's identity rail.
 * `coding-session-agent-breathe` animates a `box-shadow`, so on the `<article>`
 * it rings the whole card — every other use in the app is a chip or an 8px dot.
 */
test("A3.2/F7: the breathe is on the accent rail, never the card", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(
      CodingSessionUmbrellaTurnBlock,
      missionBlockProps(umbrella, firstTurnBlock(umbrella), {
        word: "live",
        live: true,
      }),
    ),
  );
  const rail = markup.match(
    /<span[^>]*data-testid="coding-session-umbrella-turn-rail"[^>]*>/,
  );
  assert.ok(rail, "the live block renders its own rail element");
  assert.match(rail[0], /coding-session-agent-breathe/);
  for (const shell of shellClasses(markup)) {
    assert.doesNotMatch(
      shell,
      /coding-session-agent-breathe/,
      "the card shell must not carry the animation",
    );
  }
});

test("A3.2/F7: a settled block renders no rail element at all", async () => {
  const umbrella = umbrellaOfTwoSeats();
  const markup = await renderInRouter(
    React.createElement(
      CodingSessionUmbrellaTurnBlock,
      missionBlockProps(
        umbrella,
        firstTurnBlock(umbrella),
        { word: "live", live: true },
        { isWorking: false },
      ),
    ),
  );
  assert.doesNotMatch(markup, /coding-session-umbrella-turn-rail/);
  assert.doesNotMatch(markup, /coding-session-agent-breathe/);
});
