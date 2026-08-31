import assert from "node:assert/strict";
import test from "node:test";

import { projectCodingSessionMissionTimeline } from "./codingSessionMissionStreamModel.ts";

function item(id, type, extra = {}) {
  return {
    id,
    type,
    renderClass: type === "message" ? "message" : "status",
    title: id,
    text: id,
    timestamp: "2026-08-30T12:00:00.000Z",
    ...extra,
  };
}

function block(items) {
  return {
    kind: "turn-block",
    executionKey: "seat",
    signerPubkey: "a".repeat(64),
    generation: 1,
    generationId: "generation",
    turnId: "turn",
    blockSeq: 0,
    items,
    timestampMs: 1,
  };
}

test("Brief removes only routine execution and keeps chronology", () => {
  const conversation = {
    kind: "conversation",
    message: { eventId: "event" },
    timestampMs: 0,
  };
  const projected = projectCodingSessionMissionTimeline(
    [
      conversation,
      block([
        item("thought", "thought", { renderClass: "thought" }),
        item("message", "message", { role: "assistant" }),
        item("failed", "tool", {
          renderClass: "shell",
          descriptor: {
            renderClass: "shell",
            label: "Terminal",
            preview: null,
          },
          toolName: "Terminal",
          buzzToolName: null,
          status: "failed",
          args: {},
          result: "failed",
          isError: true,
          startedAt: "2026-08-30T12:00:00.000Z",
          completedAt: "2026-08-30T12:00:01.000Z",
        }),
      ]),
    ],
    "brief",
  );
  assert.equal(projected[0], conversation);
  assert.deepEqual(
    projected[1].items.map(({ id }) => id),
    ["message", "failed"],
  );
});

test("Brief keeps permission, failure, verdict-like result, and required-action rows", () => {
  const projected = projectCodingSessionMissionTimeline(
    [
      block([
        item("permission", "lifecycle", { renderClass: "permission" }),
        item("result", "lifecycle", { renderClass: "status" }),
        item("blocker", "lifecycle", { renderClass: "error" }),
      ]),
    ],
    "brief",
  );
  assert.deepEqual(
    projected[0].items.map(({ id }) => id),
    ["permission", "result", "blocker"],
  );
});

test("Live and Trace preserve the complete reversible chronology", () => {
  const entries = [block([item("thought", "thought")])];
  assert.deepEqual(
    projectCodingSessionMissionTimeline(entries, "live"),
    entries,
  );
  assert.deepEqual(
    projectCodingSessionMissionTimeline(entries, "trace"),
    entries,
  );
});
