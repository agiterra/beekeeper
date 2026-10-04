import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * Mission Live's execution bundle reports a still-running seat's call as
 * Running, a finished turn's unended call as "Did not finish", and a call
 * nobody can vouch for as "Status unknown". The bundle holds only tool items,
 * so it never sees the Turn result itself: the block hands its facts down.
 * It used to pass `isWorking={false}`, which told a reader watching a working
 * seat that its running call "Did not finish".
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
  if (!globalThis.ResizeObserver) {
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
  }
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const LEAD_ACTOR = "ede63017".repeat(8);
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";

async function fixture({ status = "running", completed = false } = {}) {
  const { encodeStructuredKey } = await import("../lib/codingSessionKeys.ts");
  const { groupCodingSessionCatalog } = await import(
    "../lib/codingSessionUmbrellaModel.ts"
  );
  const { buildUmbrellaTimeline } = await import(
    "../lib/codingSessionUmbrellaTimeline.ts"
  );
  const { missionRowClass } = await import(
    "../lib/codingSessionMissionRowGrammar.ts"
  );
  const itemId = (seq) =>
    encodeStructuredKey(
      "coding-session-transcript-item/v1",
      "generation-scope",
      "target-key",
      String(seq),
    );
  const turnId = "turn-1";
  const transcript = [
    {
      id: itemId(1),
      type: "message",
      renderClass: "message",
      role: "assistant",
      title: "Assistant",
      text: "Running the gate now.",
      timestamp: "2026-08-29T06:09:00.000Z",
      turnId,
    },
    {
      id: itemId(2),
      type: "tool",
      renderClass: "shell",
      descriptor: { renderClass: "shell", label: "shell", preview: null },
      title: "tool-2",
      toolName: "tool-2",
      buzzToolName: null,
      status: "executing",
      args: { command: "just ci" },
      result: "",
      isError: false,
      timestamp: "2026-08-29T06:09:30.000Z",
      turnId,
    },
  ];
  if (completed) {
    transcript.push({
      id: itemId(3),
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "Completed in 2s",
      timestamp: "2026-08-29T06:09:40.000Z",
      turnId,
      durationMs: 2_000,
      costUsd: null,
    });
  }
  const record = {
    generationId: "gen-1",
    label: "claude-agent-acp · generation 1",
    title: "Singularity",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: "2026-08-29T06:10:00.000Z",
    status,
    statusAt: null,
    statusEventId: null,
    transcript,
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "claude-instance",
      sessionId: "11111111-1111-1111-1111-111111111111",
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-cc-1",
    runtime: "claude-agent-acp",
    model: "claude-opus-5",
    agentRef: LEAD_ACTOR,
    role: "lead",
    turnBudget: null,
    routing: null,
    capabilities: null,
  };
  const umbrella = groupCodingSessionCatalog([record])[0];
  const block = buildUmbrellaTimeline(umbrella, []).find(
    (entry) => entry.kind === "turn-block",
  );
  assert.ok(block, "the fixture must produce a turn block");
  return {
    umbrella,
    blockProps: {
      block,
      blockKey: "block-under-test",
      channelId: CHANNEL_ID,
      currentUserPubkey: null,
      isHighlighted: false,
      isFolded: false,
      label: "Keystone · Lead",
      labelsByExecutionKey: new Map(),
      missionCollapseSettled: true,
      missionExecutionBundle: true,
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
    },
  };
}

async function mount(element) {
  const React = (await import("react")).default;
  const { render } = await import("@testing-library/react");
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return render(React.createElement(RouterProvider, { router }));
}

async function mountBlock(props) {
  const React = (await import("react")).default;
  const { CodingSessionUmbrellaTurnBlock } = await import(
    "./CodingSessionUmbrellaTurnBlock.tsx"
  );
  return mount(React.createElement(CodingSessionUmbrellaTurnBlock, props));
}

/** Open every collapsed block and every bundle, as a reader would. */
async function openBundles() {
  const { act } = await import("@testing-library/react");
  const click = async (node) => {
    await act(async () => {
      node.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    });
  };
  for (const node of document.querySelectorAll(
    '[data-testid="coding-session-umbrella-turn-collapsed"]',
  )) {
    await click(node);
  }
  const toggles = document.querySelectorAll(
    '[data-testid="coding-session-mission-execution-bundle-toggle"]',
  );
  assert.ok(toggles.length > 0, "the Live bundle must be on screen");
  for (const toggle of toggles) await click(toggle);
}

function bundleTool() {
  const tool = document.querySelector(
    '[data-testid="coding-session-mission-execution-bundle"] [data-testid="coding-session-active-tool"]',
  );
  assert.ok(tool, "the bundle must render the executing call");
  return tool;
}

test("(a) a Mission Live working block's executing call reads Running", async () => {
  const { blockProps } = await fixture({ status: "running" });
  await mountBlock({ ...blockProps, isWorking: true });
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Running/);
  assert.doesNotMatch(tool.textContent, /Did not finish|Status unknown/);
  assert.equal(tool.hasAttribute("data-tool-unfinished"), false);
  // The block's narrative already draws the working line; the bundle does
  // not draw a second one.
  assert.equal(
    document.querySelectorAll(
      '[data-testid="coding-session-mission-execution-bundle"] [data-testid="coding-session-working"]',
    ).length,
    0,
  );
});

test("(a) the same through the timeline: a running seat's Live bundle reads Running", async () => {
  const React = (await import("react")).default;
  const { CodingSessionUmbrellaTimelineView } = await import(
    "./CodingSessionUmbrellaTimelineView.tsx"
  );
  const { umbrella } = await fixture({ status: "running" });
  await mount(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Running/);
  assert.doesNotMatch(tool.textContent, /Did not finish|Status unknown/);
});

test("a completed block's unended call did not finish, even while the seat still reads running", async () => {
  const { blockProps } = await fixture({ status: "running", completed: true });
  await mountBlock({ ...blockProps, isWorking: true });
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Did not finish/);
  assert.equal(tool.hasAttribute("data-tool-unfinished"), true);
});

test("(b) a waiting seat's unended call says its status is unknown", async () => {
  const React = (await import("react")).default;
  const { CodingSessionUmbrellaTimelineView } = await import(
    "./CodingSessionUmbrellaTimelineView.tsx"
  );
  const { umbrella } = await fixture({ status: "waiting_for_input" });
  await mount(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Status unknown/);
  assert.doesNotMatch(tool.textContent, /Running|Did not finish/);
  assert.equal(tool.hasAttribute("data-tool-status-unknown"), true);
});

test("(b) an idle seat's unended call did not finish", async () => {
  const React = (await import("react")).default;
  const { CodingSessionUmbrellaTimelineView } = await import(
    "./CodingSessionUmbrellaTimelineView.tsx"
  );
  const { umbrella } = await fixture({ status: "idle" });
  await mount(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Did not finish/);
});

test("(b) a block given no resting status says unknown, never a verdict", async () => {
  const { blockProps } = await fixture({ status: "idle" });
  await mountBlock({ ...blockProps, isWorking: false });
  await openBundles();
  const tool = bundleTool();
  assert.match(tool.textContent, /Status unknown/);
  assert.doesNotMatch(tool.textContent, /Running|Did not finish/);
});
