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
    IS_REACT_ACT_ENVIRONMENT: true,
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

const ALICE = "a1".repeat(32);
const BUILDER =
  "cc00000000000000000000000000000000000000000000000000000000000022";
const REFUTER =
  "dd00000000000000000000000000000000000000000000000000000000000033";
const NOW = 1_785_513_037;

function entry(overrides = {}) {
  return {
    eventId: "e1".padStart(64, "0"),
    pubkey: ALICE,
    createdAt: NOW - 600,
    type: "milestone",
    text: "Lane B landed.",
    claimedAreas: [],
    branch: null,
    sessionRef: null,
    supersedes: null,
    supersededBy: [],
    active: true,
    ...overrides,
  };
}

async function renderRow(value) {
  const { render } = await import("@testing-library/react");
  const React = await import("react");
  const { PulseEntryRow } = await import(
    "@/features/project-pulse/ui/PulseEntryRow"
  );
  return render(
    React.createElement(
      "ul",
      null,
      React.createElement(PulseEntryRow, { entry: value, nowSeconds: NOW }),
    ),
  );
}

test("the cost summary compacts tokens and counts seats", async () => {
  const { formatPulseCostSummary } = await import(
    "@/features/project-pulse/ui/PulseEntryRow"
  );
  assert.equal(
    formatPulseCostSummary({
      seats: [
        { actor: BUILDER, turns: 1 },
        { actor: REFUTER, turns: 1 },
      ],
      totalTokens: 193_400,
    }),
    "Σ 193k tok · 2 seats",
  );
  assert.equal(
    formatPulseCostSummary({
      seats: [{ actor: BUILDER, turns: 1 }],
      totalTokens: null,
    }),
    "1 seat",
  );
  assert.equal(
    formatPulseCostSummary({ seats: [], totalTokens: 940 }),
    "Σ 940 tok",
  );
  assert.equal(
    formatPulseCostSummary({ seats: [], totalTokens: 2_400_000 }),
    "Σ 2.4M tok",
  );
  assert.equal(formatPulseCostSummary(null), null);
  assert.equal(formatPulseCostSummary({ seats: [], totalTokens: null }), null);
});

test("an entry with a cost renders it as a suffix line", async () => {
  const screen = await renderRow(
    entry({
      cost: {
        seats: [
          { actor: BUILDER, role: "builder", turns: 2 },
          { actor: REFUTER, role: "refuter", turns: 1 },
        ],
        totalTokens: 193_400,
      },
    }),
  );
  const line = screen.getByTestId("pulse-entry-cost");
  assert.equal(line.textContent, "Σ 193k tok · 2 seats");
});

test("an entry with no cost renders no cost line at all", async () => {
  const screen = await renderRow(entry());
  assert.equal(screen.queryByTestId("pulse-entry-cost"), null);
});
