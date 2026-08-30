import assert from "node:assert/strict";
import { afterEach, mock, test } from "node:test";
import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionLiveActivityBar,
  formatLiveActivityElapsed,
} from "./CodingSessionLiveActivityBar.tsx";

const originalDocument = globalThis.document;
const originalWindow = globalThis.window;
const originalHTMLElement = globalThis.HTMLElement;
const originalActEnvironment = globalThis.IS_REACT_ACT_ENVIRONMENT;

afterEach(() => {
  mock.restoreAll();
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalHTMLElement === undefined) delete globalThis.HTMLElement;
  else globalThis.HTMLElement = originalHTMLElement;
  if (originalActEnvironment === undefined)
    delete globalThis.IS_REACT_ACT_ENVIRONMENT;
  else globalThis.IS_REACT_ACT_ENVIRONMENT = originalActEnvironment;
});

test("the live bar is one 44px row and says which counts are still in flight", () => {
  const nowMs = Date.now();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionLiveActivityBar, {
      followState: "paused",
      items: [
        {
          executionKey: "builder",
          label: "Bob · Builder",
          activity: "implementing tests",
          startedAtMs: nowMs - 198_000,
          openToolCount: 6,
          turnKey: "builder:turn-1",
        },
      ],
      onFocus() {},
      onFollow() {},
    }),
  );
  assert.match(markup, /class="[^"]*h-11/);
  assert.match(markup, /Bob · Builder · implementing tests/);
  assert.match(markup, /3m 18s/);
  assert.match(markup, /6 tools this turn/);
  assert.match(markup, /New activity ↓/);
  assert.doesNotMatch(markup, /Active work/);
});

test("missing telemetry omits clauses instead of printing zero", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionLiveActivityBar, {
      items: [
        {
          executionKey: "lead",
          label: "Helios · Lead",
          activity: null,
          startedAtMs: null,
          openToolCount: null,
          turnKey: "lead:status",
        },
      ],
      onFocus() {},
    }),
  );
  assert.match(markup, /Helios · Lead · working/);
  assert.doesNotMatch(markup, /0s|0 tools/);
  assert.doesNotMatch(markup, /Follow live/);
});

test("the live bar clock advances once per second while mounted", async () => {
  let nowMs = 2_000;
  let tick = null;
  const dom = new JSDOM(
    "<!doctype html><html><body><div id='root'></div></body></html>",
  );
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  mock.method(Date, "now", () => nowMs);
  mock.method(dom.window, "setInterval", (callback, intervalMs) => {
    assert.equal(intervalMs, 1_000);
    tick = callback;
    return 1;
  });
  mock.method(dom.window, "clearInterval", () => {});
  const root = createRoot(document.getElementById("root"));
  await act(async () => {
    root.render(
      React.createElement(CodingSessionLiveActivityBar, {
        items: [
          {
            executionKey: "builder",
            label: "Bob · Builder",
            activity: "testing",
            startedAtMs: 1_000,
            openToolCount: null,
            turnKey: "builder:turn-1",
          },
        ],
        onFocus() {},
      }),
    );
  });
  assert.match(document.body.textContent, /1\.0s/);
  nowMs = 3_000;
  await act(async () => tick());
  assert.match(document.body.textContent, /2\.0s/);
  await act(async () => root.unmount());
  dom.window.close();
});

test("elapsed formatting omits absent and future start times", () => {
  assert.equal(formatLiveActivityElapsed(null, 10_000), null);
  assert.equal(formatLiveActivityElapsed(10_000, 10_000), null);
  assert.equal(formatLiveActivityElapsed(9_000, 10_000), "1.0s");
});

test("no working seats means no Mission activity bar", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionLiveActivityBar, {
        items: [],
        onFocus() {},
      }),
    ),
    "",
  );
});
