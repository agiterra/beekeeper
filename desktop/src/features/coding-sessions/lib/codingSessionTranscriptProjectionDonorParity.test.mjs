/**
 * Donor transcript-hydration parity — the Hive `parseTranscript.ts`
 * `processTranscriptMessages` law (donor pin `e0b8198bd144`) run against this
 * fork's tool_call/tool_result pairing loop. Buzz code is canonical:
 * agreements are frozen, divergences are asserted Buzz-side with the donor
 * expectation recorded beside them as findings, never bugs to fix.
 *
 * Scope: strictly the pairing edges the main projection suite does not cover.
 * Basic call/result collapse, generation-scoped pairing, and unknown-kind
 * totality are frozen there and are not duplicated here.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

let nextEventSeq = 1;

function makeEnvelope(item) {
  const eventSeq = nextEventSeq++;
  return {
    target: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "session-1",
      generation: 1,
    },
    eventSeq,
    timestamp: 1_700_000_000_000 + eventSeq,
    item,
  };
}

function toolCall(toolId, command) {
  return makeEnvelope({
    kind: "tool_call",
    tool: { toolId, toolName: "bash", input: { command } },
  });
}

function toolResult(toolId, content) {
  return makeEnvelope({ kind: "tool_result", toolId, content });
}

test("agreement: results attach backwards only — a result preceding its call never pairs", () => {
  // Donor: tool_result registers nothing, so a result arriving before its
  // call finds no pending entry and the later call stays unresolved. Buzz
  // agrees on the never-pairs-backwards law; the early result's FATE
  // diverges (donor drops it silently, Buzz emits it standalone — asserted
  // in the orphan test below).
  const items = projectCodingSessionTranscript([
    toolResult("t1", "early"),
    toolCall("t1", "echo hello"),
  ]);
  assert.equal(items.length, 2);
  assert.equal(items[1].type, "tool");
  assert.equal(items[1].status, "executing");
  assert.equal(items[1].result, "");
});

test("divergence (Buzz canonical): an orphan tool_result surfaces as a standalone item, the donor drops it silently", () => {
  // Donor: `if (pendingCall)` guard — an unknown toolId emits NO message and
  // the result vanishes from the transcript. Buzz's trust boundary never
  // drops input: the orphan degrades to a standalone tool item whose
  // toolName falls back to the toolId.
  const items = projectCodingSessionTranscript([
    toolResult("t-unknown", "orphan output"),
  ]);
  assert.equal(items.length, 1);
  assert.equal(items[0].type, "tool");
  assert.equal(items[0].toolName, "t-unknown");
  assert.equal(items[0].result, "orphan output");
});

test("agreement: duplicate toolId — the latest call wins the pairing, both call items stay", () => {
  // Donor: the pending map entry is overwritten by the later call; both
  // call messages remain and only the most recent can receive the result.
  // Buzz's Map.set has the same last-writer-wins shape.
  const items = projectCodingSessionTranscript([
    toolCall("t1", "echo one"),
    toolCall("t1", "echo two"),
    toolResult("t1", "res"),
  ]);
  assert.equal(items.length, 2);
  assert.equal(items[0].status, "executing");
  assert.equal(items[0].args.command, "echo one");
  assert.equal(items[0].result, "");
  assert.equal(items[1].status, "completed");
  assert.equal(items[1].args.command, "echo two");
  assert.equal(items[1].result, "res");
});

test("divergence (Buzz canonical): a second result for an already-paired call becomes standalone, the donor re-hydrates in place", () => {
  // Donor: the pending map entry is NEVER deleted, so a later result for the
  // same toolId overwrites the hydrated result in place (last result wins).
  // Buzz deletes the pending entry on pairing, so the second result has no
  // call to join and surfaces standalone; the paired card keeps the FIRST
  // result.
  const items = projectCodingSessionTranscript([
    toolCall("t1", "echo hello"),
    toolResult("t1", "first"),
    toolResult("t1", "second"),
  ]);
  assert.equal(items.length, 2);
  assert.equal(items[0].status, "completed");
  assert.equal(items[0].toolName, "bash");
  assert.equal(items[0].result, "first");
  assert.equal(items[1].type, "tool");
  assert.equal(items[1].result, "second");
});

test("agreement: pairing survives malformed interleavings between call and result", () => {
  // Donor: interleaved non-tool entries never disturb the pending map. Buzz
  // agrees — a status row and a kindless malformed item between call and
  // result leave the pairing intact (and, per Buzz's totality law, both
  // interlopers still surface as items).
  const items = projectCodingSessionTranscript([
    toolCall("t1", "echo hello"),
    makeEnvelope({ kind: "status", status: "thinking" }),
    makeEnvelope({ unexpected: true }),
    toolResult("t1", "late"),
  ]);
  assert.equal(items.length, 3);
  const tools = items.filter((item) => item.type === "tool");
  assert.equal(tools.length, 1);
  assert.equal(tools[0].status, "completed");
  assert.equal(tools[0].result, "late");
});

test("agreement: a paired card keeps the call's start time and takes the result's completion time", () => {
  // Donor law, preserved: the pairing collapses two entries into one card
  // whose lifetime spans both — the call's timestamp opens it, the result's
  // closes it. A card that reported the result's time for both would erase
  // every tool duration in the transcript.
  const call = toolCall("t-span", "sleep 1");
  const result = toolResult("t-span", "done");
  const [item] = projectCodingSessionTranscript([call, result]);
  assert.equal(item.startedAt, new Date(call.timestamp).toISOString());
  assert.equal(item.completedAt, new Date(result.timestamp).toISOString());
  assert.equal(item.timestamp, item.startedAt);
});
