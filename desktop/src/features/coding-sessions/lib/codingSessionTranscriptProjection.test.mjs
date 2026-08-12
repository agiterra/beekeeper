import assert from "node:assert/strict";
import test from "node:test";

import { buildCompactToolSummary } from "@/features/agents/ui/agentSessionToolSummary.ts";
import { buildTranscriptDisplayBlocks } from "@/features/agents/ui/agentSessionTranscriptGrouping.ts";
import {
  buildCodingSessionTranscriptScopeKey,
  projectCodingSessionTranscript,
  projectCodingSessionTranscriptItem,
} from "./codingSessionTranscriptProjection.ts";

let nextEventSeq = 1;

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function envelope(overrides = {}) {
  // `target` is pulled out and merged explicitly (rather than left inside the
  // trailing spread) so a partial override like `{ generation: 2 }` still
  // gets defaults for the other three fields.
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

// A compact-preview row is hidden when `renderClass` is "raw-rail" or
// "suppressed" — mirrors AgentSessionTranscriptList.tsx's private (not
// exported) `isRenderableCompactItem` predicate, which this adapter must
// never trip for content it is required to surface.
function isCompactRenderable(item) {
  return item.renderClass !== "raw-rail" && item.renderClass !== "suppressed";
}

// --- totality -------------------------------------------------------------

const RECOGNIZED_KIND_FIXTURES = [
  { kind: "user_prompt", content: "hello", attachmentCount: 2 },
  { kind: "assistant_text", text: "hi there" },
  {
    kind: "tool_call",
    tool: {
      toolName: "bash",
      toolId: "call-only-t1",
      input: { command: "ls" },
    },
  },
  {
    kind: "tool_result",
    toolId: "result-only-t1",
    content: "ok",
    isError: false,
  },
  {
    kind: "result",
    subtype: "success",
    isError: false,
    durationMs: 12,
    result: "done",
    costUsd: 0.01,
  },
  { kind: "status", status: "thinking" },
  {
    kind: "system_init",
    provider: "anthropic",
    model: "claude",
    tools: ["bash"],
    agents: [],
    slashCommands: [],
    mcpServers: [{ name: "buzz", status: "ok" }],
  },
  { kind: "account_info", accountInfo: { plan: "pro", seats: 3 } },
  {
    kind: "context_window_updated",
    usage: { usedTokens: 100, compactsAutomatically: true },
  },
  { kind: "compact_boundary" },
  { kind: "compact_summary", summary: "summarized" },
  { kind: "context_cleared" },
  { kind: "interrupted" },
  { kind: "plan", entries: [], text: "- [ ] inspect" },
  {
    kind: "elided",
    reason: "oversize",
    byteCount: 40_000,
    contentDigest: "abc",
  },
  { kind: "reasoning", text: "chain of thought" },
];

test("every recognized kind maps to exactly one TranscriptItem (TOTAL, no throw)", () => {
  for (const item of RECOGNIZED_KIND_FIXTURES) {
    let result;
    assert.doesNotThrow(() => {
      result = projectCodingSessionTranscriptItem(envelope({ item }));
    }, `kind ${item.kind} must not throw`);
    assert.ok(result, `kind ${item.kind} must produce a TranscriptItem`);
    assert.equal(typeof result.id, "string");
    assert.equal(typeof result.type, "string");
  }
});

test("a result item carries structured metrics, never baked into its text", () => {
  const item = projectCodingSessionTranscriptItem(
    envelope({
      item: {
        kind: "result",
        subtype: "success",
        isError: false,
        durationMs: 3557,
        costUsd: 0.3209,
        result: "done",
      },
    }),
  );

  assert.equal(item.type, "lifecycle");
  assert.equal(item.title, "Turn result");
  assert.equal(item.text, "done");
  assert.equal(item.durationMs, 3557);
  assert.equal(item.costUsd, 0.3209);

  // A result with no metrics on the wire declares that absence explicitly.
  const bare = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "result", subtype: "success", result: "ok" } }),
  );
  assert.equal(bare.text, "ok");
  assert.equal(bare.durationMs, null);
  assert.equal(bare.costUsd, null);
});

