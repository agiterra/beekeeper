import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";

// SV-80/SV-81: subagent cards open the subagent page, tick only while their
// settled status is running, freeze at their outcome once finished, and carry
// a hover card with model, time, status, tokens, tools and the result.

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
});
const saved = {
  document: globalThis.document,
  window: globalThis.window,
  HTMLElement: globalThis.HTMLElement,
  Node: globalThis.Node,
  IS_REACT_ACT_ENVIRONMENT: globalThis.IS_REACT_ACT_ENVIRONMENT,
};
Object.assign(globalThis, {
  document: dom.window.document,
  window: dom.window,
  HTMLElement: dom.window.HTMLElement,
  Node: dom.window.Node,
  IS_REACT_ACT_ENVIRONMENT: true,
});
after(() => {
  for (const [key, value] of Object.entries(saved)) {
    if (value === undefined) delete globalThis[key];
    else globalThis[key] = value;
  }
  dom.window.close();
});

const React = await import("react");
const { act } = React;
const { createRoot } = await import("react-dom/client");
const { deriveCodingSessionSubagentCard } = await import(
  "../lib/codingSessionSubagentsCard.ts"
);
const { CodingSessionSubagentEntry, CodingSessionSubagentLink } = await import(
  "./CodingSessionSubagentEntry.tsx"
);
const {
  CodingSessionSubagentHoverCardContent,
  codingSessionSubagentStatusDotClass,
} = await import("./CodingSessionSubagentHoverCard.tsx");
const { CodingSessionTurnSettlementContext } = await import(
  "./CodingSessionTranscriptItem.tsx"
);
const { CodingSessionSurfaceCtxProvider } = await import(
  "./surfaces/codingSessionSurfaceContext.tsx"
);
const { codingSessionSubagentScopeOf } = await import(
  "../lib/codingSessionSubagentNavigation.ts"
);
const { readCodingSessionSubagentPage, resetCodingSessionSubagentPages } =
  await import("../lib/codingSessionSubagentPageStore.ts");

function call(overrides = {}) {
  return {
    id: "item-task-1",
    type: "tool",
    renderClass: "subagent",
    title: "Task",
    toolName: "Task",
    beekeeperToolName: null,
    status: "completed",
    args: { description: "Review the parser", subagent_type: "Explore" },
    result: "Found two bugs\nmore detail",
    isError: false,
    toolCallId: "toolu_01",
    subagent: {
      model: "claude-sonnet-4-5",
      totalTokens: 48_200,
      durationMs: 62_000,
      toolUseCount: 7,
    },
    timestamp: "2026-10-05T12:00:00.000Z",
    startedAt: "2026-10-05T12:00:00.000Z",
    completedAt: "2026-10-05T12:01:02.000Z",
    ...overrides,
  };
}

function runningCall(startedAt) {
  return call({
    status: "executing",
    result: "",
    subagent: undefined,
    completedAt: null,
    startedAt,
    timestamp: startedAt,
  });
}

