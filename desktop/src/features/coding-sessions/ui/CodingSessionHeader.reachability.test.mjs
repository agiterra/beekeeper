/**
 * Every control the session header had before SESSION_VIEW_UX_PLAN L4 is
 * still reachable, and still does what it did.
 *
 * The header went from about fourteen controls in one row to title, status,
 * the full-access badge and a few primary controls, with the rest in the `⋯`
 * menu. A fold may hide detail, never a capability: so this mounts the real
 * header in a DOM, opens the real menu, and clicks each former control
 * through to its handler. A static render cannot do this — Radix portals the
 * menu's content only on open.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    CustomEvent: dom.window.CustomEvent,
    document: dom.window.document,
    DocumentFragment: dom.window.DocumentFragment,
    DOMRect: dom.window.DOMRect,
    Element: dom.window.Element,
    Event: dom.window.Event,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    HTMLButtonElement: dom.window.HTMLButtonElement,
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    MutationObserver: dom.window.MutationObserver,
    Node: dom.window.Node,
    NodeFilter: dom.window.NodeFilter,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    ResizeObserver:
      dom.window.ResizeObserver ??
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    cancelAnimationFrame: (handle) => clearTimeout(handle),
    requestAnimationFrame: (callback) => setTimeout(() => callback(0), 0),
    window: dom.window,
  });
});
after(() => dom.window.close());

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

function spies() {
  const calls = new Map();
  const spy = (name) => () => calls.set(name, (calls.get(name) ?? 0) + 1);
  return { calls, spy };
}

async function mountHeader(overrides = {}) {
  const React = await import("react");
  const { act, render } = await import("@testing-library/react");
  const { CodingSessionHeader } = await import("./CodingSessionHeader.tsx");
  const { calls, spy } = spies();
  const props = {
    channelName: "Hive Sessions",
    generationLabel: "Keystone Session · generation 2",
    isExporting: false,
    onAddProvider: spy("addProvider"),
    onClose: spy("dismiss"),
    onCloseSession: spy("closeSession"),
    onExport: spy("export"),
    fullAccess: {
      granted: true,
      pending: null,
      error: null,
      toggle: spy("fullAccess"),
      watcher: null,
    },
    onOpenPeople: spy("people"),
    onOpenProject: spy("project"),
    onPopout: spy("popout"),
    onRename: spy("rename"),
    onStopAll: spy("stopAll"),
    onToggleRouteRail: spy("routeRail"),
    onToggleSurface: (id) => spy(`surface:${id}`)(),
    onToggleTaskRail: spy("plan"),
    peopleCount: 4,
    projectName: "Buzz Glue",
    sessionTitle: "Keystone Session",
    status: { kind: "working", label: "Working" },
    stopAllCount: 2,
    surfaceHostId: "surface-host",
    surfaceTabs: [
      {
        id: "agents",
        label: "Agents",
        icon: "agents",
        count: 2,
        active: false,
      },
      {
        id: "changes",
        label: "Observed changes",
        icon: "changes",
        count: 5,
        active: false,
      },
    ],
    taskCount: 3,
    taskRailOpen: false,
    ...overrides,
  };
  await act(async () => {
    render(React.createElement(CodingSessionHeader, props));
  });
  return { calls };
}

async function click(element) {
  const { act, fireEvent } = await import("@testing-library/react");
  await act(async () => {
    fireEvent.click(element);
  });
}

async function openMenu() {
  const { screen } = await import("@testing-library/react");
  const trigger = screen.getByTestId("coding-session-overflow");
  const { act, fireEvent } = await import("@testing-library/react");
  await act(async () => {
    // Radix opens a popover on pointerdown-free click; both are sent so the
    // test does not depend on which one this version listens to.
    fireEvent.pointerDown(trigger, { button: 0, pointerType: "mouse" });
    fireEvent.click(trigger);
  });
  if (!screen.queryByRole("group", { name: "Session actions" })) {
    await act(async () => {
      fireEvent.click(trigger);
    });
  }
  return screen.getByRole("group", { name: "Session actions" });
}

/** Every control the header carried before L4, and where it lives now. */
const ROW_CONTROLS = [
  ["coding-session-dismiss", "dismiss"],
  ["coding-session-rename", "rename"],
  ["coding-session-project-crumb", "project"],
  ["coding-session-task-rail-toggle", "plan"],
  ["coding-session-surface-toggle-agents", "surface:agents"],
  ["coding-session-surface-toggle-changes", "surface:changes"],
  ["coding-session-route-toggle", "routeRail"],
];