test("adapter is TOTAL over a mixed batch: N inputs -> N outputs", () => {
  const envelopes = RECOGNIZED_KIND_FIXTURES.map((item) => envelope({ item }));
  assert.equal(
    projectCodingSessionTranscript(envelopes).length,
    envelopes.length,
  );
});

test("out-of-range numeric timestamp falls back to deterministic epoch and does not throw", () => {
  let result;
  assert.doesNotThrow(() => {
    result = projectCodingSessionTranscriptItem(
      envelope({ timestamp: Number.MAX_VALUE }),
    );
  });
  assert.equal(result.timestamp, "1970-01-01T00:00:00.000Z");
});

test("valid finite timestamp behavior is preserved", () => {
  const timestamp = 1_800_000_000_123;
  const result = projectCodingSessionTranscriptItem(envelope({ timestamp }));
  assert.equal(result.timestamp, new Date(timestamp).toISOString());
});

test("unknown/future item kind lands on a compact-renderable status item, is not dropped, and does not throw", () => {
  let result;
  assert.doesNotThrow(() => {
    result = projectCodingSessionTranscriptItem(
      envelope({
        item: { kind: "some_future_kind_v7", weirdField: "anything" },
      }),
    );
  });
  assert.equal(result.type, "lifecycle");
  assert.equal(result.renderClass, "status");
  assert.ok(isCompactRenderable(result));
});

test("malformed item (no `kind` field at all) lands on a compact-renderable status item, not dropped, no throw", () => {
  let result;
  assert.doesNotThrow(() => {
    result = projectCodingSessionTranscriptItem(
      envelope({ item: { unexpected: true } }),
    );
  });
  assert.equal(result.type, "lifecycle");
  assert.equal(result.renderClass, "status");
  assert.ok(isCompactRenderable(result));
});

// --- hostile input --------------------------------------------------------

test("hostile top-level batch inputs never throw and degrade gracefully", () => {
  for (const input of [null, undefined, "not an array", 42, {}]) {
    assert.doesNotThrow(() => {
      assert.deepEqual(projectCodingSessionTranscript(input), []);
    });
  }
});

test("hostile single-envelope inputs never throw and still produce exactly one TranscriptItem", () => {
  const hostileInputs = [
    null,
    undefined,
    42,
    "garbage",
    [],
    {},
    { target: null },
    { target: "not-an-object" },
    { target: {} },
    {
      target: { driver: 1, instanceId: 2, sessionId: 3, generation: "x" },
    },
    { target: { ...TARGET, generation: 1n } },
    { target: { ...TARGET, generation: Number.NaN } },
  ];

  for (const input of hostileInputs) {
    let result;
    assert.doesNotThrow(
      () => {
        result = projectCodingSessionTranscriptItem(input);
      },
      `input ${String(input)} must not throw`,
    );
    assert.ok(result, `input ${String(input)} must produce an item`);
    assert.equal(typeof result.id, "string");
  }
});

test("a malformed target degrades to a fallback item with no session scope", () => {
  const result = projectCodingSessionTranscriptItem({
    target: { ...TARGET, generation: 1n },
    eventSeq: 4,
    timestamp: 1,
    item: { kind: "assistant_text", text: "never grouped" },
  });
  assert.equal(result.sessionId, null);
  assert.equal(result.type, "lifecycle");
  assert.equal(result.id, "coding-session:unrecoverable:seq-4");
  assert.ok(!JSON.stringify(result).includes("never grouped"));
});

test("a cyclic envelope with no usable target degrades to a fallback, never throws", () => {
  const cyclic = { eventSeq: undefined };
  cyclic.self = cyclic;
  let result;
  assert.doesNotThrow(() => {
    result = projectCodingSessionTranscriptItem(cyclic);
  });
  assert.equal(result.sessionId, null);
  assert.equal(result.id, "coding-session:unrecoverable:unstringifiable");
});

