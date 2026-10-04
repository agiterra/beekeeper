import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionWorkspaceSandboxFooter } from "./CodingSessionWorkspaceSandboxFooter.tsx";

function footer(report, local, sessionClosed) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionWorkspaceSandboxFooter, {
      sandbox: { report, local },
      sessionClosed,
    }),
  );
}

test("a closed session still shows full access on the chip (SV-17)", () => {
  const markup = footer(
    { state: "full-access", boundaryText: "Sandbox off — x", isolation: [] },
    null,
    true,
  );
  assert.match(markup, /data-testid="coding-session-sandbox-footer"/);
  assert.match(markup, /data-testid="coding-session-control-sandbox"/);
  assert.match(markup, />Full access</);
  assert.match(markup, /data-tone="warning"/);
  assert.match(markup, />Session closed</);
});

test("a session with no command target still shows an unenforced boundary", () => {
  const markup = footer(
    { state: "not-sandboxed", boundaryText: "x", isolation: [] },
    null,
    false,
  );
  assert.match(markup, />Not sandboxed</);
  assert.match(markup, /data-tone="warning"/);
  assert.match(markup, />No command target published</);
});

test("a local grant still warns in the read-only footer", () => {
  const markup = footer(
    { state: "sandboxed", boundaryText: "x", isolation: [] },
    { granted: true, pending: null, error: null, toggle: () => {} },
    true,
  );
  assert.match(markup, />Full access</);
  assert.match(markup, /data-tone="warning"/);
});
