import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";

// SV-104: under prefers-reduced-motion nothing live shimmers — not even a
// fresh running tool — and the waiting strip's dot holds still. Its own file:
// motion caches the media query for the process.

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
});
dom.window.matchMedia = (query) => ({
  // motion asks "(prefers-reduced-motion)"; a stylesheet asks "…: reduce".
  matches: query.includes("prefers-reduced-motion"),
  media: query,
  onchange: null,
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent() {
    return false;
  },
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
const { CodingSessionActiveTool } = await import(
  "./CodingSessionTranscriptParts.tsx"
);
const { CodingSessionLastTranscriptEventContext } = await import(
  "./CodingSessionTranscriptWorking.tsx"
);

test("reduced motion: a fresh running tool's label is plain text", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const now = new Date().toISOString();
  await act(async () => {
    root.render(
      React.createElement(
        CodingSessionLastTranscriptEventContext.Provider,
        { value: Date.now() },
        React.createElement(CodingSessionActiveTool, {
          disclosureId: "item:tool-1",
          item: {
            id: "tool-1",
            type: "tool",
            renderClass: "shell",
            descriptor: {
              renderClass: "shell",
              label: "Ran command",
              preview: "rm x",
              action: { verb: "Ran", object: "rm x" },
            },
            title: "Bash",
            toolName: "Bash",
            beekeeperToolName: null,
            status: "executing",
            args: {},
            result: "",
            isError: false,
            timestamp: now,
            startedAt: now,
            turnId: "turn-1",
          },
          onOpenChange: () => {},
          open: false,
          settlement: "live",
        }),
      ),
    );
  });
  assert.match(container.textContent ?? "", /Run rm x/);
  // A boolean, not the element: asserting on a JSDOM node makes a failure
  // message walk the whole window.
  assert.equal(
    container.querySelector(
      '[data-testid="coding-session-live-shimmer-overlay"]',
    ) === null,
    true,
  );
  await act(async () => root.unmount());
  container.remove();
});