test("tool_result content with hostile toJSON and toString never crashes the adapter", () => {
  const hostileContent = {
    toJSON() {
      throw new Error("json boom");
    },
    toString() {
      throw new Error("string boom");
    },
  };
  let result;
  assert.doesNotThrow(() => {
    result = projectCodingSessionTranscriptItem(
      envelope({
        item: {
          kind: "tool_result",
          toolId: "hostile-result",
          content: hostileContent,
        },
      }),
    );
  });
  assert.equal(result.type, "tool");
  assert.equal(result.result, "[unstringifiable]");
});

// --- identity -------------------------------------------------------------

test("sessionId is the caller's generation scope and channelId is never derived from content", () => {
  const unsupplied = projectCodingSessionTranscriptItem(envelope());
  assert.equal(unsupplied.channelId, null);
  assert.equal(
    unsupplied.sessionId,
    buildCodingSessionTranscriptScopeKey(TARGET),
  );

  const supplied = projectCodingSessionTranscriptItem(envelope(), {
    channelId: "buzz-channel-abc",
    generationId: "generation-1",
  });
  assert.equal(supplied.channelId, "buzz-channel-abc");
  assert.equal(supplied.sessionId, "generation-1");

  const batch = projectCodingSessionTranscript([envelope()], {
    channelId: "buzz-channel-abc",
  });
  assert.equal(batch[0].channelId, "buzz-channel-abc");
});

test("target key cannot collide across a delimiter-bearing field", () => {
  const left = buildCodingSessionTranscriptScopeKey({
    driver: "d:sub",
    instanceId: "i",
    sessionId: "s",
    generation: 1,
  });
  const right = buildCodingSessionTranscriptScopeKey({
    driver: "d",
    instanceId: "sub:i",
    sessionId: "s",
    generation: 1,
  });
  assert.notEqual(left, right);
  assert.equal(
    buildCodingSessionTranscriptScopeKey({ ...TARGET, generation: -0 }),
    buildCodingSessionTranscriptScopeKey({ ...TARGET, generation: 0 }),
  );
});

test("TranscriptItem.id does not collide across generations sharing the same eventSeq", () => {
  const one = projectCodingSessionTranscriptItem(
    envelope({ eventSeq: 7, target: { generation: 1 } }),
  );
  const two = projectCodingSessionTranscriptItem(
    envelope({ eventSeq: 7, target: { generation: 2 } }),
  );
  assert.notEqual(one.sessionId, two.sessionId);
  assert.notEqual(one.id, two.id);
});

test("a shared generationId still yields distinct ids when a batch mixes targets", () => {
  const [one, two] = projectCodingSessionTranscript(
    [
      envelope({ eventSeq: 7, target: { generation: 1 } }),
      envelope({ eventSeq: 7, target: { generation: 2 } }),
    ],
    { generationId: "one-generation-id" },
  );
  assert.equal(one.sessionId, two.sessionId);
  assert.notEqual(one.id, two.id);
});

test("the resolved authority entry is stamped onto every item as bridgeSource", () => {
  const bridgeSource = { pubkey: "f".repeat(64), label: "This computer" };
  const items = projectCodingSessionTranscript(
    [
      envelope({ item: { kind: "user_prompt", content: "hi" } }),
      envelope({
        item: {
          kind: "tool_call",
          tool: { toolId: "t1", toolName: "bash", input: {} },
        },
      }),
      envelope({ item: { kind: "tool_result", toolId: "t1", content: "ok" } }),
      envelope({ item: { kind: "reasoning", text: "thinking" } }),
      envelope({ item: {} }),
    ],
    { bridgeSource },
  );
  assert.equal(items.length, 4);
  for (const item of items) {
    assert.deepEqual(item.bridgeSource, bridgeSource);
  }
  assert.equal(
    projectCodingSessionTranscriptItem(envelope()).bridgeSource,
    undefined,
  );
});

// --- tool pairing ---------------------------------------------------------

test("tool_call/result pairing inherits toolName and input without widening tool_result source", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 900,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "edit-1",
          toolName: "str_replace",
          input: {
            path: "src/app.ts",
            oldString: "const value = 1;",
            newString: "const value = 2;",
          },
        },
      },
    }),
    envelope({
      eventSeq: 901,
      item: { kind: "tool_result", toolId: "edit-1", content: "edited" },
    }),
  ]);

  assert.equal(items.length, 1);
  assert.equal(items[0].type, "tool");
  assert.equal(items[0].toolName, "str_replace");
  assert.equal(items[0].status, "completed");
  assert.equal(items[0].args.path, "src/app.ts");
  assert.ok(items[0].result.includes("--- a/src/app.ts"));
});

