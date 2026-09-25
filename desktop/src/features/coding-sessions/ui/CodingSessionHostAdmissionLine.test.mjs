import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHostAdmissionLine } from "./CodingSessionHostAdmissionLine.tsx";

const render = (admission) =>
  renderToStaticMarkup(
    React.createElement(CodingSessionHostAdmissionLine, { admission }),
  );

test("a host that cannot read its project gets one honest line", () => {
  const html = render({ state: "unreadable", reason: "relay timed out" });
  assert.match(html, /data-testid="coding-session-host-admission"/);
  assert.match(html, /This computer cannot read this project: relay timed out/);
});

test("an admitted or public host renders nothing", () => {
  assert.equal(render({ state: "admitted", already: false }), "");
  assert.equal(render({ state: "public" }), "");
  assert.equal(render(null), "");
});
