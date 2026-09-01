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

function row(id, createdAt, overrides = {}) {
  return {
    key: `transaction:${id}`,
    type: "report",
    weight: "standard",
    tone: null,
    actor: {
      pubkey: "b".repeat(64),
      label: "Bob",
      monogram: "B",
      accent: { dot: "", border: "", soft: "", text: "" },
    },
    counterparty: null,
    title: `row ${id}`,
    body: "",
    meta: { timeLabel: "5:05 PM", sourceEventId: id },
    accessibleLabel: `Bob: report ${id}`,
    decision: null,
    requiredAction: null,
    delivery: null,
    unseated: false,
    fileCount: null,
    testCount: null,
    createdAt,
    showSignedSource: false,
    ...overrides,
  };
}

function conversationAt(eventId, timestampMs) {
  return { kind: "conversation", message: { eventId }, timestampMs };
}

test("U-T4: transactions interleave chronologically with the narrative", () => {
  const merged = projectCodingSessionMissionTimeline(
    [conversationAt("first", 1_000), conversationAt("last", 9_000)],
    "live",
    [row("mid", 5), row("tail", 12)],
  );
  assert.deepEqual(
    merged.map((entry) =>
      entry.kind === "transaction"
        ? entry.row.meta.sourceEventId
        : entry.message.eventId,
    ),
    ["first", "mid", "last", "tail"],
  );
});

test("U-T4: a same-second tie is stable and breaks on the signed event id", () => {
  const merged = projectCodingSessionMissionTimeline(
    [conversationAt("narrative", 7_000)],
    "live",
    [row("zeta", 7), row("alpha", 7)],
  );
  assert.deepEqual(
    merged.map((entry) =>
      entry.kind === "transaction"
        ? entry.row.meta.sourceEventId
        : entry.message.eventId,
    ),
    ["narrative", "alpha", "zeta"],
  );
});

test("U-T4: past the bound one visible quiet row states what is not shown", () => {
  const rows = Array.from({ length: 203 }, (_, index) =>
    row(String(index).padStart(4, "0"), 1_000 + index),
  );
  const merged = projectCodingSessionMissionTimeline([], "live", rows);
  const truncation = merged.filter(
    (entry) => entry.kind === "transaction-truncation",
  );
  assert.equal(truncation.length, 1);
  assert.equal(truncation[0].hiddenCount, 3);
  assert.equal(
    merged.filter((entry) => entry.kind === "transaction").length,
    200,
  );
  assert.equal(merged[0].kind, "transaction-truncation");
});

test("U-T4: Brief keeps every transaction row while it thins execution noise", () => {
  const merged = projectCodingSessionMissionTimeline(
    [
      block([item("thought", "thought", { renderClass: "thought" })]),
      conversationAt("kept", 1),
    ],
    "brief",
    [row("t1", 1), row("t2", 2)],
  );
  assert.equal(
    merged.filter((entry) => entry.kind === "transaction").length,
    2,
  );
  assert.equal(merged.filter((entry) => entry.kind === "turn-block").length, 0);
});

test("U-T4: with no transactions the projection is exactly the old one", () => {
  const entries = [conversationAt("a", 1), conversationAt("b", 2)];
  assert.deepEqual(
    projectCodingSessionMissionTimeline(entries, "live"),
    entries,
  );
  assert.deepEqual(
    projectCodingSessionMissionTimeline(entries, "live", []),
    entries,
  );
});