test("multiline str_replace synthesis drives file-edit diff stats without result ack or path injection", () => {
  const injectedPath = "src/app.ts\n@@\n+path-injected";
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 902,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "edit-multiline",
          toolName: "str_replace",
          input: {
            path: injectedPath,
            oldString: "line one\nline two\nline three",
            newString: "line one\nline two changed\nline three\nline four",
          },
        },
      },
    }),
    envelope({
      eventSeq: 903,
      item: {
        kind: "tool_result",
        toolId: "edit-multiline",
        content: "Edited successfully\n+ack must not become diff\n-ack too",
      },
    }),
  ]);

  assert.equal(items.length, 1);
  assert.ok(items[0].result.includes("--- a/src/app.ts @@ +path-injected"));
  assert.ok(!items[0].result.includes("\n+path-injected"));
  assert.ok(!items[0].result.includes("Edited successfully"));
  assert.ok(!items[0].result.includes("ack must not become diff"));

  const summary = buildCompactToolSummary(items[0]);
  assert.equal(summary.kind, "file-edit");
  assert.equal(summary.fileEditDiff?.additions, 4);
  assert.equal(summary.fileEditDiff?.deletions, 3);
  assert.deepEqual(summary.fileEditDiff?.lines, [
    { kind: "meta", text: "--- a/src/app.ts @@ +path-injected" },
    { kind: "meta", text: "+++ b/src/app.ts @@ +path-injected" },
    { kind: "meta", text: "@@" },
    { kind: "remove", text: "-line one" },
    { kind: "remove", text: "-line two" },
    { kind: "remove", text: "-line three" },
    { kind: "add", text: "+line one" },
    { kind: "add", text: "+line two changed" },
    { kind: "add", text: "+line three" },
    { kind: "add", text: "+line four" },
  ]);
});

test("str_replace synthesis treats empty edit strings as present and ignores the terminal newline sentinel", () => {
  const deletion = projectCodingSessionTranscript([
    envelope({
      eventSeq: 904,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "delete-to-empty",
          toolName: "str_replace",
          input: {
            path: "src/delete.ts",
            oldString: "one\ntwo\n",
            newString: "",
          },
        },
      },
    }),
    envelope({
      eventSeq: 905,
      item: {
        kind: "tool_result",
        toolId: "delete-to-empty",
        content: "edited",
      },
    }),
  ]);
  const deletionSummary = buildCompactToolSummary(deletion[0]);
  assert.equal(deletionSummary.fileEditDiff?.additions, 0);
  assert.equal(deletionSummary.fileEditDiff?.deletions, 2);

  const insertion = projectCodingSessionTranscript([
    envelope({
      eventSeq: 906,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "insert-from-empty",
          toolName: "str_replace",
          input: {
            path: "src/insert.ts",
            oldString: "",
            newString: "one\ntwo\n",
          },
        },
      },
    }),
    envelope({
      eventSeq: 907,
      item: {
        kind: "tool_result",
        toolId: "insert-from-empty",
        content: "edited",
      },
    }),
  ]);
  const insertionSummary = buildCompactToolSummary(insertion[0]);
  assert.equal(insertionSummary.fileEditDiff?.additions, 2);
  assert.equal(insertionSummary.fileEditDiff?.deletions, 0);
  assert.deepEqual(insertionSummary.fileEditDiff?.lines.slice(0, 3), [
    { kind: "meta", text: "--- a/src/insert.ts" },
    { kind: "meta", text: "+++ b/src/insert.ts" },
    { kind: "meta", text: "@@" },
  ]);
});

