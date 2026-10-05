/**
 * SV-20: the header's bottom- and right-panel toggles.
 *
 * Mounted in a DOM, because the dot is read from what each off-screen
 * surface's own Badge draws — a static render never runs the read.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

import {
  CLOSED_PANELS,
  fakeSurfaceShell,
  toneBadge,
} from "./CodingSessionHeaderPanelToggles.testFixtures.mjs";

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

async function mount(shell) {
  const React = await import("react");
  const { act, render } = await import("@testing-library/react");
  const { CodingSessionHeaderPanelToggles } = await import(
    "./CodingSessionHeaderPanelToggles.tsx"
  );
  let view;
  await act(async () => {
    view = render(
      React.createElement(CodingSessionHeaderPanelToggles, {
        shell,
        surfaceHostId: "surface-host",
      }),
    );
  });
  return {
    async rerender(next) {
      await act(async () => {
        view.rerender(
          React.createElement(CodingSessionHeaderPanelToggles, {
            shell: next,
            surfaceHostId: "surface-host",
          }),
        );
      });
    },
  };
}

async function screen() {
  return (await import("@testing-library/react")).screen;
}

async function click(element) {
  const { act, fireEvent } = await import("@testing-library/react");
  await act(async () => {
    fireEvent.click(element);
  });
}

const BADGED = [
  {
    id: "agents",
    label: "Agents",
    Badge: toneBadge("activity", "2 subagents running"),
  },
  {
    id: "landing",
    label: "Landing",
    Badge: toneBadge("attention", "The newest gate on the head failed"),
  },
  { id: "terminal", label: "Terminal", placement: "drawer" },
];

test("the toggles show pressed while their panel is open, and toggle it", async () => {
  const closed = fakeSurfaceShell({ surfaces: BADGED });
  await mount(closed.shell);
  const s = await screen();
  const right = s.getByTestId("coding-session-panel-toggle-right");
  const bottom = s.getByTestId("coding-session-panel-toggle-bottom");
  assert.equal(right.getAttribute("aria-pressed"), "false");
  assert.equal(bottom.getAttribute("aria-pressed"), "false");
  assert.equal(right.getAttribute("aria-controls"), "surface-host");
  await click(right);
  await click(bottom);
  assert.equal(closed.calls.get("toggleRight"), 1);
  assert.equal(closed.calls.get("toggleBottom"), 1);
});

test("an open panel and drawer read as pressed", async () => {
  const open = fakeSurfaceShell({
    surfaces: BADGED,
    panelState: {
      ...CLOSED_PANELS,
      rightOpen: true,
      bottomOpen: true,
      tabs: ["agents"],
      active: "agents",
    },
  });
  await mount(open.shell);
  const s = await screen();
  assert.equal(
    s
      .getByTestId("coding-session-panel-toggle-right")
      .getAttribute("aria-pressed"),
    "true",
  );
  assert.equal(
    s
      .getByTestId("coding-session-panel-toggle-bottom")
      .getAttribute("aria-pressed"),
    "true",
  );
});

test("with the panel closed, the right toggle's dot takes the strongest off-screen tone and names each badge", async () => {
  const { shell } = fakeSurfaceShell({ surfaces: BADGED });
  await mount(shell);
  const s = await screen();
  const dot = s.getByTestId("coding-session-panel-toggle-right-dot");
  assert.equal(dot.dataset.tone, "attention");
  const label = s
    .getByTestId("coding-session-panel-toggle-right")
    .getAttribute("aria-label");
  assert.match(label, /^Toggle right panel; not on screen: /);
  assert.match(label, /Agents: 2 subagents running/);
  assert.match(label, /Landing: The newest gate on the head failed/);
  // Terminal is a drawer surface and drew no badge: the bottom toggle is bare.
  assert.equal(s.queryByTestId("coding-session-panel-toggle-bottom-dot"), null);
});

test("the launcher shows every badge, so an open launcher clears the dot", async () => {
  const { shell } = fakeSurfaceShell({
    surfaces: BADGED,
    panelState: { ...CLOSED_PANELS, rightOpen: true },
  });
  await mount(shell);
  const s = await screen();
  assert.equal(s.queryByTestId("coding-session-panel-toggle-right-dot"), null);
  assert.equal(
    s
      .getByTestId("coding-session-panel-toggle-right")
      .getAttribute("aria-label"),
    "Toggle right panel",
  );
});

test("an open tab shows its own badge; a surface that is not a tab still dots the toggle", async () => {
  const { shell } = fakeSurfaceShell({
    surfaces: BADGED,
    panelState: {
      ...CLOSED_PANELS,
      rightOpen: true,
      tabs: ["landing"],
      active: "landing",
    },
  });
  await mount(shell);
  const s = await screen();
  const dot = s.getByTestId("coding-session-panel-toggle-right-dot");
  assert.equal(dot.dataset.tone, "activity");
  assert.doesNotMatch(
    s
      .getByTestId("coding-session-panel-toggle-right")
      .getAttribute("aria-label"),
    /Landing/,
  );
});

test("a drawer surface's badge dots the bottom toggle while the drawer is closed", async () => {
  const { shell } = fakeSurfaceShell({
    surfaces: [
      {
        id: "terminal",
        label: "Terminal",
        placement: "drawer",
        Badge: toneBadge("activity", "1 command running"),
      },
    ],
  });
  await mount(shell);
  const s = await screen();
  assert.equal(
    s.getByTestId("coding-session-panel-toggle-bottom-dot").dataset.tone,
    "activity",
  );
  assert.equal(s.queryByTestId("coding-session-panel-toggle-right-dot"), null);
});

test("a badge that draws no tone reads as neutral, never as more", async () => {
  const { shell } = fakeSurfaceShell({
    surfaces: [{ id: "diff", label: "Diff", Badge: toneBadge("untoned") }],
  });
  await mount(shell);
  const s = await screen();
  assert.equal(
    s.getByTestId("coding-session-panel-toggle-right-dot").dataset.tone,
    "neutral",
  );
});

test("no badge drawn, no dot", async () => {
  const { shell } = fakeSurfaceShell({
    surfaces: [{ id: "plan", label: "Plan", Badge: toneBadge(null) }],
  });
  await mount(shell);
  const s = await screen();
  assert.equal(s.queryByTestId("coding-session-panel-toggle-right-dot"), null);
});

test("the dot follows a badge that changes on its own", async () => {
  const React = await import("react");
  const { act } = await import("@testing-library/react");
  let setTone = () => {};
  function LiveBadge() {
    const [tone, set] = React.useState(null);
    setTone = set;
    return tone
      ? React.createElement("span", { "data-tone": tone }, "•")
      : null;
  }
  const { shell } = fakeSurfaceShell({
    surfaces: [{ id: "agents", label: "Agents", Badge: LiveBadge }],
  });
  await mount(shell);
  const s = await screen();
  assert.equal(s.queryByTestId("coding-session-panel-toggle-right-dot"), null);
  await act(async () => {
    setTone("waiting");
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(
    s.getByTestId("coding-session-panel-toggle-right-dot").dataset.tone,
    "waiting",
  );
  await act(async () => {
    setTone(null);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  assert.equal(s.queryByTestId("coding-session-panel-toggle-right-dot"), null);
});

test("an unavailable drawer dims the bottom toggle with its reason, and a click opens it like the shortcut", async () => {
  const reason = "The working tree is on another computer.";
  const dimmed = fakeSurfaceShell({
    surfaces: BADGED,
    bottomUnavailableReason: reason,
  });
  const view = await mount(dimmed.shell);
  const s = await screen();
  let bottom = s.getByTestId("coding-session-panel-toggle-bottom");
  // Dimmed and explained, but not refused: ⌘J opens the drawer to show the
  // reason, so the click must too, or the control and its shortcut disagree.
  assert.equal(bottom.getAttribute("aria-disabled"), null);
  assert.equal(bottom.dataset.unavailable, "true");
  assert.match(bottom.className, /opacity-50/);
  assert.match(bottom.getAttribute("aria-label"), new RegExp(reason));
  await click(bottom);
  assert.equal(dimmed.calls.get("toggleBottom"), 1);

  // Once open, the toggle reads as pressed (not dimmed) and closes it again.
  const opened = fakeSurfaceShell({
    surfaces: BADGED,
    bottomUnavailableReason: reason,
    panelState: { ...CLOSED_PANELS, bottomOpen: true },
  });
  await view.rerender(opened.shell);
  bottom = s.getByTestId("coding-session-panel-toggle-bottom");
  assert.equal(bottom.dataset.unavailable, undefined);
  await click(bottom);
  assert.equal(opened.calls.get("toggleBottom"), 1);
});

test("pure helpers: strongest tone and what is on screen", async () => {
  const { codingSessionSurfaceBadgeOnScreen, strongestCodingSessionBadgeTone } =
    await import("./CodingSessionHeaderPanelToggles.tsx");
  assert.equal(strongestCodingSessionBadgeTone([]), null);
  assert.equal(
    strongestCodingSessionBadgeTone([
      { tone: "neutral" },
      { tone: "waiting" },
      { tone: "activity" },
    ]),
    "waiting",
  );
  const closed = { ...CLOSED_PANELS };
  assert.equal(codingSessionSurfaceBadgeOnScreen("right", "a", closed), false);
  assert.equal(
    codingSessionSurfaceBadgeOnScreen("right", "a", {
      ...closed,
      rightOpen: true,
    }),
    true,
  );
  assert.equal(
    codingSessionSurfaceBadgeOnScreen("right", "a", {
      ...closed,
      rightOpen: true,
      tabs: ["b"],
      active: "b",
    }),
    false,
  );
  assert.equal(
    codingSessionSurfaceBadgeOnScreen("drawer", "t", {
      ...closed,
      bottomOpen: true,
    }),
    true,
  );
});
