/**
 * H-01b characterization — first tests for the builtin-shell feature (it had
 * none). Freezes the shared shell-session store law that the donor's
 * session-list behavior maps onto: a module-level external store shared by
 * every consumer, with `upsertShellSession` making a just-created session
 * visible immediately (replace-by-id, appended last) before any poll tick.
 * Rendering uses `renderToStaticMarkup`, so effects (polling, Tauri event
 * subscriptions) never start — exactly the store-law seam this test owns.
 */
import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { upsertShellSession, useShellSessions } from "./useShellSessions.ts";

function Probe() {
  const { sessions, loading } = useShellSessions();
  return React.createElement(
    "ul",
    { "data-loading": String(loading) },
    sessions.map((session) =>
      React.createElement("li", { key: session.sessionId }, session.sessionId),
    ),
  );
}

const render = () => renderToStaticMarkup(React.createElement(Probe));

const info = (sessionId, extra = {}) => ({
  sessionId,
  workspaceId: `shell:${sessionId}`,
  ...extra,
});

test("the store starts loading with no sessions and static renders never start polling", () => {
  const markup = render();
  assert.match(markup, /data-loading="true"/);
  assert.doesNotMatch(markup, /<li>/);
});

test("upsertShellSession makes a created session visible to every consumer immediately", () => {
  upsertShellSession(info("s-alpha"));
  const markup = render();
  assert.match(markup, /data-loading="false"/);
  assert.match(markup, /<li>s-alpha<\/li>/);
});

test("upsert replaces by session id and appends the fresh info last", () => {
  upsertShellSession(info("s-beta"));
  upsertShellSession(info("s-alpha", { resumed: true }));
  const markup = render();
  const alphaIndex = markup.indexOf("s-alpha");
  const betaIndex = markup.indexOf("s-beta");
  assert.ok(alphaIndex > betaIndex, "re-upserted session moves to the end");
  assert.equal(
    markup.match(/s-alpha/g)?.length,
    1,
    "no duplicate entry after re-upsert",
  );
});