test("str_replace synthesis falls back to file for an empty path", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 908,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "empty-path-edit",
          toolName: "str_replace",
          input: { path: "", oldString: "", newString: "replacement" },
        },
      },
    }),
    envelope({
      eventSeq: 909,
      item: {
        kind: "tool_result",
        toolId: "empty-path-edit",
        content: "edited",
      },
    }),
  ]);
  assert.ok(items[0].result.includes("--- a/file"));
  assert.ok(items[0].result.includes("+++ b/file"));
  assert.ok(!items[0].result.includes("--- a/\n"));
});

test("tool_call/result pairing is scoped by target generation, not raw toolId alone", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 910,
      target: { generation: 1 },
      item: {
        kind: "tool_call",
        tool: {
          toolId: "shared-tool-id",
          toolName: "bash",
          input: { command: "echo one" },
        },
      },
    }),
    envelope({
      eventSeq: 911,
      target: { generation: 2 },
      item: {
        kind: "tool_call",
        tool: {
          toolId: "shared-tool-id",
          toolName: "bash",
          input: { command: "echo two" },
        },
      },
    }),
    envelope({
      eventSeq: 912,
      target: { generation: 1 },
      item: {
        kind: "tool_result",
        toolId: "shared-tool-id",
        content: "one-result",
      },
    }),
    envelope({
      eventSeq: 913,
      target: { generation: 2 },
      item: {
        kind: "tool_result",
        toolId: "shared-tool-id",
        content: "two-result",
      },
    }),
  ]);

  assert.equal(items.length, 2);
  assert.equal(items[0].args.command, "echo one");
  assert.equal(items[0].result, "one-result");
  assert.equal(items[1].args.command, "echo two");
  assert.equal(items[1].result, "two-result");
  assert.notEqual(items[0].sessionId, items[1].sessionId);
});

test("a failed tool_result marks the paired card failed rather than completed", () => {
  const [item] = projectCodingSessionTranscript([
    envelope({
      eventSeq: 914,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "fail-1",
          toolName: "shell",
          input: { command: "false" },
        },
      },
    }),
    envelope({
      eventSeq: 915,
      item: {
        kind: "tool_result",
        toolId: "fail-1",
        content: "exit 1",
        isError: true,
      },
    }),
  ]);
  assert.equal(item.status, "failed");
  assert.equal(item.isError, true);
  assert.equal(item.renderClass, "error");
});

// --- bounds ---------------------------------------------------------------

test("quarantine-classed items render as bounded, compact-renderable status only, no raw content leaks", () => {
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
        // A quarantine record must NEVER carry the original payload, but even
        // if a buggy upstream attached one, this adapter must not read it.
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
  assert.ok(
    result.text.length < 5_000,
    `expected bounded text, got ${result.text.length} chars`,
  );
  assert.ok(result.text.includes("truncated"));
});

test("an unrecognized-kind label is length-bounded even when the kind string is hostile-huge", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "k".repeat(20_000) } }),
  );
  assert.ok(result.title.length < 500);
  assert.ok(result.text.length < 5_000);
});

test("system_init array fields are capped in count, even with a hostile-huge array", () => {
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

// --- fork-amended item kinds ---------------------------------------------

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

test("a plan item with no text re-renders the checklist from entries in the same format", () => {
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

test("a plan item with neither text nor entries is still a plan card, not a throw", () => {
  for (const item of [
    { kind: "plan" },
    { kind: "plan", text: "   ", entries: "not-an-array" },
  ]) {
    const result = projectCodingSessionTranscriptItem(envelope({ item }));
    assert.equal(result.type, "plan");
    assert.equal(result.text, "");
  }
});

test("an elided item surfaces a visible placeholder carrying size and digest", () => {
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
  assert.equal(result.title, "Content elided");
  assert.ok(result.text.includes("reason: oversize"));
  assert.ok(result.text.includes("byteCount: 41235"));
  assert.ok(result.text.includes("sha256:deadbeef"));
});

test("an elided item with missing fields still renders bounded unknowns", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "elided" } }),
  );
  assert.ok(result.text.includes("reason: unknown"));
  assert.ok(result.text.includes("byteCount: unknown"));
  assert.ok(result.text.includes("contentDigest: unknown"));
});

