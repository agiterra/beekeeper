import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionSurfaceHost,
  nextCodingSessionSurfaceTabIndex,
} from "./CodingSessionSurfaceHost.tsx";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail.tsx";
import { codingSessionSurfaceTabsToClose } from "./CodingSessionSurfaceTabStrip.tsx";
import {
  clampCodingSessionRailWidth,
  parsePersistedCodingSessionRailWidth,
} from "./useCodingSessionRailWidth.ts";

// ---------------------------------------------------------------------------
// The tab context menu (T3's close / others / right / all).
// ---------------------------------------------------------------------------

test("the tab context menu closes this, the others, those to the right, or all", () => {
  const ids = ["diff", "agents", "files"];
  assert.deepEqual(codingSessionSurfaceTabsToClose(ids, "agents", "close"), [
    "agents",
  ]);
  assert.deepEqual(
    codingSessionSurfaceTabsToClose(ids, "agents", "close-others"),
    ["diff", "files"],
  );
  assert.deepEqual(
    codingSessionSurfaceTabsToClose(ids, "agents", "close-right"),
    ["files"],
  );
  assert.deepEqual(
    codingSessionSurfaceTabsToClose(ids, "files", "close-right"),
    [],
    "nothing to the right of the last tab: the item is disabled",
  );
  assert.deepEqual(
    codingSessionSurfaceTabsToClose(["diff"], "diff", "close-others"),
    [],
  );
  assert.deepEqual(
    codingSessionSurfaceTabsToClose(ids, "diff", "close-all"),
    ids,
  );
  assert.deepEqual(codingSessionSurfaceTabsToClose(ids, "nope", "close"), []);
});

// ---------------------------------------------------------------------------
// Roving-tabindex keyboard order.
// ---------------------------------------------------------------------------

test("arrow keys cycle, Home/End jump, other keys are ignored", () => {
  assert.equal(nextCodingSessionSurfaceTabIndex("ArrowRight", 0, 2), 1);
  assert.equal(nextCodingSessionSurfaceTabIndex("ArrowRight", 1, 2), 0);
  assert.equal(nextCodingSessionSurfaceTabIndex("ArrowLeft", 0, 2), 1);
  assert.equal(nextCodingSessionSurfaceTabIndex("Home", 2, 3), 0);
  assert.equal(nextCodingSessionSurfaceTabIndex("End", 0, 3), 2);
  assert.equal(nextCodingSessionSurfaceTabIndex("Tab", 0, 3), null);
  assert.equal(nextCodingSessionSurfaceTabIndex("ArrowDown", 0, 3), null);
  assert.equal(nextCodingSessionSurfaceTabIndex("ArrowRight", 0, 0), null);
});

// ---------------------------------------------------------------------------
// Width safety: strict persisted parsing and container clamping.
// ---------------------------------------------------------------------------

