/**
 * SV-45: `useCodingSessionExecutionSandboxes` keeps its cache from committed
 * renders only. A render React throws away must not become the next render's
 * cache.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React, { act } from "react";

import {
  CODING_SESSION_BOUNDARY_TITLE,
  codingSessionBoundaryText,
} from "../lib/codingSessionBoundaryStatus.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
});

after(() => dom.window.close());

function boundary(id, status, reason) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_BOUNDARY_TITLE,
    text: codingSessionBoundaryText(status, reason),
  };
}

function seat(executionKey, active) {
  return {
    kind: "execution",
    executionKey,
    label: executionKey,
    execution: {
      executionKey,
      signerPubkey: "a".repeat(64),
      activeGeneration: { transcript: active },
      priorGenerations: [],
      operatorPubkey: null,
    },
  };
}

test("SV-45: a discarded render does not replace the committed cache", async () => {
  const { createRoot } = await import("react-dom/client");
  const { useCodingSessionExecutionSandboxes } = await import(
    "./codingSessionUmbrellaExecutionSandbox.ts"
  );
  const seen = [];
  const never = new Promise(() => {});
  let setInput = null;
  function Probe({ initial }) {
    const [input, set] = React.useState(initial);
    setInput = set;
    const reports = useCodingSessionExecutionSandboxes(input.participants);
    // Suspending inside a transition: React keeps the committed screen and
    // throws this render away.
    if (input.suspend) throw never;
    seen.push(reports);
    return null;
  }
  const transcript = [
    boundary("a", "execution_boundary_enforced", "macos-seatbelt"),
  ];
  const container = dom.window.document.createElement("div");
  const root = createRoot(container);
  try {
    await act(async () => {
      root.render(
        React.createElement(
          React.Suspense,
          { fallback: null },
          React.createElement(Probe, {
            initial: { participants: [seat("A", transcript)], suspend: false },
          }),
        ),
      );
    });
    const firstReports = seen.at(-1);
    assert.ok(firstReports);
    // Another seat set, rendered in a transition that suspends: it never
    // commits.
    await act(async () => {
      React.startTransition(() => {
        setInput({
          participants: [
            seat("B", [
              boundary("b", "execution_boundary_not_enforced", "full-access"),
            ]),
          ],
          suspend: true,
        });
      });
    });
    assert.equal(seen.at(-1), firstReports, "the transition committed");
    // The committed seat again, as new participant objects over the same
    // transcript: the cache is the committed one, so the map keeps its
    // reference. A cache written during the discarded render would hold only
    // seat B and hand back a new map.
    await act(async () => {
      setInput({ participants: [seat("A", transcript)], suspend: false });
    });
    assert.equal(seen.at(-1), firstReports);
    assert.equal(seen.at(-1).get("A").state, "sandboxed");
  } finally {
    await act(async () => root.unmount());
  }
});