const MENU_CONTROLS = [
  ["coding-session-overflow-add-provider", "addProvider", "Add provider…"],
  ["coding-session-overflow-stop-all", "stopAll", "Stop all (2 seats)"],
  ["coding-session-overflow-close-session", "closeSession", "Close session"],
  ["coding-session-overflow-export", "export", "Export transcript"],
  ["coding-session-overflow-popout", "popout", "Pop out"],
  ["coding-session-overflow-full-access", "fullAccess", "Full access"],
];

test("every control that stayed in the row still reaches its handler", async () => {
  const { screen } = await import("@testing-library/react");
  const { calls } = await mountHeader();
  for (const [testId, handler] of ROW_CONTROLS) {
    await click(screen.getByTestId(testId));
    assert.equal(calls.get(handler), 1, `${testId} → ${handler}`);
  }
  // Live counts stay on the row, never behind the menu.
  assert.match(
    screen.getByTestId("coding-session-surface-toggle-agents").textContent,
    /2/,
  );
  assert.match(
    screen.getByTestId("coding-session-surface-toggle-changes").textContent,
    /5/,
  );
  // People moved into Details; its count stayed on the trigger.
  assert.match(
    screen.getByTestId("coding-session-details-people-count").textContent,
    /4/,
  );
  assert.match(
    screen.getByTestId("coding-session-task-rail-toggle").textContent,
    /3/,
  );
  // Status is visible without a click.
  assert.ok(screen.getByTestId("coding-session-status-badge"));
});

test("Details holds People and provenance: same content, same ids, one control", async () => {
  const { act, fireEvent, screen } = await import("@testing-library/react");
  const { calls } = await mountHeader({
    founderDetails: "Ada",
    providerAuthorityPubkey: "ab".repeat(32),
  });
  const trigger = screen.getByTestId("coding-session-provenance-toggle");
  assert.match(trigger.getAttribute("aria-label"), /4 people with access/);
  // Before it opens, neither former control is a separate row button.
  assert.equal(screen.queryByTestId("coding-session-people-toggle"), null);
  await act(async () => {
    fireEvent.pointerDown(trigger, { button: 0, pointerType: "mouse" });
    fireEvent.click(trigger);
  });
  if (!screen.queryByTestId("coding-session-provenance-details")) {
    await click(trigger);
  }
  // The provenance body, verbatim.
  const details = screen.getByTestId("coding-session-provenance-details");
  assert.match(details.textContent, /Founded by/);
  assert.match(details.textContent, /Ada/);
  assert.match(details.textContent, /Signed projection/);
  // People: the same id, the same handler, the count on screen.
  const people = screen.getByTestId("coding-session-people-toggle");
  assert.match(people.textContent, /4 people with access/);
  await click(people);
  assert.equal(calls.get("people"), 1);
  // It hands over to the People dialog rather than staying open behind it.
  assert.equal(screen.queryByTestId("coding-session-provenance-details"), null);
});

test("Details without a People surface is provenance alone, with no count", async () => {
  const { screen } = await import("@testing-library/react");
  await mountHeader({ onOpenPeople: undefined });
  const trigger = screen.getByTestId("coding-session-provenance-toggle");
  assert.equal(trigger.getAttribute("aria-label"), "Show session details");
  assert.equal(
    screen.queryByTestId("coding-session-details-people-count"),
    null,
  );
});

