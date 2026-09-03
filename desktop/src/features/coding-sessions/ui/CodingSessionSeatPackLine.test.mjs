import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionSeatPackLine } from "./CodingSessionSeatPackLine.tsx";

const OWNER = "a".repeat(64);
const PACK_REF = {
  repo: `30617:${OWNER}:agiterra-packs`,
  sha: "b".repeat(40),
  role: "builder",
  path: "personas/roles/builder",
};

test("a staged pack is named by role and short sha", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatPackLine, { packRef: PACK_REF }),
  );
  assert.match(markup, /data-testid="coding-session-seat-pack"/);
  assert.match(markup, /pack builder@bbbbbbbb/);
  assert.match(
    markup,
    /title="pack builder@bbbbbbbb · personas\/roles\/builder"/,
  );
  assert.match(markup, /text-2xs/);
  assert.match(markup, /text-muted-foreground/);
});

test("no pack staged is disclosed, not blank — unlike the bee line", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatPackLine, { packRef: null }),
  );
  assert.match(markup, /no pack staged/);
  assert.match(markup, /title="no pack staged"/);
  assert.notEqual(markup, "");
});

test("the line never carries state in colour or a frozen pixel size", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatPackLine, { packRef: PACK_REF }),
  );
  assert.doesNotMatch(markup, /#[0-9a-fA-F]{3,8}\b/);
  assert.doesNotMatch(markup, /text-\[/);
});
