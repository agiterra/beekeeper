/**
 * SV-22 / SV-20: the Terminal badge's tone reaches the header's bottom
 * toggle. The header's off-screen probe reads `data-tone`; a running command
 * in this session's shell with the drawer closed must dot the bottom toggle
 * in the activity tone, the badge's own, never the neutral "something new".
 *
 * Mounted in a DOM with the real header and the real Terminal badge, because
 * the dot is read from what the badge draws.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { fakeSurfaceShell } from "../CodingSessionHeaderPanelToggles.testFixtures.mjs";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    DocumentFragment: dom.window.DocumentFragment,
    Element: dom.window.Element,
    Event: dom.window.Event,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    MutationObserver: dom.window.MutationObserver,
    Node: dom.window.Node,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    ResizeObserver: class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
    window: dom.window,
  });
});
after(() => dom.window.close());

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

function terminalExtension(runningCount) {
  return {
    shells: [],
    shellsLoading: false,
    runningIds: new Set(),
    idleIds: new Set(),
    runningCount,
    shared: [],
    foregroundUnknown: false,
    sharedState: "ready",
    sharedTruncated: false,
    myPubkey: null,
  };
}

async function mountWithRunning(runningCount) {
  const React = await import("react");
  const { act, render, screen } = await import("@testing-library/react");
  const { CodingSessionHeaderPanelToggles } = await import(
    "../CodingSessionHeaderPanelToggles.tsx"
  );
  const { CodingSessionSurfaceTerminalBadge } = await import(
    "./CodingSessionSurfaceTerminalBadge.tsx"
  );
  const { shell } = fakeSurfaceShell({
    surfaces: [
      {
        id: "terminal",
        label: "Terminal",
        placement: "drawer",
        Badge: CodingSessionSurfaceTerminalBadge,
      },
    ],
  });
  shell.ctx.extensions = { terminal: terminalExtension(runningCount) };
  await act(async () => {
    render(
      React.createElement(CodingSessionHeaderPanelToggles, {
        shell,
        surfaceHostId: "surface-host",
      }),
    );
  });
  return screen;
}

test("a running command with the drawer closed dots the bottom toggle in the activity tone", async () => {
  const s = await mountWithRunning(1);
  const dot = s.getByTestId("coding-session-panel-toggle-bottom-dot");
  assert.equal(dot.dataset.tone, "activity");
  assert.match(
    s
      .getByTestId("coding-session-panel-toggle-bottom")
      .getAttribute("aria-label"),
    /Terminal/,
  );
});

test("at the prompt the badge clears and the bottom toggle is bare", async () => {
  const s = await mountWithRunning(0);
  assert.equal(s.queryByTestId("coding-session-panel-toggle-bottom-dot"), null);
});
