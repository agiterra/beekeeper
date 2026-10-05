import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionSurfaceTabStrip,
  codingSessionSurfaceTabKeyAction,
} from "./CodingSessionSurfaceTabStrip.tsx";

// SV-69: the surface tab strip follows the WAI-ARIA tabs pattern. The tablist
// owns only tabs; the tab is the one stop; close buttons are out of the tab
// order and Delete/Backspace on a focused tab closes it; the badge drawn in
// the close button's icon slot is part of the tab's accessible name.

const Icon = () => null;

function surface(id, label, Badge) {
  return {
    id,
    label,
    icon: Icon,
    shortcut: label[0],
    order: 0,
    placement: "right",
    lenses: ["conversation"],
    availability: () => ({ available: true }),
    Badge,
    Panel: () => null,
  };
}

const AgentsBadge = () =>
  React.createElement(
    "span",
    { "aria-label": "2 subagents running", role: "img" },
    "2",
  );

const TABS = [
  surface("agents", "Agents", AgentsBadge),
  surface("diff", "Diff", undefined),
];

function stripMarkup(overrides = {}) {
  const noop = () => {};
  return renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceTabStrip, {
      activeId: "diff",
      ctx: {},
      expanded: false,
      idBase: "strip",
      onActivate: noop,
      onClosePanel: noop,
      onCloseTab: noop,
      onCloseTabs: noop,
      onOpen: noop,
      onToggleExpanded: noop,
      showPanelControls: true,
      surfaces: TABS.map((definition) => ({
        definition,
        availability: { available: true },
      })),
      tabs: TABS,
      ...overrides,
    }),
  );
}

/** The opening tag of the element carrying this testid. */
function tagFor(markup, testId) {
  const match = markup.match(
    new RegExp(`<[a-z]+[^>]*data-testid="${testId}"[^>]*>`),
  );
  assert.ok(match, `no element with data-testid="${testId}"`);
  return match[0];
}

/** The text content of the element with this id (no nested same tag). */
function textOfId(markup, id) {
  const match = markup.match(
    new RegExp(`<([a-z]+)[^>]*\\bid="${id}"[^>]*>([\\s\\S]*?)</\\1>`),
  );
  assert.ok(match, `no element with id="${id}"`);
  return match[2];
}

test("the tablist holds only the tabs; the add button sits outside it", () => {
  const markup = stripMarkup();
  const tablist = markup.match(
    /<div[^>]*role="tablist"[^>]*>([\s\S]*?)<\/div><button[^>]*data-testid="coding-session-surface-add"/,
  );
  assert.ok(tablist, "the add button follows the tablist, not inside it");
  assert.match(tablist[0], /aria-label="Session surface tabs"/);
  assert.equal(
    markup.match(/role="tab"/g)?.length,
    2,
    "exactly one tab per opened surface",
  );
});

test("the active tab is the strip's only stop; close buttons are out of the tab order", () => {
  const markup = stripMarkup();
  assert.match(
    tagFor(markup, "coding-session-surface-tab-diff"),
    /tabindex="0"/,
  );
  assert.match(
    tagFor(markup, "coding-session-surface-tab-agents"),
    /tabindex="-1"/,
  );
  for (const id of ["agents", "diff"]) {
    const close = tagFor(markup, `coding-session-surface-tab-close-${id}`);
    assert.match(close, /tabindex="-1"/, `${id}'s close button is no stop`);
    assert.match(close, /type="button"/);
  }
  assert.match(markup, /aria-label="Close Agents"/);
  // With no active tab the first tab is the stop.
  const none = stripMarkup({ activeId: null });
  assert.match(
    tagFor(none, "coding-session-surface-tab-agents"),
    /tabindex="0"/,
  );
});

test("the tab's accessible name includes its badge", () => {
  const markup = stripMarkup();
  const agents = tagFor(markup, "coding-session-surface-tab-agents");
  assert.match(agents, /role="tab"/);
  assert.match(
    agents,
    /aria-labelledby="strip-tab-agents-label strip-tab-agents-badge"/,
  );
  assert.match(agents, /aria-keyshortcuts="Delete"/);
  assert.equal(textOfId(markup, "strip-tab-agents-label"), "Agents");
  // The badge slot named by the tab carries the badge's own label, so the
  // name reads "Agents 2 subagents running".
  assert.match(
    textOfId(markup, "strip-tab-agents-badge"),
    /aria-label="2 subagents running"/,
  );
  // A surface with no badge contributes nothing beyond its label.
  assert.doesNotMatch(textOfId(markup, "strip-tab-diff-badge"), /aria-label=/);
  // The panel the tab controls keeps the tab's id.
  const diff = tagFor(markup, "coding-session-surface-tab-diff");
  assert.match(diff, /\bid="strip-tab-diff"/);
  assert.match(diff, /aria-controls="strip-panel-diff"/);
});

test("arrows, Home and End rove across tabs from the focused tab", () => {
  assert.deepEqual(codingSessionSurfaceTabKeyAction("ArrowRight", 0, 3), {
    kind: "move",
    index: 1,
  });
  assert.deepEqual(codingSessionSurfaceTabKeyAction("ArrowRight", 2, 3), {
    kind: "move",
    index: 0,
  });
  assert.deepEqual(codingSessionSurfaceTabKeyAction("ArrowLeft", 0, 3), {
    kind: "move",
    index: 2,
  });
  assert.deepEqual(codingSessionSurfaceTabKeyAction("Home", 2, 3), {
    kind: "move",
    index: 0,
  });
  assert.deepEqual(codingSessionSurfaceTabKeyAction("End", 0, 3), {
    kind: "move",
    index: 2,
  });
  assert.equal(codingSessionSurfaceTabKeyAction("Tab", 0, 3), null);
  assert.equal(codingSessionSurfaceTabKeyAction("Enter", 0, 3), null);
  assert.equal(codingSessionSurfaceTabKeyAction("ArrowDown", 0, 3), null);
});

test("Delete and Backspace close the focused tab", () => {
  assert.deepEqual(codingSessionSurfaceTabKeyAction("Delete", 1, 3), {
    kind: "close",
    index: 1,
  });
  assert.deepEqual(codingSessionSurfaceTabKeyAction("Backspace", 0, 1), {
    kind: "close",
    index: 0,
  });
});

test("keys pressed off a tab do nothing", () => {
  // Focus on something that is not a tab (index -1) never moves or closes.
  assert.equal(codingSessionSurfaceTabKeyAction("ArrowRight", -1, 3), null);
  assert.equal(codingSessionSurfaceTabKeyAction("Delete", -1, 3), null);
  assert.equal(codingSessionSurfaceTabKeyAction("Delete", 3, 3), null);
  assert.equal(codingSessionSurfaceTabKeyAction("ArrowRight", 0, 0), null);
});