test("reasoning is admitted into the thought lane — the fork's deliberate reversal of the donor ban", () => {
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

test("a reasoning item with a non-string text degrades to an empty thought", () => {
  const result = projectCodingSessionTranscriptItem(
    envelope({ item: { kind: "reasoning", text: { nested: "object" } } }),
  );
  assert.equal(result.type, "thought");
  assert.equal(result.text, "");
});

test("the donor's exit_plan_mode tool-call plan derivation still applies", () => {
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

// --- turn presentation ----------------------------------------------------

test("a producer-declared turnId always wins over the synthetic turn", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 1_200,
      turnId: "turn-a",
      item: { kind: "user_prompt", content: "Start" },
    }),
    envelope({
      eventSeq: 1_201,
      turnId: "turn-a",
      item: { kind: "assistant_text", text: "Working" },
    }),
    envelope({
      eventSeq: 1_202,
      turnId: "turn-b",
      item: { kind: "assistant_text", text: "Next turn" },
    }),
    envelope({ eventSeq: 1_203, turnId: null, item: { kind: "status" } }),
  ]);
  assert.deepEqual(
    items.map((item) => item.turnId),
    ["turn-a", "turn-a", "turn-b", undefined],
  );
  assert.equal(items[0].acpSource, "session/prompt:user");
});

test("prompts without a declared turn derive stable synthetic turns and real prompt bubbles", () => {
  const envelopes = [
    envelope({
      eventSeq: 1_100,
      item: { kind: "status", status: "preparing" },
    }),
    envelope({
      eventSeq: 1_101,
      item: { kind: "user_prompt", content: "Inspect the project" },
    }),
    envelope({
      eventSeq: 1_102,
      item: { kind: "assistant_text", text: "I will inspect it." },
    }),
    envelope({
      eventSeq: 1_103,
      item: { kind: "result", subtype: "success", result: "done" },
    }),
    envelope({ eventSeq: 1_104, item: { kind: "status", status: "idle" } }),
  ];

  const first = projectCodingSessionTranscript(envelopes);
  const replay = projectCodingSessionTranscript(envelopes);
  assert.equal(first.length, envelopes.length);
  assert.equal(first[0].turnId, undefined, "pre-prompt status stays orphaned");
  assert.equal(first[1].acpSource, "session/prompt:user");
  assert.ok(first[1].turnId, "a non-steered prompt starts a synthetic turn");
  assert.equal(first[2].turnId, first[1].turnId);
  assert.equal(first[3].turnId, first[1].turnId);
  assert.equal(first[4].turnId, undefined, "result closes the synthetic turn");
  assert.deepEqual(
    replay.map((item) => item.turnId),
    first.map((item) => item.turnId),
    "replaying immutable envelope identity must reproduce turn identity",
  );

  const blocks = buildTranscriptDisplayBlocks(first);
  assert.deepEqual(
    blocks.map((block) => block.kind),
    ["single", "turn", "single"],
  );
  assert.equal(blocks[1].turnId, first[1].turnId);
  assert.equal(blocks[1].segments[0].kind, "prompt");
  assert.equal(blocks[1].segments[0].user.text, "Inspect the project");
});

test("steered prompts stay inside the active synthetic turn and interruption closes it", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 1_110,
      item: { kind: "user_prompt", content: "Start" },
    }),
    envelope({
      eventSeq: 1_111,
      item: { kind: "assistant_text", text: "Working" },
    }),
    envelope({
      eventSeq: 1_112,
      item: { kind: "user_prompt", content: "Also check tests", steered: true },
    }),
    envelope({
      eventSeq: 1_113,
      item: { kind: "assistant_text", text: "Checking" },
    }),
    envelope({ eventSeq: 1_114, item: { kind: "interrupted" } }),
    envelope({ eventSeq: 1_115, item: { kind: "status", status: "idle" } }),
  ]);

  const turnId = items[0].turnId;
  assert.ok(turnId);
  assert.equal(items[0].acpSource, "session/prompt:user");
  assert.equal(items[2].acpSource, "session/steer:user");
  assert.deepEqual(
    items.slice(0, 5).map((item) => item.turnId),
    [turnId, turnId, turnId, turnId, turnId],
  );
  assert.equal(items[5].turnId, undefined);

  const [turnBlock, trailing] = buildTranscriptDisplayBlocks(items);
  assert.equal(turnBlock.kind, "turn");
  assert.equal(
    turnBlock.segments.filter((segment) => segment.kind === "prompt").length,
    2,
    "prompt and steer both render as prompt bubbles in one turn",
  );
  assert.equal(trailing.kind, "single");
});

