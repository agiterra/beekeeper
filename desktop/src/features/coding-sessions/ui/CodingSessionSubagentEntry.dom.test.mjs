import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    CSS: dom.window.CSS ?? { escape: (value) => value },
    CustomEvent: dom.window.CustomEvent,
    document: dom.window.document,
    HTMLDetailsElement: dom.window.HTMLDetailsElement,
    window: dom.window,
  });
  if (!globalThis.CSS.escape) globalThis.CSS = { escape: (value) => value };
});

after(() => dom.window.close());

test("a spawn not in the DOM is handed to the umbrella timeline's reveal", async () => {
  const { revealCodingSessionSubagentInStream } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const { UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT } = await import(
    "./CodingSessionUmbrellaTimelineWindow.ts"
  );

  // Nobody handles the request: the caller keeps its own inline view.
  assert.equal(revealCodingSessionSubagentInStream("call-1"), false);

  const requested = [];
  const handler = (event) => {
    requested.push(event.detail.itemId);
    event.preventDefault();
  };
  document.addEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  try {
    assert.equal(revealCodingSessionSubagentInStream("call-1"), true);
    assert.deepEqual(requested, ["call-1"]);
  } finally {
    document.removeEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  }
});

test("a mounted spawn row is opened in place without asking the timeline", async () => {
  const { revealCodingSessionSubagentInStream } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const { UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT } = await import(
    "./CodingSessionUmbrellaTimelineWindow.ts"
  );
  document.body.innerHTML =
    '<div data-testid="coding-session-transcript"><details data-subagent-call-ids="call-0 call-2"><summary>x</summary></details></div>';
  const details = document.querySelector("details");
  details.scrollIntoView = () => {};
  let asked = false;
  const handler = () => {
    asked = true;
  };
  document.addEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
  try {
    assert.equal(revealCodingSessionSubagentInStream("call-2"), true);
    assert.equal(details.open, true);
    assert.equal(asked, false);
  } finally {
    document.removeEventListener(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, handler);
    document.body.innerHTML = "";
  }
});

function spawnEntry(count) {
  const spawns = Array.from({ length: count }, (_, index) => ({
    call: {
      id: `task-${index}`,
      type: "tool",
      renderClass: "subagent",
      title: "Task",
      toolName: "Task",
      beekeeperToolName: null,
      status: "completed",
      args: { description: `Review part ${index}` },
      result: `Report ${index}`,
      isError: false,
      timestamp: "2026-07-30T12:00:00.000Z",
    },
    status: "done",
    children: [],
  }));
  return {
    kind: "subagents",
    id: "subagents-1",
    label: count === 1 ? "Ran 1 subagent" : `Ran ${count} subagents`,
    spawns,
  };
}

