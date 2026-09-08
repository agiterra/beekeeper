import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { SeatRowButton } from "./SeatRowButton.tsx";

function seat(overrides = {}) {
  return {
    key: "channel-1:generation-1",
    channelId: "channel-1",
    generationId: "generation-1",
    label: "Build the card",
    agentName: null,
    agentPubkey: "a".repeat(64),
    projectId: "p1",
    projectName: "General",
    role: "builder",
    status: "idle",
    ageSeconds: 10,
    packSha: null,
    ...overrides,
  };
}

test("a row on a role card wraps rather than shortening every column to a stub", () => {
  const html = renderToStaticMarkup(
    React.createElement(SeatRowButton, {
      columns: "role-card",
      seat: seat(),
    }),
  );

  assert.match(html, /flex-wrap/);
  // The status and the version always render whole; only the two open-ended
  // columns are capped.
  assert.match(
    html,
    /class="whitespace-nowrap"[^>]*data-seat-column="status"[^>]*>idle \(just now\)</,
  );
  assert.match(html, /data-seat-column="sha"[^>]*>version unknown</);
  assert.match(html, /max-w-40 truncate[^>]*data-seat-column="agent"/);
  assert.match(html, /unmanaged agent/);
});

test("a row under a project block keeps its single truncating line", () => {
  const html = renderToStaticMarkup(
    React.createElement(SeatRowButton, {
      columns: "project-block",
      seat: seat({ agentName: "Builder" }),
    }),
  );

  assert.doesNotMatch(html, /flex-wrap/);
  assert.match(
    html,
    /class="truncate text-foreground"[^>]*data-seat-column="agent"/,
  );
  assert.match(html, /data-seat-column="role"[^>]*>builder</);
  // The project block never carries the version column.
  assert.doesNotMatch(html, /data-seat-column="sha"/);
});
