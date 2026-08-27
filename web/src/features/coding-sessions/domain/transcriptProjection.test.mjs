import assert from "node:assert/strict";
import { test } from "node:test";
import {
  orderCodingSessionTranscriptBlocks,
  projectCodingSessionTranscript,
} from "./transcriptProjection.ts";

const TARGET = {
  driver: "provider-a",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 1,
};

function envelope(eventSeq, item, extra = {}) {
  return {
    target: TARGET,
    eventSeq,
    timestamp: 1_700_000_000_000 + eventSeq,
    item,
    eventId: `e${String(eventSeq).padStart(3, "0")}`,
    ...extra,
  };
}

test("eventSeq orders numerically, not lexicographically", () => {
  const items = projectCodingSessionTranscript([
    envelope(10, { kind: "assistant_text", text: "ten" }),
    envelope(9, { kind: "assistant_text", text: "nine" }),
    envelope(2, { kind: "assistant_text", text: "two" }),
  ]);
  assert.deepEqual(
    items.map((item) => item.text),
    ["two", "nine", "ten"],
  );
});

test("a tie on eventSeq breaks on event id, never on arrival order", () => {
  const forward = projectCodingSessionTranscript([
    { ...envelope(1, { kind: "assistant_text", text: "b" }), eventId: "bbb" },
    { ...envelope(1, { kind: "assistant_text", text: "a" }), eventId: "aaa" },
  ]);
  const reversed = projectCodingSessionTranscript([
    { ...envelope(1, { kind: "assistant_text", text: "a" }), eventId: "aaa" },
    { ...envelope(1, { kind: "assistant_text", text: "b" }), eventId: "bbb" },
  ]);
  assert.deepEqual(
    forward.map((item) => item.text),
    ["a", "b"],
  );
  assert.deepEqual(
    reversed.map((item) => item.text),
    ["a", "b"],
  );
});

test("a tool_call pairs with its tool_result into one completed row", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "tool_call",
      tool: { toolName: "read", toolId: "t1", input: { path: "a.txt" } },
    }),
    envelope(2, { kind: "tool_result", toolId: "t1", content: "file body" }),
  ]);
  assert.equal(items.length, 1);
  assert.equal(items[0].role, "tool");
  assert.equal(items[0].tool.toolName, "read");
  assert.equal(items[0].tool.status, "completed");
  assert.equal(items[0].tool.result, "file body");
  assert.equal(items[0].folded, true, "tool rows fold to one line by default");
});

test("an errored tool_result marks the paired row as an error", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "tool_call",
      tool: { toolName: "bash", toolId: "t1", input: {} },
    }),
    envelope(2, {
      kind: "tool_result",
      toolId: "t1",
      content: "boom",
      isError: true,
    }),
  ]);
  assert.equal(items[0].tool.status, "error");
});

test("an unpaired tool_result renders standalone rather than vanishing", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "tool_result", toolId: "orphan", content: "x" }),
  ]);
  assert.equal(items.length, 1);
  assert.equal(items[0].role, "tool");
  assert.equal(items[0].tool.status, "completed");
});

test("a tool_call from another target never pairs across streams", () => {
  const other = { ...TARGET, sessionId: "session-2" };
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "tool_call",
      tool: { toolName: "read", toolId: "t1", input: {} },
    }),
    { ...envelope(2, { kind: "tool_result", toolId: "t1" }), target: other },
  ]);
  assert.equal(items.length, 2, "the result must not collapse the other call");
  assert.equal(items[0].tool.status, "pending");
});

test("an unknown kind renders as a named status row with no payload", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "wormhole", secret: "do not render me" }),
  ]);
  assert.equal(items.length, 1);
  assert.equal(items[0].unknownKind, "wormhole");
  assert.equal(items[0].text, "");
  assert.deepEqual(items[0].meta, []);
  assert.match(items[0].title, /wormhole/);
  assert.equal(
    JSON.stringify(items[0]).includes("do not render me"),
    false,
    "no field of an unknown item may reach the row",
  );
});

test("an elided item shows only its reason and byte count", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "elided",
      reason: "too large",
      byteCount: 40_000,
      contentDigest: "deadbeef",
    }),
  ]);
  assert.equal(items[0].title, "Content elided");
  assert.equal(items[0].text, "");
  assert.deepEqual(items[0].meta, ["too large", "40000 bytes"]);
});

test("reasoning is folded by default", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "reasoning", text: "thinking out loud" }),
  ]);
  assert.equal(items[0].title, "Reasoning");
  assert.equal(items[0].folded, true);
});

test("a steered prompt is titled as steered and opens no new turn", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "user_prompt", content: "first" }),
    envelope(2, { kind: "user_prompt", content: "steer", steered: true }),
  ]);
  assert.equal(items[0].title, "Prompt");
  assert.equal(items[1].title, "Steered prompt");
  assert.equal(
    items[0].turnId,
    items[1].turnId,
    "a steer belongs to the turn already open",
  );
});

test("a declared turnId wins outright, including an explicit null", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "user_prompt", content: "x" }, { turnId: "turn-a" }),
    envelope(2, { kind: "assistant_text", text: "y" }, { turnId: null }),
  ]);
  assert.equal(items[0].turnId, "turn-a");
  assert.equal(items[1].turnId, null, "null means: belongs to no turn");
});

test("a result closes the synthetic turn; later telemetry stays ungrouped", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, { kind: "user_prompt", content: "go" }),
    envelope(2, { kind: "assistant_text", text: "ok" }),
    envelope(3, { kind: "result", durationMs: 12, costUsd: 0.5 }),
    envelope(4, { kind: "status", status: "idle" }),
  ]);
  assert.notEqual(items[0].turnId, null);
  assert.equal(items[1].turnId, items[0].turnId);
  assert.equal(items[3].turnId, null);
});

test("a turn result carries duration and cost structurally, not baked into text", () => {
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "result",
      durationMs: 1234,
      costUsd: 0.02,
      result: "done",
    }),
  ]);
  assert.equal(items[0].title, "Turn result");
  assert.deepEqual(items[0].lifecycle, {
    durationMs: 1234,
    costUsd: 0.02,
    isError: false,
  });
  assert.equal(items[0].text, "done");
});

test("a prompt carries its operator and command as metadata, truncated", () => {
  const operator = "b".repeat(64);
  const items = projectCodingSessionTranscript([
    envelope(1, {
      kind: "user_prompt",
      content: "hi",
      operatorPubkey: operator,
      commandId: "cmd-123456789",
    }),
  ]);
  assert.equal(items[0].meta.length, 2);
  assert.equal(
    items[0].meta[0].includes(operator),
    false,
    "a full pubkey is never printed as a recognition aid",
  );
});

test("hostile input degrades instead of throwing", () => {
  assert.deepEqual(projectCodingSessionTranscript(null), []);
  assert.deepEqual(projectCodingSessionTranscript("nope"), []);
  const items = projectCodingSessionTranscript([
    envelope(1, null),
    envelope(2, { noKind: true }),
  ]);
  assert.equal(items.length, 2);
  assert.equal(items[0].text, "");
});

test("blocks order by start time, never by merging their items", () => {
  const ordered = orderCodingSessionTranscriptBlocks([
    { blockKey: "b", label: "B", items: [], startedAt: 200 },
    { blockKey: "a", label: "A", items: [], startedAt: 100 },
  ]);
  assert.deepEqual(
    ordered.map((block) => block.blockKey),
    ["a", "b"],
  );
});
