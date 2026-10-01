import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionFullAccessBadge } from "./CodingSessionFullAccessBadge.tsx";
import { CodingSessionHeaderOverflow } from "./CodingSessionHeaderOverflow.tsx";

function view(granted, extra = {}) {
  return {
    granted,
    pending: null,
    error: null,
    toggle() {},
    watcher: null,
    ...extra,
  };
}

const overflowBase = { stopAllLabel: "unused", stopAllSentence: "unused" };

test("the badge shows only when the host answered granted", () => {
  const on = renderToStaticMarkup(
    React.createElement(CodingSessionFullAccessBadge, {
      fullAccess: view(true),
    }),
  );
  assert.match(on, /data-testid="coding-session-header-full-access"/);
  assert.match(on, />Full access</);
  const off = renderToStaticMarkup(
    React.createElement(CodingSessionFullAccessBadge, {
      fullAccess: view(false),
    }),
  );
  assert.equal(off, "");
});

test("no read — foreign provider, failed or pending — renders nothing", () => {
  for (const fullAccess of [null, undefined]) {
    assert.equal(
      renderToStaticMarkup(
        React.createElement(CodingSessionFullAccessBadge, { fullAccess }),
      ),
      "",
    );
  }
});

test("the badge mounts the restart watcher whatever the state", () => {
  const watcher = React.createElement("i", { "data-watcher": "1" });
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionFullAccessBadge, {
      fullAccess: view(false, { watcher }),
    }),
  );
  assert.match(markup, /data-watcher="1"/);
});

test("a local session's grant alone is enough to offer the ⋯ menu", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionHeaderOverflow, {
      ...overflowBase,
      fullAccess: view(false),
    }),
  );
  assert.match(markup, /data-testid="coding-session-overflow"/);
  const none = renderToStaticMarkup(
    React.createElement(CodingSessionHeaderOverflow, {
      ...overflowBase,
      fullAccess: null,
    }),
  );
  assert.equal(none, "");
});
