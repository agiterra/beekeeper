import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { ToolItem } from "@/features/agents/ui/AgentSessionToolItem/ToolItem.tsx";
import { ShellCommandBlock } from "@/features/agents/ui/AgentSessionToolItem/ShellCommandBlock.tsx";
import { ToolDetailBlocks } from "@/features/agents/ui/AgentSessionToolItem/ToolDetailBlocks.tsx";
import { formatToolOutputGapDetail } from "@/features/agents/ui/agentSessionUtils.ts";
import {
  readCodingSessionToolContentSource,
  readCodingSessionToolOutputGap,
} from "./codingSessionToolOutputCompleteness.ts";
import {
  buildBaseTranscriptItem,
  buildPairedToolResultItem,
} from "./codingSessionTranscriptItems.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

const IDENTITY = {
  id: "item-1",
  sessionId: "session-1",
  targetKey: "target-1",
  channelId: "channel-1",
  timestamp: "2026-10-03T00:00:00.000Z",
};

const INCOMPLETE = {
  contentSource: "streamed_deltas",
  outputComplete: false,
  outputGap: { streamedBytes: 1193, aggregatedBytes: 1793 },
};

function toolResult(extra = {}) {
  return {
    kind: "tool_result",
    toolId: "call-1",
    toolName: "bash",
    content: "tail of the output",
    ...extra,
  };
}

// ── parse ───────────────────────────────────────────────────────────────────

test("outputGap_incomplete_readsBothByteCounts", () => {
  assert.deepEqual(readCodingSessionToolOutputGap(toolResult(INCOMPLETE)), {
    streamedBytes: 1193,
    aggregatedBytes: 1793,
  });
});

test("outputGap_incompleteWithoutAggregate_keepsStreamedOnly", () => {
  assert.deepEqual(
    readCodingSessionToolOutputGap(
      toolResult({
        contentSource: "streamed_deltas",
        outputComplete: false,
        outputGap: { streamedBytes: 600 },
      }),
    ),
    { streamedBytes: 600 },
  );
});

test("outputGap_recoveredOrVerified_isNull", () => {
  for (const contentSource of ["native_rollout", "streamed_deltas"]) {
    assert.equal(
      readCodingSessionToolOutputGap(
        toolResult({ contentSource, outputComplete: true }),
      ),
      null,
    );
  }
});

test("outputGap_absentFields_isNull", () => {
  assert.equal(readCodingSessionToolOutputGap(toolResult()), null);
  assert.equal(readCodingSessionToolOutputGap(null), null);
  assert.equal(readCodingSessionToolOutputGap("x"), null);
});

test("outputGap_nonBooleanOutputComplete_isNoClaim", () => {
  for (const outputComplete of ["false", 0, null, undefined]) {
    assert.equal(
      readCodingSessionToolOutputGap(
        toolResult({ ...INCOMPLETE, outputComplete }),
      ),
      null,
    );
  }
});

test("outputGap_malformedGap_keepsNoticeDropsCounts", () => {
  for (const outputGap of [
    "1193",
    null,
    [],
    { streamedBytes: -1, aggregatedBytes: "1793" },
    { streamedBytes: Number.NaN },
    { streamedBytes: 1.5 },
  ]) {
    assert.deepEqual(
      readCodingSessionToolOutputGap(
        toolResult({ outputComplete: false, outputGap }),
      ),
      {},
    );
  }
});

test("outputGap_aggregateBelowCaptured_dropsAggregate", () => {
  assert.deepEqual(
    readCodingSessionToolOutputGap(
      toolResult({
        outputComplete: false,
        outputGap: { streamedBytes: 900, aggregatedBytes: 600 },
      }),
    ),
    { streamedBytes: 900 },
  );
});

test("contentSource_unknownValue_readsAsAbsent", () => {
  assert.equal(
    readCodingSessionToolContentSource("native_rollout"),
    "native_rollout",
  );
  assert.equal(
    readCodingSessionToolContentSource("streamed_deltas"),
    "streamed_deltas",
  );
  assert.equal(readCodingSessionToolContentSource("adapter_final"), null);
  assert.equal(readCodingSessionToolContentSource(undefined), null);
});

test("formatToolOutputGapDetail_formatsKnownCounts", () => {
  assert.equal(
    formatToolOutputGapDetail({ streamedBytes: 1193, aggregatedBytes: 1793 }),
    "captured 1,193 of 1,793 bytes",
  );
  assert.equal(
    formatToolOutputGapDetail({ streamedBytes: 600 }),
    "captured 600 bytes",
  );
  assert.equal(formatToolOutputGapDetail({}), null);
});