async function mountEntry(props) {
  Object.assign(globalThis, {
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const React = await import("react");
  const { act } = React;
  const { createRoot } = await import("react-dom/client");
  const { CodingSessionSubagentEntry } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const render = async (next) =>
    act(async () => {
      root.render(
        React.createElement(CodingSessionSubagentEntry, {
          disclosureId: "subagents:subagents-1",
          renderChild: () => null,
          ...next,
        }),
      );
    });
  await render(props);
  return {
    act,
    container,
    render,
    unmount: async () => {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

test("SV-06: with an Agents surface the row opens it; the chevron alone expands in place", async () => {
  const opened = [];
  const toggled = [];
  const props = {
    entry: spawnEntry(1),
    onOpenAgentsSurface: () => opened.push("agents"),
    onOpenChange: (id, open) => toggled.push([id, open]),
    open: false,
  };
  const view = await mountEntry(props);
  try {
    const details = view.container.querySelector("details");
    assert.equal(details.dataset.opensSurface, "agents");
    const summary = details.querySelector("summary");
    // Attribution: the single spawn's description is on the row's tooltip.
    assert.equal(summary.getAttribute("title"), "Review part 0");

    await view.act(async () => {
      summary.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    });
    assert.deepEqual(opened, ["agents"]);
    assert.equal(details.open, false);

    const toggle = view.container.querySelector(
      '[data-testid="coding-session-subagents-inline-toggle"]',
    );
    assert.ok(toggle);
    await view.act(async () => {
      toggle.dispatchEvent(
        new dom.window.MouseEvent("click", { bubbles: true, cancelable: true }),
      );
    });
    assert.deepEqual(opened, ["agents"]);
    assert.deepEqual(toggled, [["subagents:subagents-1", true]]);

    // Opened, even one spawn is named — "Ran 1 subagent" says how many,
    // not which.
    // (No report, so no Markdown — which needs a router — is built here.)
    const quietEntry = spawnEntry(1);
    quietEntry.spawns[0].call.result = "";
    await view.render({ ...props, entry: quietEntry, open: true });
    const spawn = view.container.querySelector(
      '[data-testid="coding-session-subagent-spawn"]',
    );
    assert.match(spawn.textContent, /Review part 0/);
  } finally {
    await view.unmount();
  }
});

test("SV-06: without an Agents surface the row expands inline as before", async () => {
  const view = await mountEntry({
    entry: spawnEntry(2),
    onOpenChange: () => {},
    open: false,
  });
  try {
    const details = view.container.querySelector("details");
    assert.equal(details.dataset.opensSurface, undefined);
    assert.equal(
      view.container.querySelector(
        '[data-testid="coding-session-subagents-inline-toggle"]',
      ),
      null,
    );
    assert.equal(
      details.querySelector("summary").getAttribute("title"),
      "Review part 0\nReview part 1",
    );
  } finally {
    await view.unmount();
  }
});

test("SV-06: a live batch says how many are still working; a done one only describes it", async () => {
  const live = spawnEntry(3);
  live.label = "Kicked off 3 subagents";
  live.spawns[0].status = "running";
  live.spawns[1].status = "running";
  live.spawns[2].status = "failed";
  const view = await mountEntry({
    entry: live,
    onOpenChange: () => {},
    open: false,
  });
  try {
    const summary = view.container.querySelector("summary");
    assert.equal(
      summary.getAttribute("aria-description"),
      "2 working · 1 failed",
    );
    const counts = view.container.querySelector(
      '[data-testid="coding-session-subagents-status-summary"]',
    );
    assert.ok(counts);
    assert.equal(counts.textContent, "· 2 working · 1 failed");

    const done = spawnEntry(3);
    done.label = "Kicked off 3 subagents";
    await view.render({ entry: done, onOpenChange: () => {}, open: false });
    assert.equal(
      view.container.querySelector("summary").getAttribute("aria-description"),
      "3 done",
    );
    assert.equal(
      view.container.querySelector(
        '[data-testid="coding-session-subagents-status-summary"]',
      ),
      null,
    );
  } finally {
    await view.unmount();
  }
});

async function mountInTurn(settlement, entry) {
  Object.assign(globalThis, {
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const React = await import("react");
  const { act } = React;
  const { createRoot } = await import("react-dom/client");
  const { CodingSessionSubagentEntry } = await import(
    "./CodingSessionSubagentEntry.tsx"
  );
  const { CodingSessionTurnSettlementContext } = await import(
    "./CodingSessionTranscriptItem.tsx"
  );
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      React.createElement(
        CodingSessionTurnSettlementContext.Provider,
        { value: settlement },
        React.createElement(CodingSessionSubagentEntry, {
          disclosureId: "subagents:subagents-1",
          entry,
          onOpenChange: () => {},
          open: false,
          renderChild: () => null,
        }),
      ),
    );
  });
  return {
    container,
    unmount: async () => {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

function openSpawnEntry() {
  const entry = spawnEntry(1);
  entry.label = "Kicked off 1 subagent";
  entry.spawns[0].status = "running";
  entry.spawns[0].call.status = "in_progress";
  entry.spawns[0].call.result = "";
  return entry;
}

test("a spawn that never answered reads through its turn's settlement, never a spinner over a settled turn", async () => {
  const cases = [
    ["live", "running", "1 working", "Kicked off 1 subagent", true],
    ["settled", "stopped", "1 stopped", "Ran 1 subagent", false],
    ["unknown", "unknown", "1 status unknown", "Ran 1 subagent", false],
  ];
  for (const [settlement, status, words, label, spins] of cases) {
    const view = await mountInTurn(settlement, openSpawnEntry());
    try {
      const details = view.container.querySelector("details");
      assert.equal(details.dataset.status, status, settlement);
      const summary = details.querySelector("summary");
      assert.equal(summary.getAttribute("aria-description"), words);
      assert.equal(
        view.container.querySelector(
          '[data-testid="coding-session-subagents-status-summary"]',
        ).textContent,
        `· ${words}`,
      );
      assert.equal(summary.getAttribute("aria-label"), label);
      assert.equal(
        view.container.querySelector(".animate-spin") !== null,
        spins,
        settlement,
      );
      assert.equal(
        view.container.querySelector('[aria-label="Running"]') !== null,
        spins,
      );
      if (settlement === "unknown") {
        assert.ok(
          view.container.querySelector('[aria-label="Status unknown"]'),
        );
      }
    } finally {
      await view.unmount();
    }
  }
});
