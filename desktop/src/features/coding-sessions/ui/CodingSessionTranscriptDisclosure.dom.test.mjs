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
    HTMLDetailsElement: dom.window.HTMLDetailsElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    // The router takes a defined `window` to mean a browser, and a browser
    // to have `self`.
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

const timestamp = "2026-07-30T12:00:00.000Z";

function message(id, role, text, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId,
  };
}

function tool(id, turnId) {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: {
      renderClass: "shell",
      label: "Ran command",
      preview: "bun test",
      action: { verb: "Ran", object: "bun test" },
    },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status: "completed",
    args: { command: "bun test" },
    result: "10 pass",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId,
  };
}

function result(id, turnId, text) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text,
    timestamp,
    turnId,
  };
}

function settledTurn(turnId) {
  return [
    message(`${turnId}-prompt`, "user", `Prompt ${turnId}`, turnId),
    tool(`${turnId}-tool-1`, turnId),
    tool(`${turnId}-tool-2`, turnId),
    message(`${turnId}-answer`, "assistant", `Answer ${turnId}`, turnId),
    result(`${turnId}-result`, turnId, `Answer ${turnId} (1200ms)`),
  ];
}

async function mount(props) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { CodingSessionTranscript } = await import(
    "./CodingSessionTranscript.tsx"
  );
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");
  // Markdown links read the router, as in the app.
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: createRootRoute({
      component: () => React.createElement(CodingSessionTranscript, props),
    }),
  });
  await router.load();
  let view;
  await act(async () => {
    view = render(React.createElement(RouterProvider, { router }));
  });
  return { act, view };
}

test("the store notifies only the toggled id's subscribers", async () => {
  const { createCodingSessionDisclosureStore } = await import(
    "./CodingSessionTranscriptDisclosure.ts"
  );
  const store = createCodingSessionDisclosureStore(["seeded"]);
  const calls = { a: 0, b: 0 };
  const stopA = store.subscribe("a", () => {
    calls.a += 1;
  });
  store.subscribe("b", () => {
    calls.b += 1;
  });

  assert.equal(store.isOpen("seeded"), true);
  store.setOpen("a", true);
  store.setOpen("a", true); // no change, no notification
  assert.deepEqual(calls, { a: 1, b: 0 });
  stopA();
  store.setOpen("a", false);
  assert.deepEqual(calls, { a: 1, b: 0 });
  assert.equal(store.isOpen("a"), false);
});

test("opening one turn's fold changes that turn alone, and survives a remount", async () => {
  const { createCodingSessionDisclosureStore } = await import(
    "./CodingSessionTranscriptDisclosure.ts"
  );
  const store = createCodingSessionDisclosureStore();
  const items = [...settledTurn("turn-1"), ...settledTurn("turn-2")];
  const props = {
    disclosureStore: store,
    generationId: "generation-1",
    isWorking: false,
    items,
  };
  const { act, view } = await mount(props);

  const turns = () => [
    ...view.container.querySelectorAll('[data-testid="coding-session-turn"]'),
  ];
  assert.equal(turns().length, 2);
  const secondTurnAnswer = turns()[1].querySelector(
    '[data-testid="coding-session-assistant-message"]',
  );
  const folds = view.container.querySelectorAll(
    '[data-testid="coding-session-worked-fold"]',
  );
  assert.equal(folds.length, 2);
  assert.equal(
    view.container.querySelectorAll('[data-testid="coding-session-tool-group"]')
      .length,
    0,
    "settled work starts folded",
  );

  await act(async () => {
    folds[0].dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true }),
    );
  });

  assert.equal(store.isOpen("fold:turn-1"), true);
  assert.equal(store.isOpen("fold:turn-2"), false);
  assert.equal(turns()[0].dataset.fold, "open");
  assert.equal(turns()[1].dataset.fold, "closed");
  assert.equal(
    turns()[0].querySelectorAll('[data-testid="coding-session-tool-group"]')
      .length,
    1,
  );
  // The other turn was not rebuilt: the very same node is still mounted.
  assert.equal(
    turns()[1].querySelector(
      '[data-testid="coding-session-assistant-message"]',
    ),
    secondTurnAnswer,
  );

  // A remount (what the virtualizer does to a row scrolled away) comes back
  // as it was left, because the state is not in the row.
  view.unmount();
  const again = await mount(props);
  const remounted = [
    ...again.view.container.querySelectorAll(
      '[data-testid="coding-session-turn"]',
    ),
  ];
  assert.equal(remounted[0].dataset.fold, "open");
  assert.equal(remounted[1].dataset.fold, "closed");
});

test("the working line ticks without a React render", async () => {
  const startedAt = new Date(Date.now() - 5_000).toISOString();
  const { act, view } = await mount({
    generationId: "generation-1",
    isWorking: true,
    items: [
      { ...message("prompt", "user", "Go", "turn-1"), timestamp: startedAt },
    ],
  });
  const working = view.container.querySelector(
    '[data-testid="coding-session-working"]',
  );
  assert.ok(working);
  const label = working.querySelector("span");
  const textNode = label.firstChild;
  assert.match(label.textContent, /^Working for \d/);
  assert.equal(
    working.getAttribute("role"),
    null,
    "not a per-second live region",
  );
  // The interval writes React's own text node in place.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 1_100));
  });
  assert.equal(label.firstChild, textNode);
  assert.match(label.textContent, /^Working for \d/);
});
