import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionComposerSandboxChip,
  CodingSessionSandboxModeRows,
} from "./CodingSessionComposerSandboxChip.tsx";

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

function modes(props) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionSandboxModeRows, {
      granted: null,
      pending: null,
      state: "sandboxed",
      ...props,
    }),
  );
}

/** [mode, checked, is a control] per row, in order. */
function rows(markup) {
  return [...markup.matchAll(/<li([^>]*)>(.*?)<\/li>/g)].map(
    ([, attrs, body]) => [
      attrs.match(/data-mode="([^"]+)"/)?.[1],
      /aria-current="true"/.test(attrs),
      /<button/.test(body),
    ],
  );
}

test("SV-17: the menu lists both modes with descriptions and checks the one in force", () => {
  const markup = modes({});
  assert.deepEqual(rows(markup), [
    ["sandboxed", true, false],
    ["full-access", false, false],
  ]);
  assert.match(markup, /Commands and edits stay inside this project/);
  assert.match(markup, /Commands and edits can reach anything/);
  assert.equal((markup.match(/lucide-check/g) ?? []).length, 1);
});

test("SV-17: on the agent's own computer only the mode the grant flips to is a control", () => {
  const toggle = () => {};
  assert.deepEqual(rows(modes({ granted: false, toggle })), [
    ["sandboxed", true, false],
    ["full-access", false, true],
  ]);
  assert.deepEqual(
    rows(modes({ granted: true, state: "full-access", toggle })),
    [
      ["sandboxed", false, true],
      ["full-access", true, false],
    ],
  );
  // A change in flight disables the control.
  assert.match(
    modes({ granted: false, pending: true, toggle }),
    /sandbox-full-access-toggle"[^>]*disabled/,
  );
});

test("SV-17: an unenforced or unreported boundary is checked as itself, never as a mode", () => {
  for (const state of ["not-sandboxed", "unreported"]) {
    const result = rows(modes({ state }));
    assert.deepEqual(result, [
      [state, true, false],
      ["sandboxed", false, false],
      ["full-access", false, false],
    ]);
  }
});

test("SV-17: a grant the running agent has not restarted under is pending, never checked", () => {
  // Turned on, the restart failed, the restore failed: the grant is on disk
  // but the host still reports the running agent sandboxed.
  const toggle = () => {};
  const markup = modes({ granted: true, state: "sandboxed", toggle });
  assert.deepEqual(rows(markup), [
    ["sandboxed", true, true],
    ["full-access", false, false],
  ]);
  assert.equal((markup.match(/lucide-check/g) ?? []).length, 1);
  assert.match(
    markup,
    /data-mode="full-access" data-next-start="true"[\s\S]*Granted · applies when the agent next starts/,
  );
  assert.doesNotMatch(markup, /data-mode="sandboxed" data-next-start/);
  // The same grant under a boundary the host did not enforce checks that
  // boundary and marks full access pending.
  for (const state of ["not-sandboxed", "unreported"]) {
    const result = modes({ granted: true, state });
    assert.deepEqual(rows(result), [
      [state, true, false],
      ["sandboxed", false, false],
      ["full-access", false, false],
    ]);
    assert.match(result, /data-mode="full-access" data-next-start="true"/);
  }
  // A revoked grant under a running full-access agent: full access stays
  // checked until the restart, and Sandboxed is the pending choice.
  const revoked = modes({ granted: false, state: "full-access" });
  assert.deepEqual(rows(revoked), [
    ["sandboxed", false, false],
    ["full-access", true, false],
  ]);
  assert.match(
    revoked,
    /data-mode="sandboxed" data-next-start="true"[\s\S]*Full access revoked · applies when the agent next starts/,
  );
  // Agreement: no pending mark when the grant matches the running boundary.
  assert.doesNotMatch(
    modes({ granted: true, state: "full-access" }),
    /data-next-start/,
  );
});
