import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

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
const FIRST_SENTENCE = "Lane A is done on both sides.";

async function fixture() {
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
  const record = {
    generationId: "gen-1",
    label: "claude-agent-acp · generation 1",
    title: "Singularity",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: "2026-08-29T06:10:00.000Z",
    status: "completed",
    statusAt: null,
    statusEventId: null,
    transcript: [
      {
        id: itemId(1),
        type: "message",
        renderClass: "message",
        role: "assistant",
        title: "Assistant",
        text: `${FIRST_SENTENCE} The gates ran clean.`,
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
        status: "completed",
        args: {},
        result: "",
        isError: false,
        timestamp: "2026-08-29T06:09:30.000Z",
        turnId,
      },
      {
        id: itemId(3),
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Completed in 2s",
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
    props: {
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

async function mount(props) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");
  const { CodingSessionUmbrellaTurnBlock } = await import(
    "./CodingSessionUmbrellaTurnBlock.tsx"
  );
  // The block's markdown renderer calls `useAppNavigation`, so it needs a
  // loaded router. `render` cannot await `router.load()`, so the router is
  // built and loaded here and the props live in a harness component's state —
  // which is what makes a *re-render of the same instance* possible, and that
  // is the whole point of this file: F3 is about state surviving a prop change.
  let update = null;
  function Harness() {
    const [current, setCurrent] = React.useState(props);
    update = setCurrent;
    return React.createElement(CodingSessionUmbrellaTurnBlock, current);
  }
  const rootRoute = createRootRoute({
    component: () => React.createElement(Harness),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  const view = render(React.createElement(RouterProvider, { router }));
  return {
    view,
    rerender: async (next) => {
      await act(async () => {
        update?.((current) => ({ ...current, ...next }));
      });
    },
    click: async (node) => {
      await act(async () => {
        node.dispatchEvent(
          new dom.window.MouseEvent("click", {
            bubbles: true,
            cancelable: true,
          }),
        );
      });
    },
  };
}

const collapsed = () =>
  document.querySelectorAll(
    '[data-testid="coding-session-umbrella-turn-collapsed"]',
  );
const expandedControl = () =>
  document.querySelectorAll(
    '[data-testid="coding-session-umbrella-turn-expanded"]',
  );

test("F3: a block that settles while the reader is watching stays open", async () => {
  const { props } = await fixture();
  const { rerender } = await mount({ ...props, isWorking: true });
  assert.equal(collapsed().length, 0, "a working block is open");

  // The turn finishes. Nothing about the reader changed.
  await rerender({ isWorking: false });
  assert.equal(
    collapsed().length,
    0,
    "the block shut itself the instant the turn settled",
  );
  assert.equal(expandedControl().length, 1, "and it offers to shut now");
});

test("F3: a block that was already settled when it mounted opens collapsed", async () => {
  const { props } = await fixture();
  await mount({ ...props, isWorking: false });
  assert.equal(collapsed().length, 1);
  assert.equal(expandedControl().length, 0);
});

test("F4: the disclosure toggles both ways, with a real aria-expanded each way", async () => {
  const { props } = await fixture();
  const { click } = await mount({ ...props, isWorking: false });

  const shut = collapsed()[0];
  assert.ok(shut, "a settled block opens collapsed");
  assert.equal(shut.getAttribute("aria-expanded"), "false");
  await click(shut);
  assert.equal(collapsed().length, 0, "clicking did not open the block");

  const open = expandedControl()[0];
  assert.ok(open, "an opened block must offer to shut again");
  assert.equal(open.getAttribute("aria-expanded"), "true");
  await click(open);
  assert.equal(collapsed().length, 1, "the disclosure is one-way");
});