test("persisted widths parse only as plain in-range integers", () => {
  assert.equal(parsePersistedCodingSessionRailWidth("360"), 360);
  assert.equal(parsePersistedCodingSessionRailWidth("288"), 288);
  assert.equal(parsePersistedCodingSessionRailWidth("720"), 720);
  // Corrupt values are rejected strictly, not best-effort coerced.
  assert.equal(parsePersistedCodingSessionRailWidth(null), null);
  assert.equal(parsePersistedCodingSessionRailWidth(""), null);
  assert.equal(parsePersistedCodingSessionRailWidth("abc"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("360px"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("360.5"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("-360"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("1e3"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("99999"), null);
  // In-range digits only — outside the legal band is corrupt too.
  assert.equal(parsePersistedCodingSessionRailWidth("100"), null);
  assert.equal(parsePersistedCodingSessionRailWidth("9000"), null);
});

test("widths clamp to the container's bounds", () => {
  // A huge width is bounded by the container minus the narrative minimum.
  assert.equal(clampCodingSessionRailWidth(5000, 1000), 580);
  // A tiny width is raised to the minimum.
  assert.equal(clampCodingSessionRailWidth(10, 1000), 288);
  // A tiny container still yields the minimum, never a negative width.
  assert.equal(clampCodingSessionRailWidth(400, 300), 288);
});

// ---------------------------------------------------------------------------
// Host chrome: tabs, launcher, panel, resizer semantics, controls, landmarks.
// ---------------------------------------------------------------------------

const Icon = () => null;

function definition(id, label, shortcut, Panel) {
  return {
    definition: {
      id,
      label,
      icon: Icon,
      shortcut,
      order: 0,
      placement: "right",
      lenses: ["conversation"],
      availability: () => ({ available: true }),
      Panel,
    },
    availability: { available: true },
  };
}

const SURFACES = [
  definition("agents", "Agents", "A", () =>
    React.createElement("p", null, "agents content"),
  ),
  definition("diff", "Diff", "D", () =>
    React.createElement(CodingSessionChangesRail, { files: [] }),
  ),
];

function hostMarkup({ state = {}, ...overrides } = {}) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceHost, {
      ctx: {},
      hostId: "host-under-test",
      layout: "inline",
      panels: {
        state: {
          rightOpen: true,
          tabs: ["agents", "diff"],
          active: "agents",
          expanded: false,
          bottomOpen: false,
          ...state,
        },
        actions: {},
      },
      surfaces: SURFACES,
      widthContainerRef: { current: null },
      ...overrides,
    }),
  );
}

test("the inline host renders the open tabs, the active panel and its controls", () => {
  const markup = hostMarkup();
  assert.match(markup, /role="tablist"/);
  assert.match(markup, /aria-label="Session surface tabs"/);
  assert.match(markup, /data-testid="coding-session-surface-tab-agents"/);
  assert.match(markup, /data-testid="coding-session-surface-tab-diff"/);
  // Only the active surface's panel mounts; no launcher beside it.
  assert.match(markup, /agents content/);
  assert.doesNotMatch(markup, /No observed changes yet/);
  assert.doesNotMatch(markup, /Open a surface/);
  // Roving tabindex: the active tab is the only tab stop.
  assert.match(
    markup,
    /data-testid="coding-session-surface-tab-agents"[^>]*tabindex="0"|tabindex="0"[^>]*data-testid="coding-session-surface-tab-agents"/,
  );
  // Each tab closes on its own; "+" adds; expand and close sit top right.
  assert.match(markup, /data-testid="coding-session-surface-tab-close-diff"/);
  assert.match(markup, /aria-label="Close Diff"/);
  // The close control sits in the icon slot, before the tab's label, and
  // its X shows only on hover or focus (T3's PanelTabCloseButton).
  assert.match(
    markup,
    /data-testid="coding-session-surface-tab-close-diff"[\s\S]*?group-hover\/tab:block[\s\S]*?data-testid="coding-session-surface-tab-diff"/,
  );
  assert.doesNotMatch(markup, /opacity-60/);
  assert.match(markup, /data-testid="coding-session-surface-add"/);
  assert.match(markup, /data-testid="coding-session-surface-expand"/);
  assert.equal(
    markup.match(/data-testid="coding-session-surface-close"/g)?.length,
    1,
  );
});

test("switching the active surface swaps the mounted panel", () => {
  const markup = hostMarkup({ state: { active: "diff" } });
  assert.match(markup, /No observed changes yet/);
  assert.doesNotMatch(markup, /agents content/);
  assert.match(
    markup,
    /data-testid="coding-session-surface-tab-diff"[^>]*aria-selected="true"|aria-selected="true"[^>]*data-testid="coding-session-surface-tab-diff"/,
  );
});

test("an open panel with no active tab shows the launcher", () => {
  const markup = hostMarkup({ state: { tabs: [], active: null } });
  assert.match(markup, /data-testid="coding-session-surface-launcher"/);
  assert.match(markup, />Open a surface</);
  assert.match(
    markup,
    /data-testid="coding-session-surface-launcher-row-agents"/,
  );
  // No tabs means no "+" (T3 shows it only beside tabs).
  assert.doesNotMatch(markup, /data-testid="coding-session-surface-add"/);
});

test("the resizer is a vertical separator with honest value semantics", () => {
  const markup = hostMarkup();
  const resizer = markup.match(
    /<button[^>]*data-testid="coding-session-surface-resize"[^>]*>/u,
  );
  assert.ok(resizer, "resizer must render in inline layout");
  assert.match(resizer[0], /role="separator"/);
  assert.match(resizer[0], /aria-orientation="vertical"/);
  assert.match(resizer[0], /aria-valuemin="288"/);
  assert.match(resizer[0], /aria-valuemax="720"/);
  assert.match(resizer[0], /aria-valuenow="\d+"/);
});

test("an expanded panel fills the body and drops the resizer", () => {
  const markup = hostMarkup({ state: { expanded: true } });
  assert.match(markup, /data-expanded="true"/);
  assert.doesNotMatch(markup, /coding-session-surface-resize/);
  assert.match(markup, /aria-label="Restore panel size"/);
});

test("the host is the only aside landmark — surfaces contribute none", () => {
  const markup = hostMarkup({ state: { active: "diff" } });
  assert.equal(markup.match(/<aside/g)?.length, 1);
  assert.match(markup, /aria-label="Session surfaces"/);
  assert.match(markup, /id="host-under-test"/);
});

test("an unmeasured layout renders nothing rather than a flashing inline panel", () => {
  assert.equal(hostMarkup({ layout: null }), "");
});
