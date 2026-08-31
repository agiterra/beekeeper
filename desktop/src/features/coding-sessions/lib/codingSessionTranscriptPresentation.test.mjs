import assert from "node:assert/strict";
import test from "node:test";

import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  buildCodingSessionTranscriptGenerationId,
  projectTrustedCodingSessionTranscriptsToTranscript,
} from "./codingSessionTranscriptPresentation.ts";

const CHANNEL_ID = "channel-1";
const SIGNER = "a".repeat(64);
const OTHER_SIGNER = "b".repeat(64);
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function entry({
  eventSeq,
  item,
  turnId = "turn-1",
  channelId = CHANNEL_ID,
  signerPubkey = SIGNER,
  target = TARGET,
  conflictCount = 0,
  eventId = `event-${eventSeq}`,
}) {
  return {
    channelId,
    targetKey: "unused-by-the-projector",
    signerPubkey,
    transcript: {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: target,
      eventSeq,
      timestamp: 1_800_000_000_000 + eventSeq,
      turnId,
      item,
    },
    eventId,
    createdAt: 1_800_000_000 + eventSeq,
    conflictCount,
  };
}

const GENERATION_ID = buildCodingSessionTranscriptGenerationId(
  CHANNEL_ID,
  SIGNER,
  TARGET,
);

test("generation identity is collision-free across channel, signer, and target", () => {
  const ids = new Set([
    GENERATION_ID,
    buildCodingSessionTranscriptGenerationId("channel-2", SIGNER, TARGET),
    buildCodingSessionTranscriptGenerationId(CHANNEL_ID, OTHER_SIGNER, TARGET),
    buildCodingSessionTranscriptGenerationId(CHANNEL_ID, SIGNER, {
      ...TARGET,
      generation: 2,
    }),
  ]);
  assert.equal(ids.size, 4);
  assert.equal(
    GENERATION_ID,
    buildCodingSessionTranscriptGenerationId(CHANNEL_ID, SIGNER, {
      ...TARGET,
    }),
  );
});

test("entries project in eventSeq order and carry the generation scope", () => {
  const items = projectTrustedCodingSessionTranscriptsToTranscript(
    [
      entry({ eventSeq: 2, item: { kind: "assistant_text", text: "second" } }),
      entry({ eventSeq: 1, item: { kind: "user_prompt", content: "first" } }),
      entry({ eventSeq: 3, item: { kind: "assistant_text", text: "third" } }),
    ],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.deepEqual(
    items.map((item) => item.text),
    ["first", "second", "third"],
  );
  for (const item of items) {
    assert.equal(item.sessionId, GENERATION_ID);
    assert.equal(item.channelId, CHANNEL_ID);
    assert.equal(item.turnId, "turn-1");
    assert.deepEqual(item.bridgeSource, {
      pubkey: SIGNER,
      label: "Trusted coding-session provider",
    });
  }
});

test("the ingress authority's label names the source when one is supplied", () => {
  const [item] = projectTrustedCodingSessionTranscriptsToTranscript(
    [entry({ eventSeq: 1, item: { kind: "assistant_text", text: "hi" } })],
    CHANNEL_ID,
    SIGNER,
    TARGET,
    { pubkey: SIGNER, label: "This computer (coding sessions)" },
  );
  assert.deepEqual(item.bridgeSource, {
    pubkey: SIGNER,
    label: "This computer (coding sessions)",
  });
});

test("trusted ingress retains the exact signed source and provider cursor", () => {
  const eventId = "c".repeat(64);
  const [item] = projectTrustedCodingSessionTranscriptsToTranscript(
    [
      entry({
        eventSeq: 7,
        eventId,
        item: { kind: "result", subtype: "success", result: "done" },
      }),
    ],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.equal(item.sourceEventId, eventId);
  assert.equal(item.sourceEventSeq, 7);
  assert.equal(typeof item.sourceTargetKey, "string");
  assert.ok(item.sourceTargetKey.length > 0);
});

test("a duplicate eventSeq orders on event id, not on arrival", () => {
  const first = entry({
    eventSeq: 1,
    item: { kind: "assistant_text", text: "aaa" },
    eventId: "aaa",
  });
  const second = entry({
    eventSeq: 1,
    item: { kind: "assistant_text", text: "bbb" },
    eventId: "bbb",
  });
  const forward = projectTrustedCodingSessionTranscriptsToTranscript(
    [first, second],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  const reversed = projectTrustedCodingSessionTranscriptsToTranscript(
    [second, first],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.deepEqual(
    forward.map((item) => item.text),
    ["aaa", "bbb"],
  );
  assert.deepEqual(
    reversed.map((item) => item.text),
    forward.map((item) => item.text),
  );
});

test("entries from another channel, signer, target, or in conflict are excluded", () => {
  const items = projectTrustedCodingSessionTranscriptsToTranscript(
    [
      entry({ eventSeq: 1, item: { kind: "assistant_text", text: "kept" } }),
      entry({
        eventSeq: 2,
        item: { kind: "assistant_text", text: "other channel" },
        channelId: "channel-2",
      }),
      entry({
        eventSeq: 3,
        item: { kind: "assistant_text", text: "other signer" },
        signerPubkey: OTHER_SIGNER,
      }),
      entry({
        eventSeq: 4,
        item: { kind: "assistant_text", text: "other generation" },
        target: { ...TARGET, generation: 2 },
      }),
      entry({
        eventSeq: 5,
        item: { kind: "assistant_text", text: "conflicted" },
        conflictCount: 1,
      }),
    ],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.deepEqual(
    items.map((item) => item.text),
    ["kept"],
  );
  assert.deepEqual(
    projectTrustedCodingSessionTranscriptsToTranscript(
      [],
      CHANNEL_ID,
      SIGNER,
      TARGET,
    ),
    [],
  );
});

test("no presentation-only identity leaks into the projected items", () => {
  const items = projectTrustedCodingSessionTranscriptsToTranscript(
    [
      entry({ eventSeq: 1, item: { kind: "user_prompt", content: "go" } }),
      entry({
        eventSeq: 2,
        item: {
          kind: "tool_call",
          tool: { toolId: "t1", toolName: "shell", input: { command: "ls" } },
        },
      }),
      entry({
        eventSeq: 3,
        item: { kind: "tool_result", toolId: "t1", content: "a.txt" },
      }),
    ],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.equal(items.length, 2, "call and result still collapse into one card");
  const serialized = JSON.stringify(items);
  assert.ok(!serialized.includes("presentation-only"));
  assert.ok(!serialized.includes("hiveInstanceId"));
  assert.ok(!serialized.includes("seatId"));
});

test("a turn boundary in the entries is preserved, not re-derived", () => {
  const items = projectTrustedCodingSessionTranscriptsToTranscript(
    [
      entry({
        eventSeq: 1,
        item: { kind: "user_prompt", content: "one" },
        turnId: "turn-a",
      }),
      entry({
        eventSeq: 2,
        item: { kind: "assistant_text", text: "still one" },
        turnId: "turn-a",
      }),
      entry({
        eventSeq: 3,
        item: { kind: "user_prompt", content: "two" },
        turnId: "turn-b",
      }),
      entry({
        eventSeq: 4,
        item: { kind: "status", status: "idle" },
        turnId: null,
      }),
    ],
    CHANNEL_ID,
    SIGNER,
    TARGET,
  );
  assert.deepEqual(
    items.map((item) => item.turnId),
    ["turn-a", "turn-a", "turn-b", undefined],
  );
});
