import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_BOUNDARY_TITLE,
  codingSessionBoundaryText,
  codingSessionSandboxFromTranscript,
} from "../lib/codingSessionBoundaryStatus.ts";
import {
  CodingSessionOtherSeatsSandboxBadge,
  CodingSessionSeatSandboxTag,
  codingSessionSeatSandboxWarning,
} from "./CodingSessionComposerSeatSandbox.tsx";

function boundary(id, status, reason) {
  return {
    id,
    type: "lifecycle",
    title: CODING_SESSION_BOUNDARY_TITLE,
    text: codingSessionBoundaryText(status, reason),
  };
}

const FULL = boundary("f", "execution_boundary_not_enforced", "full-access");
const NONE = boundary(
  "n",
  "execution_boundary_not_enforced",
  "no-backend-for-platform",
);
const SAFE = boundary("s", "execution_boundary_enforced", "macos-seatbelt");

function warning(items) {
  return codingSessionSeatSandboxWarning(
    codingSessionSandboxFromTranscript(items),
  );
}

test("a seat outside a boundary, now or earlier in its generation, is warned", () => {
  assert.equal(warning([FULL])?.label, "Full access");
  assert.equal(warning([NONE])?.label, "Not sandboxed");
  assert.equal(warning([FULL, SAFE])?.label, "Was full access");
  assert.equal(warning([NONE, SAFE])?.label, "Was not sandboxed");
});

test("a sandboxed or unreported seat carries no warning", () => {
  assert.equal(warning([SAFE]), null);
  assert.equal(warning([]), null);
});

test("the tag and the trigger badge wear the warning colour", () => {
  const tag = renderToStaticMarkup(
    React.createElement(CodingSessionSeatSandboxTag, {
      warning: warning([FULL]),
    }),
  );
  assert.match(tag, /data-testid="coding-session-seat-sandbox-warning"/);
  assert.match(tag, /amber/);
  assert.match(tag, /Full access/);

  const badge = renderToStaticMarkup(
    React.createElement(CodingSessionOtherSeatsSandboxBadge, { count: 2 }),
  );
  assert.match(badge, /2 other seats are not sandboxed/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionOtherSeatsSandboxBadge, { count: 0 }),
    ),
    "",
  );
});
