import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionSurfaceHost,
  nextCodingSessionSurfaceTabIndex,
  reconcileCodingSessionSurfaceTab,
} from "./CodingSessionSurfaceHost.tsx";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail.tsx";
import {
  clampCodingSessionRailWidth,
  parsePersistedCodingSessionRailWidth,
} from "./useCodingSessionRailWidth.ts";

// ---------------------------------------------------------------------------
// Stale-tab reconciliation: closed or {tab}, never content with no tab.
// ---------------------------------------------------------------------------

test("closed stays closed regardless of available surfaces", () => {
  assert.equal(
    reconcileCodingSessionSurfaceTab({
      availableIds: ["agents", "changes"],
      lastTab: "changes",
      requestedTab: null,
    }),
    null,
  );
});

test("a still-offered requested tab is kept", () => {
  assert.equal(
    reconcileCodingSessionSurfaceTab({
      availableIds: ["agents", "changes"],
      lastTab: null,
      requestedTab: "changes",
    }),
    "changes",
  );
});

test("a vanished tab falls back to the remembered tab, then the first offered", () => {
  assert.equal(
    reconcileCodingSessionSurfaceTab({
      availableIds: ["agents", "changes"],
      lastTab: "changes",
      requestedTab: "terminal",
    }),
    "changes",
  );
  assert.equal(
    reconcileCodingSessionSurfaceTab({
      availableIds: ["agents", "changes"],
      lastTab: "browser",
      requestedTab: "terminal",
    }),
    "agents",
  );
});

test("no offered surfaces means closed, never content with no selected tab", () => {
  assert.equal(
    reconcileCodingSessionSurfaceTab({
      availableIds: [],
      lastTab: "changes",
      requestedTab: "changes",
    }),
    null,
  );
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
// Host chrome: tabs, panel, resizer semantics, close control, landmarks.
// ---------------------------------------------------------------------------

function hostMarkup(overrides = {}) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceHost, {
      activeSurfaceId: "agents",
      hostId: "host-under-test",
      layout: "inline",
      onClose() {},
      onSelectSurface() {},
      surfaces: [
        {
          id: "agents",
          label: "Agents",
          count: 2,
          content: React.createElement("p", null, "agents content"),
        },
        {
          id: "changes",
          label: "Observed changes",
          count: 0,
          content: React.createElement(CodingSessionChangesRail, {
            files: [],
          }),
        },
      ],
      widthContainerRef: { current: null },
      ...overrides,
    }),
  );
}

test("the inline host renders one tab strip, the active panel, and one close control", () => {
  const markup = hostMarkup();
  assert.match(markup, /role="tablist"/);
  assert.match(markup, /data-testid="coding-session-surface-tab-agents"/);
  assert.match(markup, /data-testid="coding-session-surface-tab-changes"/);
  assert.match(markup, />Agents</);
  assert.match(markup, />Observed changes</);
  // Only the active surface's content mounts.
  assert.match(markup, /agents content/);
  assert.doesNotMatch(markup, /No observed changes yet/);
  // Roving tabindex: active tab is the only tab stop.
  assert.match(
    markup,
    /data-testid="coding-session-surface-tab-agents"[^>]*tabindex="0"/,
  );
  assert.match(
    markup,
    /tabindex="-1"[^>]*data-testid="coding-session-surface-tab-changes"|data-testid="coding-session-surface-tab-changes"[^>]*tabindex="-1"/,
  );
  // Exactly one close control.
  assert.equal(
    markup.match(/data-testid="coding-session-surface-close"/g)?.length,
    1,
  );
  // Count badge renders for nonzero counts only.
  assert.match(markup, />2</);
});

test("switching the active surface swaps the mounted panel", () => {
  const markup = hostMarkup({ activeSurfaceId: "changes" });
  assert.match(markup, /No observed changes yet/);
  assert.doesNotMatch(markup, /agents content/);
  assert.match(
    markup,
    /data-testid="coding-session-surface-tab-changes"[^>]*aria-selected="true"|aria-selected="true"[^>]*data-testid="coding-session-surface-tab-changes"/,
  );
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

test("the host is the only aside landmark — surfaces contribute none", () => {
  const markup = hostMarkup({ activeSurfaceId: "changes" });
  assert.equal(markup.match(/<aside/g)?.length, 1);
  assert.match(markup, /aria-label="Session surfaces"/);
  assert.match(markup, /id="host-under-test"/);
});

test("an unmeasured layout renders nothing rather than a flashing inline panel", () => {
  assert.equal(hostMarkup({ layout: null }), "");
});
