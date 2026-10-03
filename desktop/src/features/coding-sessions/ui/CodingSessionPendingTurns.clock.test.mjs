/**
 * The pending clock re-renders the pending row, and nothing above it.
 *
 * Before: `useVisibleCodingSessionPendingTurns` owned a 2 s interval, so every
 * tick re-rendered the whole workspace that called it — header, transcript,
 * composer — while a turn waited. Now the row owns its clock; the host
 * re-renders only when which rows survive could change.
 */
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "6f1e0a52-3c7d-4a43-9d55-6c1b7c0f2a10";
const TARGET_KEY = "target-key-1";

async function loadStore() {
  return import("../lib/codingSessionPendingTurns.ts");
}

beforeEach(async () => {
  (await loadStore()).resetPendingCodingSessionTurns();
});

test("a stalling row changes its own caption without re-rendering its host", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionPendingTurnList, useVisibleCodingSessionPendingTurns } =
    await import("./CodingSessionPendingTurns.tsx");
  const store = await loadStore();
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");

  // Published, unanswered, and 1.5 s short of the stall threshold: within one
  // row-clock tick it must start saying "Not picked up yet".
  store.recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: TARGET_KEY,
    commandId: "csc-stalling",
    text: "run the suite",
    operatorPubkey: null,
    recordedAt: Date.now() - store.PENDING_CODING_SESSION_TURN_STALL_MS + 1_500,
    published: true,
  });

  let hostRenders = 0;
  const ECHOES = [];
  function Host() {
    hostRenders += 1;
    const pending = useVisibleCodingSessionPendingTurns({
      channelId: CHANNEL_ID,
      echoes: ECHOES,
      targetKey: TARGET_KEY,
    });
    return React.createElement(CodingSessionPendingTurnList, pending);
  }
  const rootRoute = createRootRoute({
    component: () => React.createElement(Host),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();

  try {
    await act(async () => {
      render(React.createElement(RouterProvider, { router }));
    });
    const status = () =>
      screen.queryByTestId("coding-session-pending-turn-status")?.textContent ??
      null;
    assert.equal(status(), null, "an in-flight row says nothing yet");
    const rendersAtMount = hostRenders;

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 4_300));
    });
    assert.equal(status(), "Not picked up yet", "the row aged by itself");
    assert.equal(
      hostRenders,
      rendersAtMount,
      "two row-clock ticks re-rendered the host zero times (was one per tick)",
    );
    console.log(
      `pending clock over 4.3 s: host re-renders ${hostRenders - rendersAtMount} (before: one per 2 s tick)`,
    );
  } finally {
    cleanup();
  }
});

test("the host re-reads the clock when a row's TTL runs out, and the row goes", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionPendingTurnList, useVisibleCodingSessionPendingTurns } =
    await import("./CodingSessionPendingTurns.tsx");
  const store = await loadStore();
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");

  // 300 ms before its TTL: the host has to notice without any poll.
  store.recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: TARGET_KEY,
    commandId: "csc-expiring",
    text: "nobody will answer this",
    operatorPubkey: null,
    recordedAt: Date.now() - store.PENDING_CODING_SESSION_TURN_TTL_MS + 300,
    published: true,
  });
  const ECHOES = [];
  function Host() {
    const pending = useVisibleCodingSessionPendingTurns({
      channelId: CHANNEL_ID,
      echoes: ECHOES,
      targetKey: TARGET_KEY,
    });
    return React.createElement(CodingSessionPendingTurnList, pending);
  }
  const rootRoute = createRootRoute({
    component: () => React.createElement(Host),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  try {
    await act(async () => {
      render(React.createElement(RouterProvider, { router }));
    });
    assert.ok(screen.queryByTestId("coding-session-pending-turn"));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 600));
    });
    assert.equal(screen.queryByTestId("coding-session-pending-turn"), null);
    assert.equal(store.readPendingCodingSessionTurns().length, 0);
  } finally {
    cleanup();
  }
});

test("an expiry timer that wakes early re-arms instead of leaving the row stuck", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionPendingTurnList, useVisibleCodingSessionPendingTurns } =
    await import("./CodingSessionPendingTurns.tsx");
  const store = await loadStore();
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");

  // Every expiry-sized timer (100 ms – 10 s) wakes after 10 ms instead: the
  // clock adjustment / early-timer case. Row clocks (exactly 2 s) and React's
  // own short timers are left alone.
  const realSetTimeout = dom.window.setTimeout;
  let earlyWakes = 0;
  dom.window.setTimeout = (callback, delay, ...rest) => {
    if (delay > 100 && delay < 10_000 && delay !== 2_000) {
      earlyWakes += 1;
      return realSetTimeout.call(dom.window, callback, 10, ...rest);
    }
    return realSetTimeout.call(dom.window, callback, delay, ...rest);
  };

  // 400 ms before its TTL.
  store.recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey: TARGET_KEY,
    commandId: "csc-early-timer",
    text: "the timer lies about the time",
    operatorPubkey: null,
    recordedAt: Date.now() - store.PENDING_CODING_SESSION_TURN_TTL_MS + 400,
    published: true,
  });
  const ECHOES = [];
  function Host() {
    const pending = useVisibleCodingSessionPendingTurns({
      channelId: CHANNEL_ID,
      echoes: ECHOES,
      targetKey: TARGET_KEY,
    });
    return React.createElement(CodingSessionPendingTurnList, pending);
  }
  const rootRoute = createRootRoute({
    component: () => React.createElement(Host),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  try {
    await act(async () => {
      render(React.createElement(RouterProvider, { router }));
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    assert.ok(earlyWakes >= 1, "the expiry timer woke early at least once");
    assert.ok(
      screen.queryByTestId("coding-session-pending-turn"),
      "an early wake does not expire a row whose TTL has not passed",
    );
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 900));
    });
    assert.equal(
      screen.queryByTestId("coding-session-pending-turn"),
      null,
      "the re-armed deadline expired the row once its TTL truly passed",
    );
    assert.equal(store.readPendingCodingSessionTurns().length, 0);
  } finally {
    dom.window.setTimeout = realSetTimeout;
    cleanup();
  }
});
