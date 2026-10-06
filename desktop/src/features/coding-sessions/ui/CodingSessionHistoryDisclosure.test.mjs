import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionHistoryDisclosure,
  codingSessionHistoryDisclosureText,
} from "./CodingSessionHistoryDisclosure.tsx";

function completeness(state, reason = null, message = null) {
  return { state, loadedEarlierCount: 0, reason, message };
}

function render(value) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionHistoryDisclosure, {
      completeness: value,
    }),
  );
}

test("a complete or not-yet-answered history renders nothing", () => {
  assert.equal(render(completeness("complete")), "");
  assert.equal(render(completeness("pending")), "");
});

test("older pages still arriving read as loading, quietly", () => {
  const html = render(completeness("loading-earlier"));
  assert.match(html, /data-testid="coding-session-history-disclosure"/);
  assert.match(html, /data-state="loading-earlier"/);
  assert.match(html, /Loading earlier events…/);
});

test("a history that stopped short says so for every reason", () => {
  for (const reason of [
    "error",
    "page-budget",
    "crowded-second",
    "not-paged",
  ]) {
    const text = codingSessionHistoryDisclosureText(
      completeness("incomplete", reason),
    );
    assert.ok(text, `${reason} must be disclosed`);
    assert.doesNotMatch(text, /Loading/);
  }
  const html = render(completeness("incomplete", "error", "relay went away"));
  assert.match(html, /data-state="incomplete"/);
  assert.match(html, /could not be loaded/);
  assert.match(html, /title="relay went away"/);
});