test("every action that moved into ⋯ is there, labelled, and reaches its handler", async () => {
  const { calls } = await mountHeader();
  for (const [testId, handler, label] of MENU_CONTROLS) {
    const menu = await openMenu();
    const item = menu.querySelector(`[data-testid="${testId}"]`);
    assert.ok(item, `${testId} must be in the ⋯ menu`);
    assert.match(item.textContent, new RegExp(escapeRegExp(label)));
    await click(item);
    assert.equal(calls.get(handler), 1, `${testId} → ${handler}`);
  }
});

test("the workspace item is offered from the same menu when the header has a session", async () => {
  await mountHeader({
    workspaceReuse: { channelId: "channel-1", sessionRef: "session-1" },
  });
  const menu = await openMenu();
  assert.ok(
    menu.querySelector(
      '[data-testid="coding-session-overflow-new-session-in-workspace"]',
    ),
  );
});

test("the moved controls keep their consequence lines and their disabled state", async () => {
  await mountHeader({ isExporting: true });
  const menu = await openMenu();
  const stopAll = menu.querySelector(
    '[data-testid="coding-session-overflow-stop-all"]',
  );
  // The flat button's title said the stopped seat cannot be resumed; the
  // item says it on screen, not in a tooltip.
  assert.match(stopAll.textContent, /cannot be resumed/);
  const close = menu.querySelector(
    '[data-testid="coding-session-overflow-close-session"]',
  );
  assert.match(close.textContent, /Settled/);
  assert.match(close.textContent, /providers keep running/);
  const exportItem = menu.querySelector(
    '[data-testid="coding-session-overflow-export"]',
  );
  assert.equal(exportItem.disabled, true);
});

test("choosing an item closes the menu it was chosen from", async () => {
  const { screen } = await import("@testing-library/react");
  await mountHeader();
  const menu = await openMenu();
  await click(
    menu.querySelector('[data-testid="coding-session-overflow-popout"]'),
  );
  assert.equal(screen.queryByRole("group", { name: "Session actions" }), null);
});

test("a closed session keeps Reopen in the row and the plan stands down while shown", async () => {
  const { screen } = await import("@testing-library/react");
  let reopened = 0;
  await mountHeader({
    onCloseSession: undefined,
    onReopenSession: () => {
      reopened += 1;
    },
    sessionClosed: true,
    taskRailOpen: true,
  });
  await click(screen.getByTestId("coding-session-reopen"));
  assert.equal(reopened, 1);
  // The rail is on screen, so the header offers no second way to the plan.
  assert.equal(screen.queryByTestId("coding-session-task-rail-toggle"), null);
});

test("the docked plan opens only when asked, and its close still reaches the dock", async () => {
  const React = await import("react");
  const { act, render, screen } = await import("@testing-library/react");
  const { CodingSessionTaskRail } = await import("./CodingSessionTaskRail.tsx");
  let closed = 0;
  await act(async () => {
    render(
      React.createElement(CodingSessionTaskRail, {
        model: {
          sourceItemId: "plan-1",
          turnId: "turn-1",
          timestamp: "2026-10-03T12:00:00.000Z",
          completedCount: 0,
          explanation: null,
          copyText: null,
          state: "active",
          tasks: [
            { id: "a", text: "Split the header", status: "in_progress" },
            { id: "b", text: "Compact the composer", status: "pending" },
          ],
        },
        onClose: () => {
          closed += 1;
        },
        variant: "dock",
      }),
    );
  });
  const dock = screen.getByTestId("coding-session-task-dock");
  assert.equal(dock.dataset.expanded, "false");
  assert.equal(screen.queryByText("Compact the composer"), null);
  await click(screen.getByTestId("coding-session-task-dock-toggle"));
  assert.equal(dock.dataset.expanded, "true");
  assert.ok(screen.getByText("Compact the composer"));
  await click(screen.getByLabelText("Close session tasks"));
  assert.equal(closed, 1);
});

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