async function mount(element) {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(element));
  return {
    container,
    render: (next) => act(async () => root.render(next)),
    unmount: async () => {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

/** Counts live intervals so a test can see the clock stop. */
function trackIntervals() {
  const live = new Set();
  const originalSet = dom.window.setInterval;
  const originalClear = dom.window.clearInterval;
  dom.window.setInterval = (...args) => {
    const id = originalSet.apply(dom.window, args);
    live.add(id);
    return id;
  };
  dom.window.clearInterval = (id) => {
    live.delete(id);
    originalClear.call(dom.window, id);
  };
  return {
    live,
    restore() {
      dom.window.setInterval = originalSet;
      dom.window.clearInterval = originalClear;
    },
  };
}

test("a finished card is frozen at its outcome and the report's duration", () => {
  const card = deriveCodingSessionSubagentCard({
    call: call(),
    children: [],
    status: "done",
  });
  assert.equal(card.phase, "finished");
  assert.equal(card.outcomeLabel, "Finished");
  assert.equal(card.durationMs, 62_000);
  assert.equal(card.parentToolId, "toolu_01");
  assert.equal(card.detail, "Found two bugs");
  assert.equal(card.totalTokens, 48_200);
  assert.equal(card.toolCount, 7);
});

test("a stopped spawn is finished without a duration or a result", () => {
  const card = deriveCodingSessionSubagentCard({
    call: runningCall("2026-10-05T12:00:00.000Z"),
    children: [],
    status: "stopped",
  });
  assert.equal(card.phase, "finished");
  assert.equal(card.outcomeLabel, "Stopped");
  assert.equal(card.durationMs, null);
  assert.equal(card.detail, null);
});

test("the clock ticks while running and stops when the status settles", async () => {
  const intervals = trackIntervals();
  const startedAt = new Date(Date.now() - 5_000).toISOString();
  const live = deriveCodingSessionSubagentCard({
    call: runningCall(startedAt),
    children: [],
    status: "running",
  });
  const view = await mount(
    React.createElement(CodingSessionSubagentLink, {
      card: live,
      onOpen: null,
    }),
  );
  try {
    const link = view.container.querySelector(
      '[data-testid="coding-session-subagent-link"]',
    );
    assert.equal(link.dataset.phase, "live");
    const clock = view.container.querySelector(
      '[data-testid="coding-session-subagent-elapsed"]',
    );
    assert.equal(clock.dataset.live, "true");
    assert.match(clock.textContent, /^[0-9.]+s$/);
    assert.equal(intervals.live.size, 1, "one ticking interval while live");

    const finished = deriveCodingSessionSubagentCard({
      call: call({ startedAt }),
      children: [],
      status: "done",
    });
    await view.render(
      React.createElement(CodingSessionSubagentLink, {
        card: finished,
        onOpen: null,
      }),
    );
    assert.equal(intervals.live.size, 0, "the interval is gone once settled");
    const finish = view.container.querySelector(
      '[data-testid="coding-session-subagent-finish"]',
    );
    assert.ok(finish, "the settled card is the finish card");
    assert.equal(finish.dataset.phase, "finished");
    const frozen = finish.querySelector(
      '[data-testid="coding-session-subagent-elapsed"]',
    );
    assert.equal(frozen.dataset.live, "false");
    assert.equal(frozen.textContent, "1m 2s");
    assert.equal(
      finish.querySelector('[data-testid="coding-session-subagent-outcome"]')
        .textContent,
      "Finished",
    );
  } finally {
    intervals.restore();
    await view.unmount();
  }
});

test("a click on the card calls the open function with its parentToolId", async () => {
  const opened = [];
  const card = deriveCodingSessionSubagentCard({
    call: call(),
    children: [],
    status: "done",
  });
  const view = await mount(
    React.createElement(CodingSessionSubagentLink, {
      card,
      onOpen: (id) => opened.push(id),
    }),
  );
  try {
    const button = view.container.querySelector("button");
    assert.equal(button.getAttribute("aria-label"), "Open Review the parser");
    await act(async () => button.click());
    assert.deepEqual(opened, ["toolu_01"]);
  } finally {
    await view.unmount();
  }
});

test("without a call id the card is no button, and says why", async () => {
  const card = deriveCodingSessionSubagentCard({
    call: call({ toolCallId: undefined }),
    children: [],
    status: "done",
  });
  const view = await mount(
    React.createElement(CodingSessionSubagentLink, {
      card,
      onOpen: () => assert.fail("nothing to open"),
    }),
  );
  try {
    assert.equal(view.container.querySelector("button"), null);
  } finally {
    await view.unmount();
  }
  const hover = await mount(
    React.createElement(CodingSessionSubagentHoverCardContent, { card }),
  );
  try {
    assert.match(hover.container.textContent, /No page to open/);
  } finally {
    await hover.unmount();
  }
});

function entryOf(spawns) {
  return {
    kind: "subagents",
    id: "subagents:item-task-1",
    label: "Kicked off 1 subagent",
    spawns,
  };
}

test("in a settled turn an open spawn's dot and card read stopped, never running", async () => {
  const intervals = trackIntervals();
  const startedAt = new Date(Date.now() - 5_000).toISOString();
  const entry = entryOf([
    { call: runningCall(startedAt), children: [], status: "running" },
  ]);
  const render = (settlement) =>
    React.createElement(
      CodingSessionTurnSettlementContext.Provider,
      { value: settlement },
      React.createElement(CodingSessionSubagentEntry, {
        disclosureId: "subagents:x",
        entry,
        onOpenChange: () => {},
        open: false,
        renderChild: () => null,
      }),
    );
  const view = await mount(render("live"));
  try {
    const dot = () =>
      view.container.querySelector(
        '[data-testid="coding-session-subagent-cards"] [data-testid="coding-session-subagent-status-dot"]',
      );
    assert.equal(dot().dataset.status, "running");
    assert.equal(intervals.live.size, 1);

    await view.render(render("settled"));
    assert.equal(dot().dataset.status, "stopped");
    assert.equal(intervals.live.size, 0, "no clock for a stopped spawn");
    assert.ok(
      view.container.querySelector(
        '[data-testid="coding-session-subagent-finish"][data-status="stopped"]',
      ),
    );

    await view.render(render("unknown"));
    assert.equal(dot().dataset.status, "unknown");
    assert.equal(intervals.live.size, 0);
    assert.equal(
      view.container.querySelector(
        '[data-testid="coding-session-subagent-elapsed"]',
      ),
      null,
    );
  } finally {
    intervals.restore();
    await view.unmount();
  }
});

test("inside a session workspace the stream card opens that subagent's page", async () => {
  resetCodingSessionSubagentPages();
  const ctx = {
    communityScope: "community-1",
    channelId: "channel-1",
    sessionKey: "session-1",
    layout: "single",
    focusedRecord: { generationId: "generation-1" },
    extensions: {},
  };
  const scope = codingSessionSubagentScopeOf(ctx);
  const entry = entryOf([{ call: call(), children: [], status: "done" }]);
  const view = await mount(
    React.createElement(
      CodingSessionSurfaceCtxProvider,
      { value: ctx },
      React.createElement(CodingSessionSubagentEntry, {
        disclosureId: "subagents:x",
        entry,
        onOpenChange: () => {},
        open: false,
        renderChild: () => null,
      }),
    ),
  );
  try {
    const finish = view.container.querySelector(
      'button[data-testid="coding-session-subagent-finish"]',
    );
    assert.ok(finish, "the finish card is a button inside a workspace");
    assert.ok(finish.querySelector("svg"), "a chevron says it opens");
    await act(async () => finish.click());
    assert.equal(readCodingSessionSubagentPage(scope), "toolu_01");
  } finally {
    resetCodingSessionSubagentPages();
    await view.unmount();
  }
});

test("the hover card says model, time, status, tokens, tools and the result", async () => {
  const card = deriveCodingSessionSubagentCard({
    call: call(),
    children: [],
    status: "done",
  });
  const view = await mount(
    React.createElement(CodingSessionSubagentHoverCardContent, { card }),
  );
  try {
    const text = view.container.textContent;
    // One human name for the model on every subagent surface (SV-98).
    assert.match(text, /ModelClaude Sonnet 4\.5/);
    assert.match(text, /1m 2s/);
    assert.match(text, /Finished/);
    assert.match(text, /48\.2k tok/);
    assert.match(text, /Tools7/);
    assert.match(text, /Result: Found two bugs/);
  } finally {
    await view.unmount();
  }

  const unreported = deriveCodingSessionSubagentCard({
    call: runningCall(new Date().toISOString()),
    children: [],
    status: "stopped",
  });
  const quiet = await mount(
    React.createElement(CodingSessionSubagentHoverCardContent, {
      card: unreported,
    }),
  );
  try {
    const text = quiet.container.textContent;
    assert.match(text, /Stopped/);
    assert.match(text, /Modelnot reported/);
    assert.match(text, /Durationnever returned/);
    assert.match(text, /Tokensnot reported/);
    assert.doesNotMatch(text, /Result:/);
  } finally {
    await quiet.unmount();
  }
});

test("SV-98: a finished subagent's dot is the success colour everywhere", () => {
  // One mapping for the stream card, the hover card and the Agents panel.
  assert.match(codingSessionSubagentStatusDotClass("done"), /bg-emerald-500/);
  assert.match(codingSessionSubagentStatusDotClass("failed"), /bg-destructive/);
  // Only running pulses, and never under reduced motion.
  assert.match(
    codingSessionSubagentStatusDotClass("running"),
    /animate-pulse.*motion-reduce:animate-none/,
  );
  for (const status of ["done", "failed", "stopped", "unknown"]) {
    assert.doesNotMatch(codingSessionSubagentStatusDotClass(status), /pulse/);
  }
});
