import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionSurfaceBadgePill } from "./CodingSessionSurfaceBadgePill.tsx";

function render(badge) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceBadgePill, {
      badge,
      slot: "launcher",
      surfaceId: "agents",
    }),
  );
}

test("the pill states the fact, not just the number", () => {
  const html = render({
    tone: "activity",
    count: 2,
    facts: ["2 subagents running"],
    detail: null,
  });
  assert.match(html, /aria-label="2 subagents running"/);
  assert.match(html, /data-tone="activity"/);
  assert.match(html, /data-testid="coding-session-surface-badge-agents"/);
  assert.match(html, />2<\/span>/);
});

test("attention is a dot with no number; detail rides in the title", () => {
  const html = render({
    tone: "attention",
    count: null,
    facts: ["Gate failed: cargo test (observed)", "Gate running: just ci"],
    detail: "restart note",
  });
  assert.match(
    html,
    /aria-label="Gate failed: cargo test \(observed\)\. Gate running: just ci"/,
  );
  assert.match(html, /title="[^"]*restart note"/);
  assert.doesNotMatch(html, />\d+<\/span>/);
});

test("no badge draws nothing", () => {
  assert.equal(render(null), "");
});
