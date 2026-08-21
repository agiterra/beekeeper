import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionHandoffLink,
  buildCodingSessionHandoffPrefill,
  isCompletedCodingSessionTurnBlock,
  parseCodingSessionHandoffLink,
  parseCodingSessionHandoffPrefill,
  readCodingSessionTranscriptItemEventSeq,
  readCodingSessionTurnBlockPrompt,
  resolveCodingSessionHandoffSource,
} from "./codingSessionHandoff.ts";
import { encodeStructuredKey } from "./codingSessionKeys.ts";

const LINK = {
  channelId: "0c8016c8-9483-4426-a4b1-b45c8e21d0a1",
  targetKey: "coding-session/v1|5:acp-x4:i-129:session-11:7",
  eventSeq: 42,
};

function assistantItem(overrides = {}) {
  return {
    id: encodeStructuredKey(
      "coding-session-transcript-item/v1",
      "generation-scope",
      "coding-session-transcript/v1|5:acp-x4:i-129:session-11:71:9",
      "9",
    ),
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text: "The failing test is fixtures/relay.rs:88.\nRegenerate the fixture.",
    timestamp: "2026-08-12T10:00:00.000Z",
    ...overrides,
  };
}

function turnResultItem() {
  return {
    id: "result-1",
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Done",
    timestamp: "2026-08-12T10:01:00.000Z",
  };
}

test("handoff deep link round-trips through URL encoding", () => {
  const url = buildCodingSessionHandoffLink(LINK);
  assert.match(url, /^beekeeper:\/\/coding-session\?/);
  assert.deepEqual(parseCodingSessionHandoffLink(url), LINK);
});

test("malformed deep links parse to null", () => {
  assert.equal(
    parseCodingSessionHandoffLink("beekeeper://message?id=abc"),
    null,
  );
  assert.equal(
    parseCodingSessionHandoffLink(
      "beekeeper://coding-session?channel=x&target=y",
    ),
    null,
  );
  assert.equal(
    parseCodingSessionHandoffLink(
      "beekeeper://coding-session?channel=x&target=y&seq=-4",
    ),
    null,
  );
  assert.equal(
    parseCodingSessionHandoffLink(
      "beekeeper://coding-session?channel=x&target=y&seq=nope",
    ),
    null,
  );
});

test("prefill builds the provenance block and parses back to its parts", () => {
  const prefill = buildCodingSessionHandoffPrefill({
    sourceLabel: "Claude · claude-opus-5",
    link: LINK,
    quote: "The failing test is fixtures/relay.rs:88.\nRegenerate the fixture.",
  });
  assert.match(
    prefill,
    /^> From Claude · claude-opus-5 \(this session\) — beekeeper:/,
  );
  assert.match(prefill, /\n> Regenerate the fixture\.\n\n$/);

  const parsed = parseCodingSessionHandoffPrefill(
    `${prefill}Take Claude's finding above and regenerate the fixture.`,
  );
  assert.ok(parsed);
  assert.equal(parsed.sourceLabel, "Claude · claude-opus-5");
  assert.deepEqual(parsed.link, LINK);
  assert.equal(
    parsed.quote,
    "The failing test is fixtures/relay.rs:88.\nRegenerate the fixture.",
  );
  assert.equal(
    parsed.instruction,
    "Take Claude's finding above and regenerate the fixture.",
  );
});

test("edited or plain prompts degrade: recognition returns null, never a guess", () => {
  assert.equal(parseCodingSessionHandoffPrefill("Just fix the fixture."), null);
  assert.equal(
    parseCodingSessionHandoffPrefill(
      "> A plain quote without a provenance header\nDo the thing.",
    ),
    null,
  );
  // A header with no quoted lines under it is not the built shape.
  assert.equal(
    parseCodingSessionHandoffPrefill(
      `> From Claude (this session) — ${buildCodingSessionHandoffLink(LINK)}\nDo it.`,
    ),
    null,
  );
});

test("a link-less provenance header still parses with link null (degraded chip)", () => {
  const parsed = parseCodingSessionHandoffPrefill(
    "> From Claude (this session) — beekeeper://coding-session?broken\n> quote\n\nGo.",
  );
  assert.ok(parsed);
  assert.equal(parsed.link, null);
  assert.equal(parsed.quote, "quote");
});

test("eventSeq is recovered from projected item ids and refused elsewhere", () => {
  assert.equal(readCodingSessionTranscriptItemEventSeq(assistantItem().id), 9);
  // A targetKey whose own content ends in digits cannot fool the front-walk.
  const trickyId = encodeStructuredKey(
    "coding-session-transcript-item/v1",
    "scope-7",
    "target-key-ending-42",
    "13",
  );
  assert.equal(readCodingSessionTranscriptItemEventSeq(trickyId), 13);
  assert.equal(
    readCodingSessionTranscriptItemEventSeq(
      encodeStructuredKey(
        "coding-session-transcript-item/v1",
        "scope",
        "target",
        "unknown-seq",
      ),
    ),
    null,
  );
  assert.equal(
    readCodingSessionTranscriptItemEventSeq(
      "coding-session:unrecoverable:seq-4",
    ),
    null,
  );
  assert.equal(readCodingSessionTranscriptItemEventSeq("agent-item-1"), null);
});

test("a completed block quotes its latest assistant claim", () => {
  const block = { items: [assistantItem(), turnResultItem()] };
  assert.equal(isCompletedCodingSessionTurnBlock(block), true);
  const source = resolveCodingSessionHandoffSource(block);
  assert.ok(source);
  assert.equal(source.eventSeq, 9);
  assert.match(source.quote, /fixtures\/relay\.rs:88/);
});

test("streaming and tool-only blocks offer no handoff source", () => {
  assert.equal(
    isCompletedCodingSessionTurnBlock({ items: [assistantItem()] }),
    false,
  );
  assert.equal(
    resolveCodingSessionHandoffSource({ items: [turnResultItem()] }),
    null,
  );
});

test("a block prompt is only the leading user message", () => {
  const userPrompt = assistantItem({ role: "user", id: "prompt-1" });
  assert.equal(
    readCodingSessionTurnBlockPrompt({ items: [userPrompt, assistantItem()] }),
    userPrompt,
  );
  assert.equal(
    readCodingSessionTurnBlockPrompt({ items: [assistantItem()] }),
    null,
  );
  assert.equal(readCodingSessionTurnBlockPrompt({ items: [] }), null);
});