// ── projection ──────────────────────────────────────────────────────────────

test("projection_unpairedIncompleteResult_carriesOutputGap", () => {
  const item = buildBaseTranscriptItem(toolResult(INCOMPLETE), IDENTITY);
  assert.deepEqual(item.outputGap, {
    streamedBytes: 1193,
    aggregatedBytes: 1793,
  });
});

test("projection_completeRecoveredOrAbsent_hasNoOutputGapKey", () => {
  for (const extra of [
    {},
    { contentSource: "native_rollout", outputComplete: true },
    { contentSource: "streamed_deltas", outputComplete: true },
  ]) {
    const item = buildBaseTranscriptItem(toolResult(extra), IDENTITY);
    assert.equal("outputGap" in item, false);
  }
});

test("projection_pairedResult_takesGapFromTheResult", () => {
  const call = {
    kind: "tool_call",
    tool: { toolId: "call-1", toolName: "bash", input: { command: "ls" } },
  };
  const paired = buildPairedToolResultItem(
    call,
    IDENTITY,
    toolResult(INCOMPLETE),
    { ...IDENTITY, id: "item-2" },
  );
  assert.deepEqual(paired.outputGap, {
    streamedBytes: 1193,
    aggregatedBytes: 1793,
  });

  const [projected] = projectCodingSessionTranscript([
    {
      eventSeq: 1,
      timestamp: 1_700_000_000_001,
      target: {
        driver: "codex-acp",
        instanceId: "0123456789abcdef",
        sessionId: "11111111-2222-3333-4444-555555555555",
        generation: 1,
      },
      item: call,
    },
    {
      eventSeq: 2,
      timestamp: 1_700_000_000_002,
      target: {
        driver: "codex-acp",
        instanceId: "0123456789abcdef",
        sessionId: "11111111-2222-3333-4444-555555555555",
        generation: 1,
      },
      item: toolResult(INCOMPLETE),
    },
  ]);
  assert.deepEqual(projected.outputGap, {
    streamedBytes: 1193,
    aggregatedBytes: 1793,
  });
});

// ── rendering ───────────────────────────────────────────────────────────────

test("ShellCommandBlock_withGap_rendersNoticeAndCounts", () => {
  const html = renderToStaticMarkup(
    React.createElement(ShellCommandBlock, {
      command: "cargo test",
      isError: false,
      outputGap: { streamedBytes: 1193, aggregatedBytes: 1793 },
      result: "tail",
    }),
  );
  assert.match(html, /data-testid="transcript-tool-output-gap"/);
  assert.match(html, /Output may be missing its beginning/);
  assert.match(html, /captured 1,193 of 1,793 bytes/);
});

test("ShellCommandBlock_withoutGap_rendersNoNotice", () => {
  const html = renderToStaticMarkup(
    React.createElement(ShellCommandBlock, {
      command: "cargo test",
      isError: false,
      result: "all output",
    }),
  );
  assert.doesNotMatch(html, /Output may be missing/);
});

test("ToolDetailBlocks_genericResultWithGap_rendersNotice", () => {
  const html = renderToStaticMarkup(
    React.createElement(ToolDetailBlocks, {
      args: {},
      fileEditDiff: null,
      fileReadContent: null,
      hasArgs: false,
      hasResult: true,
      imagePreview: null,
      isError: false,
      outputGap: { streamedBytes: 600 },
      result: "tail",
      shellCommand: null,
    }),
  );
  assert.match(html, /Output may be missing its beginning/);
  assert.match(html, /captured 600 bytes/);
});

test("ToolItem_projectedIncompleteResult_marksRowAndDetails", () => {
  const item = buildBaseTranscriptItem(toolResult(INCOMPLETE), IDENTITY);
  const html = renderToStaticMarkup(
    React.createElement(ToolItem, {
      agentName: "codex",
      agentPubkey: "a".repeat(64),
      item,
    }),
  );
  assert.match(html, /data-testid="transcript-tool-output-gap-marker"/);
  assert.match(html, /Output may be missing its beginning/);

  const complete = buildBaseTranscriptItem(
    toolResult({ contentSource: "native_rollout", outputComplete: true }),
    IDENTITY,
  );
  const plain = renderToStaticMarkup(
    React.createElement(ToolItem, {
      agentName: "codex",
      agentPubkey: "a".repeat(64),
      item: complete,
    }),
  );
  assert.doesNotMatch(plain, /Output may be missing/);
});