test("synthetic turn state never crosses a target generation fence", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 1_130,
      target: { generation: 21 },
      item: { kind: "user_prompt", content: "generation one" },
    }),
    envelope({
      eventSeq: 1_131,
      target: { generation: 21 },
      item: { kind: "assistant_text", text: "inside generation one" },
    }),
    envelope({
      eventSeq: 1_132,
      target: { generation: 22 },
      item: { kind: "assistant_text", text: "orphan generation two" },
    }),
  ]);
  assert.ok(items[0].turnId);
  assert.equal(items[1].turnId, items[0].turnId);
  assert.equal(items[2].turnId, undefined);
  assert.notEqual(items[1].sessionId, items[2].sessionId);
});

test("projected per-turn tool bursts collapse through the real display grouper", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      eventSeq: 1_120,
      item: { kind: "user_prompt", content: "Inspect and verify" },
    }),
    envelope({
      eventSeq: 1_121,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "read-burst",
          toolName: "read_file",
          input: { path: "src/app.ts" },
        },
      },
    }),
    envelope({
      eventSeq: 1_122,
      item: {
        kind: "tool_result",
        toolId: "read-burst",
        content: "file contents",
      },
    }),
    envelope({
      eventSeq: 1_123,
      item: {
        kind: "tool_call",
        tool: {
          toolId: "shell-burst",
          toolName: "bash",
          input: { command: "pnpm test" },
        },
      },
    }),
    envelope({
      eventSeq: 1_124,
      item: { kind: "tool_result", toolId: "shell-burst", content: "ok" },
    }),
    envelope({
      eventSeq: 1_125,
      item: { kind: "result", subtype: "success", result: "verified" },
    }),
  ]);

  assert.equal(items.length, 4, "tool-call/result pairing is preserved");
  assert.ok(items.every((item) => item.turnId === items[0].turnId));
  const [block] = buildTranscriptDisplayBlocks(items);
  assert.equal(block.kind, "turn");
  const summary = block.segments.find((segment) => segment.kind === "summary");
  assert.ok(
    summary,
    "adjacent projected tools collapse into a supervision row",
  );
  assert.equal(summary.summary.variant, "mixed");
  assert.equal(summary.summary.count, 2);
  assert.equal(summary.summary.label, "Ran 2 tool calls");
});

test("output items pass through buildTranscriptDisplayBlocks and group by generation", () => {
  const items = projectCodingSessionTranscript([
    envelope({
      target: { generation: 1 },
      item: { kind: "user_prompt", content: "hi" },
    }),
    envelope({
      target: { generation: 1 },
      item: { kind: "assistant_text", text: "hello" },
    }),
    envelope({
      target: { generation: 2 },
      item: { kind: "user_prompt", content: "again" },
    }),
  ]);

  let blocks;
  assert.doesNotThrow(() => {
    blocks = buildTranscriptDisplayBlocks(items, null);
  });
  assert.ok(blocks.length > 0);
  assert.ok(
    blocks.some((block) => block.kind === "session-boundary"),
    "a generation rebind should surface as a session boundary",
  );
});

test("an unknown/quarantine item actually renders in compact preview, not the empty state", () => {
  const items = projectCodingSessionTranscript([
    envelope({ item: { kind: "some_future_kind" } }),
    envelope({
      item: {
        schema: "seat-transcript-quarantine/v1",
        quarantineClass: "malformed_known_kind",
        decodeError: "malformed-shape",
      },
    }),
    envelope({ item: { kind: "elided", reason: "oversize" } }),
  ]);
  for (const item of items) {
    assert.ok(
      isCompactRenderable(item),
      `expected ${item.type}/${item.renderClass} to be compact-renderable`,
    );
  }
  assert.ok(buildTranscriptDisplayBlocks(items, null).length > 0);
});
