import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionModelSwitchedRow } from "./codingSessionModelSwitchedStatus.ts";
import { buildBaseTranscriptItem } from "./codingSessionTranscriptItems.ts";

const CTX = {
  id: "item-1",
  sessionId: "session-1",
  targetKey: "target-1",
  channelId: null,
  timestamp: "2026-10-07T00:00:00.000Z",
};

function switched(model, requested) {
  return {
    kind: "status",
    status: "model_switched",
    model,
    requested,
    commandId: "model-1",
  };
}

test("model_switched names what took effect and the provider that signed it", () => {
  const item = buildBaseTranscriptItem(
    switched("opus[1m][high]", "opus[1m][high]"),
    { ...CTX, bridgeSource: { pubkey: "a".repeat(64), label: "Claude host" } },
  );
  assert.equal(item.title, "Switched to opus[1m][high] — Claude host");
  assert.equal(item.text, "");
  assert.equal(item.renderClass, "status");
});

test("a request the adapter trimmed reads Asked X · running Y", () => {
  const item = buildBaseTranscriptItem(
    switched("opus[1m]", "opus[1m][xhigh]"),
    CTX,
  );
  assert.equal(item.title, "Switched to opus[1m]");
  assert.equal(item.text, "Asked opus[1m][xhigh] · running opus[1m]");
});

test("a model_switched without a model falls back to the generic Status row", () => {
  assert.equal(
    codingSessionModelSwitchedRow({ status: "model_switched" }, null),
    undefined,
  );
  const item = buildBaseTranscriptItem(
    { kind: "status", status: "model_switched" },
    CTX,
  );
  assert.equal(item.title, "Status");
  assert.equal(item.text, "model_switched");
});
