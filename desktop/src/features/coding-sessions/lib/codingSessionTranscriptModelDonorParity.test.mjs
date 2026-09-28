/**
 * H-01a characterization — the donor transcript visibility law (Hive
 * `src/client/app/KannaTranscript.tsx` `shouldRender`/grouping at pin
 * `e0b8198bd144`) run against Buzz's canonical model-layer suppression in
 * `codingSessionTranscriptModel.ts`, plus the module's uncovered formatting
 * exports. Buzz code is canonical: agreements are frozen, divergences are
 * asserted Buzz-side with the donor expectation recorded beside them.
 *
 * Scope: strictly the donor-discriminating edges — single-init suppression,
 * echo-based result dedup, and prefix grouping are already frozen by the
 * existing `codingSessionTranscriptModel` suite and are not re-asserted at
 * their existing vectors. No-seam disposition (recorded in
 * `conformance/session-ui-projections/CONTRACT.md`): the donor's
 * todo_write latest-only visibility law — Buzz has no TodoWrite concept in
 * this model, so the law has no seam here.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionTranscriptModel,
  formatCodingSessionCompletionOutcome,
  formatCodingSessionCost,
  formatCodingSessionCostBasis,
  isCompletedSuccessfulCodingSessionTool,
} from "./codingSessionTranscriptModel.ts";

const timestamp = "2026-07-30T12:00:00.000Z";

function message({ id, role, text, turnId = "turn-1" }) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Andy" : "Assistant",
    text,
    timestamp,
    turnId,
  };
}

function lifecycle({
  id,
  title,
  text,
  renderClass = "status",
  turnId = "turn-1",
}) {
  return {
    id,
    type: "lifecycle",
    renderClass,
    title,
    text,
    timestamp,
    turnId,
  };
}

function tool({
  id,
  toolName = "Bash",
  renderClass = "shell",
  turnId = "turn-1",
}) {
  return {
    id,
    type: "tool",
    renderClass,
    descriptor: { renderClass, label: "Ran command", preview: `preview-${id}` },
    title: toolName,
    toolName,
    buzzToolName: null,
    status: "completed",
    args: { command: `command-${id}` },
    result: "ok",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId,
  };
}

const entryIds = (turn) =>
  turn.entries.map((entry) =>
    entry.kind === "item" ? entry.item.id : entry.id,
  );

test("divergence (Buzz canonical): every system init is omitted, the donor renders only the first", () => {
  // Donor law: only the FIRST system_init index renders; a second lifecycle
  // would still show one row. Buzz drops system-init metadata entirely —
  // none reach entries or diagnostics, however many arrive.
  const model = deriveCodingSessionTranscriptModel(
    [
      lifecycle({ id: "init-1", title: "System Init", text: "claude" }),
      lifecycle({ id: "init-2", title: "System Init", text: "claude again" }),
      message({ id: "prompt", role: "user", text: "hello" }),
      message({ id: "reply", role: "assistant", text: "hi" }),
    ],
    { isWorking: false },
  );
  assert.equal(model.blocks.length, 1);
  assert.deepEqual(entryIds(model.blocks[0]), ["prompt", "reply"]);
  assert.deepEqual(model.diagnostics, []);
});

test("divergence (Buzz canonical): result visibility keys on echo equality, the donor keys on duration", () => {
  // Donor law: a successful result row is hidden iff durationMs <= 60000
  // (and shown past 60s); context_cleared adjacency also hides it — no Buzz
  // analogue for either. Buzz keys on echo equality with the turn's
  // assistant prose, duration-independent, and keeps duration/cost as the
  // turn completion either way.
  const echoed = deriveCodingSessionTranscriptModel(
    [
      message({
        id: "reply",
        role: "assistant",
        text: "The relay is healthy.",
      }),
      lifecycle({
        id: "settle",
        title: "Turn result",
        text: "The relay is healthy. (61000ms)",
      }),
    ],
    { isWorking: false },
  );
  const echoedTurn = echoed.blocks[0];
  // Donor would RENDER this row (61s > 60s success); Buzz suppresses the
  // echo and keeps only the assistant message.
  assert.deepEqual(entryIds(echoedTurn), ["reply"]);
  assert.equal(echoedTurn.completion.durationMs, 61000);
  assert.equal(echoedTurn.completion.state, "completed");

  const fresh = deriveCodingSessionTranscriptModel(
    [
      message({ id: "reply", role: "assistant", text: "Working on it." }),
      lifecycle({
        id: "settle",
        title: "Turn result",
        text: "Deployed the fix. (2000ms)",
      }),
    ],
    { isWorking: false },
  );
  const freshTurn = fresh.blocks[0];
  // Donor would HIDE this row (2s success); Buzz promotes the un-echoed body
  // to a synthesized assistant message.
  assert.deepEqual(entryIds(freshTurn), ["reply", "settle:assistant-result"]);
  assert.equal(freshTurn.completion.durationMs, 2000);
});

test("agreement: completion outcome noise values suppress like the donor's successful-result rows", () => {
  // Donor hides ceremony (short successful results); Buzz hides ceremony
  // outcomes — the closed 7-value noise list returns null, real outcomes
  // surface lowercased with _- runs as spaces.
  const noise = [
    "completed",
    "error",
    "failed",
    "interrupted",
    "result",
    "success",
    "unknown",
  ];
  for (const outcome of noise) {
    assert.equal(
      formatCodingSessionCompletionOutcome({ outcome }),
      null,
      outcome,
    );
    assert.equal(
      formatCodingSessionCompletionOutcome({
        outcome: ` ${outcome.toUpperCase()} `,
      }),
      null,
      outcome,
    );
  }
  assert.equal(formatCodingSessionCompletionOutcome({ outcome: "" }), null);
  assert.equal(formatCodingSessionCompletionOutcome({ outcome: "   " }), null);
  assert.equal(formatCodingSessionCompletionOutcome({}), null);
  assert.equal(
    formatCodingSessionCompletionOutcome({ outcome: "tool_use-limit_reached" }),
    "tool use limit reached",
  );
  assert.equal(
    formatCodingSessionCompletionOutcome({ outcome: "Max-Turns" }),
    "max turns",
  );
});

test("characterization: cost formats four decimals under a cent, else two", () => {
  // Donor renders the raw `($cost)` suffix from the result row unformatted.
  assert.equal(formatCodingSessionCost(0), "$0.0000");
  assert.equal(formatCodingSessionCost(0.0001), "$0.0001");
  assert.equal(formatCodingSessionCost(0.005), "$0.0050");
  assert.equal(formatCodingSessionCost(0.0099), "$0.0099");
  assert.equal(formatCodingSessionCost(0.01), "$0.01");
  assert.equal(formatCodingSessionCost(0.5), "$0.50");
  assert.equal(formatCodingSessionCost(1.234), "$1.23");
  assert.equal(formatCodingSessionCost(12), "$12.00");
  // Ledger 272(d): no figure is an invoice, and each says whose it is.
  assert.equal(
    formatCodingSessionCostBasis("adapter_estimate"),
    "adapter estimate",
  );
  assert.equal(
    formatCodingSessionCostBasis("table_estimate"),
    "price-table estimate",
  );
  assert.equal(formatCodingSessionCostBasis(null), "estimate");
});

test("characterization: only settled successful tools satisfy the completed-successful predicate", () => {
  const success = tool({ id: "ok" });
  assert.equal(isCompletedSuccessfulCodingSessionTool(success), true);
  assert.equal(
    isCompletedSuccessfulCodingSessionTool({ ...success, status: "executing" }),
    false,
  );
  assert.equal(
    isCompletedSuccessfulCodingSessionTool({ ...success, status: "failed" }),
    false,
  );
  assert.equal(
    isCompletedSuccessfulCodingSessionTool({ ...success, isError: true }),
    false,
  );
  assert.equal(
    isCompletedSuccessfulCodingSessionTool({
      ...success,
      renderClass: "error",
    }),
    false,
  );
  assert.equal(
    isCompletedSuccessfulCodingSessionTool(
      message({ id: "not-a-tool", role: "assistant", text: "hi" }),
    ),
    false,
  );
});

test("divergence (Buzz canonical): grouping keys on settled success, the donor exempts tools by name", () => {
  // Donor law: collapsible = tool && toolName NOT IN {AskUserQuestion,
  // ExitPlanMode, TodoWrite} — a NAME allowlist. Buzz has no name check:
  // any completed successful tool joins the prefix group, including one the
  // donor would always keep standalone.
  const model = deriveCodingSessionTranscriptModel(
    [
      message({ id: "prompt", role: "user", text: "go" }),
      tool({
        id: "ask",
        toolName: "AskUserQuestion",
        renderClass: "file-read",
      }),
      tool({ id: "run-1" }),
      tool({ id: "run-2" }),
      tool({ id: "run-3" }),
      tool({ id: "run-4" }),
    ],
    { isWorking: false },
  );
  const turn = model.blocks[0];
  assert.deepEqual(entryIds(turn), [
    "prompt",
    "tools:ask",
    "run-2",
    "run-3",
    "run-4",
  ]);
  const group = turn.entries[1];
  assert.equal(group.kind, "tool-group");
  assert.deepEqual(
    group.items.map((item) => item.id),
    ["ask", "run-1"],
  );
  // Mixed render classes fall back to the generic label.
  assert.equal(group.label, "Ran 2 tool calls");
});

test("characterization: tool group labels count a single render class by verb", () => {
  const derive = (ids, renderClass) =>
    deriveCodingSessionTranscriptModel(
      [
        message({ id: "prompt", role: "user", text: "go" }),
        ...ids.map((id) => tool({ id, renderClass })),
      ],
      { isWorking: false },
    ).blocks[0].entries[1];
  assert.equal(
    derive(["s1", "s2", "s3", "s4", "s5"], "shell").label,
    "Ran 2 commands",
  );
  assert.equal(
    derive(["r1", "r2", "r3", "r4", "r5"], "file-read").label,
    "Read 2 files",
  );
  assert.equal(
    derive(["e1", "e2", "e3", "e4", "e5"], "file-edit").label,
    "Edited 2 files",
  );
});
