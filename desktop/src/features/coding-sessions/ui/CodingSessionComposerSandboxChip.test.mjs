import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionComposerSandboxChip } from "./CodingSessionComposerSandboxChip.tsx";

function chip(report, local = null) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionComposerSandboxChip, {
      sandbox: { report, local },
    }),
  );
}

test("a sandboxed session reads as one quiet chip with a dropdown caret", () => {
  const markup = chip({ state: "sandboxed", boundaryText: "x", isolation: [] });
  assert.match(markup, /data-testid="coding-session-control-sandbox"/);
  assert.match(markup, /data-tone="safe"/);
  assert.match(markup, />Sandboxed</);
  assert.match(markup, /lucide-chevron-down/);
  assert.match(markup, /aria-label="Sandbox: Sandboxed. Show details"/);
});

test("full access is warned on the chip itself, not only inside the dropdown", () => {
  const markup = chip({
    state: "full-access",
    boundaryText: "Sandbox off — x",
    isolation: [],
  });
  assert.match(markup, /data-tone="warning"/);
  assert.match(markup, />Full access</);
  assert.match(markup, /amber/);
  assert.doesNotMatch(markup, />Sandboxed</);
});

test("a local grant pending restart still reads as full access", () => {
  const markup = chip(
    { state: "sandboxed", boundaryText: "x", isolation: [] },
    { granted: true, pending: null, error: null, toggle: () => {} },
  );
  assert.match(markup, />Full access</);
  assert.match(markup, /data-tone="warning"/);
});

test("no boundary report is said plainly", () => {
  const markup = chip({
    state: "unreported",
    boundaryText: null,
    isolation: [],
  });
  assert.match(markup, />Sandbox unreported</);
  assert.match(markup, /data-tone="muted"/);
});

test("a read-only chip keeps its warning on the chip itself", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposerSandboxChip, {
      readOnlyNote: "Closed.",
      sandbox: {
        report: { state: "full-access", boundaryText: "x", isolation: [] },
        local: { granted: true, pending: null, error: null, toggle: () => {} },
      },
    }),
  );
  assert.match(markup, /data-tone="warning"/);
  assert.match(markup, />Full access</);
});

test("the popover lists every earlier boundary period, full access in the warning colour", () => {
  const markup = chip({
    state: "sandboxed",
    boundaryText: "Enforced — x",
    isolation: [],
    earlier: [
      {
        id: "a",
        timestamp: "2026-10-04T09:00:00.000Z",
        state: "full-access",
        boundaryText: "Sandbox off — earlier",
        isolation: [],
      },
      {
        id: "b",
        timestamp: "",
        state: "sandboxed",
        boundaryText: "Enforced — older",
        isolation: ["Git credentials withheld"],
      },
    ],
  });
  assert.match(markup, />Sandboxed · was full access</);
  assert.match(markup, /data-tone="warning"/);
  // Radix renders popover content only while open, so the history rows are
  // checked on the section itself below.
});

test("the history section renders each period with its state and text", async () => {
  const { CodingSessionSandboxHistory } = await import(
    "./CodingSessionComposerSandboxChip.tsx"
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSandboxHistory, {
      earlier: [
        {
          id: "a",
          timestamp: "2026-10-04T09:00:00.000Z",
          state: "full-access",
          boundaryText: "Sandbox off — earlier",
          isolation: [],
        },
        {
          id: "b",
          timestamp: "",
          state: "sandboxed",
          boundaryText: "Enforced — older",
          isolation: ["Git credentials withheld"],
        },
      ],
    }),
  );
  assert.match(markup, /data-testid="coding-session-sandbox-history"/);
  const rows = markup.match(
    /data-testid="coding-session-sandbox-history-row"/g,
  );
  assert.equal(rows?.length, 2);
  assert.match(
    markup,
    /<li class="[^"]*amber[^"]*"[^>]*data-state="full-access"/,
  );
  assert.doesNotMatch(
    markup,
    /<li class="[^"]*amber[^"]*"[^>]*data-state="sandboxed"/,
  );
  assert.match(markup, /Sandbox off — earlier/);
  assert.match(markup, /Git credentials withheld/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionSandboxHistory, { earlier: [] }),
    ),
    "",
  );
});
