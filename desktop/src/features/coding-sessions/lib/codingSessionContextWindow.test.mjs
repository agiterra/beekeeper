import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionContextWindow } from "./codingSessionContextWindow.ts";

function context(text) {
  return {
    id: `context-${text}`,
    type: "lifecycle",
    renderClass: "status",
    title: "Context Window Updated",
    text,
    timestamp: "2026-08-25T12:00:00.000Z",
  };
}

test("derives a bounded percentage from the newest signed context observation", () => {
  const usage = deriveCodingSessionContextWindow([
    context("usedTokens: 100\nmaxTokens: 1000"),
    context("usedTokens: 750\nmaxTokens: 1000"),
  ]);

  assert.deepEqual(usage, {
    usedTokens: 750,
    maxTokens: 1000,
    usedPercentage: 75,
  });
});

test("keeps an honest token count when the provider did not report a capacity", () => {
  assert.deepEqual(
    deriveCodingSessionContextWindow([context("inputTokens: 66546")]),
    {
      usedTokens: 66546,
      maxTokens: null,
      usedPercentage: null,
    },
  );
});

test("omits a decorative meter when no usable signed count exists", () => {
  assert.equal(
    deriveCodingSessionContextWindow([context("compactsAutomatically: true")]),
    null,
  );
  assert.equal(deriveCodingSessionContextWindow([]), null);
});
