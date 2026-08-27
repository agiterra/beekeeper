import assert from "node:assert/strict";
import { test } from "node:test";
import {
  buildCodingSessionExecutionKey,
  buildCodingSessionTargetKey,
  codingSessionReceiptSemanticKey,
  encodeStructuredKey,
} from "./keys.ts";

test("the cs-target key matches the desktop test literal byte for byte", () => {
  // Pinned from desktop/src/features/coding-sessions/lib/
  // codingSessionCommand.test.mjs — the tag value a signed 44220 carries.
  assert.equal(
    buildCodingSessionTargetKey({
      driver: "provider-a",
      instanceId: "instance-1",
      sessionId: "session-1",
      generation: 2,
    }),
    "coding-session/v1|10:provider-a10:instance-19:session-11:2",
  );
});

test("byte lengths, not code-unit lengths, prefix each field", () => {
  // "é" is two UTF-8 bytes but one JS character; a length-prefixed key that
  // counted characters would let two distinct tuples collide.
  assert.equal(encodeStructuredKey("d", "é"), "d|2:é");
  assert.equal(encodeStructuredKey("d", "ab"), "d|2:ab");
  assert.notEqual(
    encodeStructuredKey("d", "a", "bc"),
    encodeStructuredKey("d", "ab", "c"),
  );
});

test("generation is part of the target key: each generation is its own stream", () => {
  const base = {
    driver: "d",
    instanceId: "i",
    sessionId: "s",
    generation: 1,
  };
  assert.notEqual(
    buildCodingSessionTargetKey(base),
    buildCodingSessionTargetKey({ ...base, generation: 2 }),
  );
});

test("the execution key drops generation but keeps the signer", () => {
  const base = { driver: "d", instanceId: "i", sessionId: "s", generation: 1 };
  assert.equal(
    buildCodingSessionExecutionKey("pk", base),
    buildCodingSessionExecutionKey("pk", { ...base, generation: 7 }),
  );
  assert.notEqual(
    buildCodingSessionExecutionKey("pk", base),
    buildCodingSessionExecutionKey("other", base),
  );
});

test("a turn receipt key names its stage; a lifecycle receipt key does not", () => {
  assert.equal(
    codingSessionReceiptSemanticKey("cmd", "created", false),
    "coding-session-lifecycle-receipt/v1|3:cmd",
  );
  assert.equal(
    codingSessionReceiptSemanticKey("cmd", "turn_started", true),
    "coding-session-lifecycle-receipt/v1|3:cmd12:turn_started",
  );
});
