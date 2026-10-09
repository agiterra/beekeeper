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
          beekeeperToolName: null,
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

/** A block whose items run past its start; `timestampMs` stays the start. */
function blockAt(startMs, itemTimestamps, { completed = false } = {}) {
  const items = itemTimestamps.map((iso, index) =>
    item(`item-${index}`, "message", { role: "assistant", timestamp: iso }),
  );
  if (completed) {
    items.push(
      item("result", "lifecycle", {
        renderClass: "status",
        title: "Turn result",
        timestamp: itemTimestamps[itemTimestamps.length - 1],
      }),
    );
  }
  return { ...block(items), timestampMs: startMs };
}

test("A3.1: an open turn block sorts at its newest item, so a row minted during it lands above", () => {
  // Block opens at 100s, its newest item lands at 140s; the assignment is
  // signed at 120s — during the turn.
  const open = blockAt(100_000, [
    "2026-09-01T00:01:40.000Z",
    "2026-09-01T00:02:20.000Z",
  ]);
  const merged = projectCodingSessionMissionTimeline([open], "live", [
    row("assignment", 120),
  ]);
  assert.deepEqual(
    merged.map((entry) => entry.kind),
    ["transaction", "turn-block"],
  );
});

test("A3.1: once the block closes it drops back to start order", () => {
  const settled = blockAt(
    100_000,
    ["2026-09-01T00:01:40.000Z", "2026-09-01T00:02:20.000Z"],
    { completed: true },
  );
  const merged = projectCodingSessionMissionTimeline([settled], "live", [
    row("assignment", 120),
  ]);
  assert.deepEqual(
    merged.map((entry) => entry.kind),
    ["turn-block", "transaction"],
  );
});

test("A3.1: transaction rows and non-block entries keep their own start", () => {
  const merged = projectCodingSessionMissionTimeline(
    [conversationAt("early", 100_000)],
    "live",
    [row("later", 120)],
  );
  assert.deepEqual(
    merged.map((entry) => entry.kind),
    ["conversation", "transaction"],
  );
});

/** A named block for one seat, so a multi-seat stream can be read as a list. */
function seatBlock(executionKey, startSeconds, itemSeconds, { open } = {}) {
  const iso = (seconds) => new Date(seconds * 1_000).toISOString();
  const items = itemSeconds.map((seconds, index) =>
    item(`${executionKey}-${index}`, "message", {
      role: "assistant",
      timestamp: iso(seconds),
    }),
  );
  if (!open) {
    items.push(
      item(`${executionKey}-result`, "lifecycle", {
        renderClass: "status",
        title: "Turn result",
        timestamp: iso(itemSeconds[itemSeconds.length - 1]),
      }),
    );
  }
  return {
    ...block(items),
    executionKey,
    generationId: executionKey,
    timestampMs: startSeconds * 1_000,
  };
}

/** `block:lead` / `tx:assignment`, so an assertion reads like the stream. */
function streamNames(merged) {
  return merged.map((entry) =>
    entry.kind === "transaction"
      ? `tx:${entry.row.meta.sourceEventId}`
      : entry.kind === "turn-block"
        ? `block:${entry.executionKey}`
        : entry.kind,
  );
}

/**
 * F1: the merge is a two-pointer walk over two lists that must both be sorted
 * by the key it compares. A3.1 made `entrySeconds` non-monotonic over the
 * narrative list, and a merge over a non-monotonic list compares every row
 * against the first entry only — so one open block early in the stream hoisted
 * every later transaction above every entry after it.
 */
test("A3.1/F1: one open block does not hoist later rows above settled blocks", () => {
  const entries = [
    seatBlock("lead", 60, [60, 540], { open: true }),
    seatBlock("bob1", 120, [120]),
    seatBlock("bob2", 300, [300]),
  ];
  const rows = [row("a", 180), row("b", 360)];
  assert.deepEqual(
    streamNames(projectCodingSessionMissionTimeline(entries, "live", rows)),
    // The open block floats to its newest item (540s) — A3.1's whole rule —
    // and everything settled keeps the order the wire produced.
    ["block:bob1", "tx:a", "block:bob2", "tx:b", "block:lead"],
  );
});

test("A3.1/F1: a settled block does not move when a different seat's block settles", () => {
  const rows = [row("a", 180), row("b", 360)];
  const settledOthers = ["block:bob1", "tx:a", "block:bob2", "tx:b"];
  const whileOpen = streamNames(
    projectCodingSessionMissionTimeline(
      [
        seatBlock("lead", 60, [60, 540], { open: true }),
        seatBlock("bob1", 120, [120]),
        seatBlock("bob2", 300, [300]),
      ],
      "live",
      rows,
    ),
  );
  const afterSettle = streamNames(
    projectCodingSessionMissionTimeline(
      [
        seatBlock("lead", 60, [60, 540]),
        seatBlock("bob1", 120, [120]),
        seatBlock("bob2", 300, [300]),
      ],
      "live",
      rows,
    ),
  );
  // Only the block that settled moves. Everything else holds its place.
  assert.deepEqual(
    whileOpen.filter((name) => name !== "block:lead"),
    settledOthers,
  );
  assert.deepEqual(
    afterSettle.filter((name) => name !== "block:lead"),
    settledOthers,
  );
  assert.deepEqual(afterSettle, ["block:lead", ...settledOthers]);
});

test("A3.1/F1: ties keep the narrative's own clamped order (stable sort)", () => {
  const entries = [
    seatBlock("first", 100, [100]),
    seatBlock("second", 100, [100]),
    seatBlock("third", 100, [100]),
  ];
  assert.deepEqual(
    streamNames(
      projectCodingSessionMissionTimeline(entries, "live", [row("z", 900)]),
    ),
    ["block:first", "block:second", "block:third", "tx:z"],
  );
});
