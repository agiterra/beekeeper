import assert from "node:assert/strict";
import test from "node:test";

import { projectCodingSessionTranscriptItem } from "./codingSessionTranscriptProjection.ts";

let nextEventSeq = 1;

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function envelope(overrides = {}) {
  const { target: targetOverrides, ...rest } = overrides;
  const eventSeq = overrides.eventSeq ?? nextEventSeq++;
  return {
    eventSeq,
    timestamp: 1_700_000_000_000 + eventSeq,
    item: { kind: "status", status: "idle" },
    ...rest,
    target: { ...TARGET, ...(targetOverrides ?? {}) },
  };
}

function isCompactRenderable(item) {
  return item.renderClass !== "raw-rail" && item.renderClass !== "suppressed";
}

test("quarantine-classed items render as bounded status only", () => {
  const secretPayload = "TOP-SECRET-RAW-CONTENT-MUST-NOT-LEAK";
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        schema: "seat-transcript-quarantine/v1",
        quarantineClass: "malformed_known_kind",
        decodeError: "malformed-shape",
        sourceKey: "provider-1:42",
        claimedKind: "assistant_text",
        claimedEntrySchema: "seat-transcript/v1",
        byteCount: 128,
        contentDigest: "abc123",
        originalLookingField: secretPayload,
      },
    }),
  );

  assert.equal(result.type, "lifecycle");
  assert.equal(result.renderClass, "status");
  assert.ok(isCompactRenderable(result));
  const serialized = JSON.stringify(result);
  assert.ok(!serialized.includes(secretPayload));
  assert.ok(serialized.includes("malformed_known_kind"));
  assert.ok(serialized.includes("abc123"));
});

test("quarantine metadata fields are length-bounded even when hostile-huge", () => {
  const hugeString = "x".repeat(50_000);
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        schema: "seat-transcript-quarantine/v1",
        quarantineClass: hugeString,
        decodeError: hugeString,
        sourceKey: hugeString,
        contentDigest: hugeString,
      },
    }),
  );
  assert.ok(result.text.length < 5_000);
  assert.ok(result.text.includes("truncated"));
});

test("an unrecognized-kind label is bounded even when hostile-huge", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "k".repeat(20_000) } }),
  );
  assert.ok(result.title.length < 500);
  assert.ok(result.text.length < 5_000);
});

test("system_init array fields are capped", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "system_init",
        tools: Array.from({ length: 10_000 }, (_, i) => `tool-${i}`),
      },
    }),
  );
  assert.ok(!result.text.includes("tool-9999"));
  assert.ok(result.text.includes("more"));
});

test("a plan item renders the producer's markdown checklist verbatim", () => {
  const text = "- [x] read the code\n- [ ] write the test (in progress)";
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "plan",
        entries: [
          { content: "read the code", priority: "high", status: "completed" },
          {
            content: "write the test",
            priority: "medium",
            status: "in_progress",
          },
        ],
        text,
      },
    }),
  );
  assert.equal(result.type, "plan");
  assert.equal(result.renderClass, "plan");
  assert.equal(result.title, "Plan");
  assert.equal(result.text, text);
});

test("a plan item with no text re-renders its entries", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "plan",
        entries: [
          { content: "done thing", status: "completed" },
          { content: "current thing", status: "in_progress" },
          { content: "later thing", status: "pending" },
          { content: "", status: "pending" },
          "not-a-record",
        ],
      },
    }),
  );
  assert.equal(result.type, "plan");
  assert.equal(
    result.text,
    "- [x] done thing\n- [ ] current thing (in progress)\n- [ ] later thing",
  );
});

test("a plan item with neither text nor entries remains a plan", () => {
  for (const item of [
    { kind: "plan" },
    { kind: "plan", text: "   ", entries: "not-an-array" },
  ]) {
    const result = projectCodingSessionTranscriptItem(envelope({ item }));
    assert.equal(result.type, "plan");
    assert.equal(result.text, "");
  }
});

test("an elided item surfaces a placeholder carrying size and digest", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "elided",
        reason: "oversize",
        byteCount: 41_235,
        contentDigest: "sha256:deadbeef",
      },
    }),
  );
  assert.equal(result.type, "lifecycle");
  assert.equal(result.renderClass, "status");
  assert.ok(isCompactRenderable(result));
  assert.equal(result.title, "Content dropped");
  assert.deepEqual(result.elision, {
    bytes: 41_235,
    digest: "deadbeef",
    reason: "oversize",
  });
  assert.ok(result.text.includes("41 KB"));
  assert.ok(result.text.includes("oversize"));
  assert.ok(result.text.includes("sha256:deadbeef"));
});

test("an elided item with missing fields reports unknowns", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "elided" } }),
  );
  assert.deepEqual(result.elision, {
    bytes: null,
    digest: null,
    reason: "unknown",
  });
  assert.ok(result.text.includes("an unknown amount"));
  assert.ok(result.text.includes("unknown"));
  assert.ok(!result.text.includes("sha256:"));
});

test("reasoning is admitted into the thought lane", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: { kind: "reasoning", text: "I should read the file first." },
    }),
  );
  assert.equal(result.type, "thought");
  assert.equal(result.renderClass, "thought");
  assert.equal(result.title, "Reasoning");
  assert.equal(result.text, "I should read the file first.");
});

test("a reasoning item with non-string text degrades safely", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "reasoning", text: { nested: "object" } } }),
  );
  assert.equal(result.type, "thought");
  assert.equal(result.text, "");
});

test("exit_plan_mode plan derivation still applies", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "tool_call",
        tool: {
          toolId: "plan-1",
          toolName: "exit_plan_mode",
          input: { plan: "- inspect\n- verify" },
        },
      },
    }),
  );
  assert.equal(result.type, "plan");
  assert.equal(result.title, "Plan proposal");
  assert.equal(result.text, "- inspect\n- verify");
});
