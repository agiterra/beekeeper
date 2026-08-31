import assert from "node:assert/strict";
import test from "node:test";

import {
  buildBaseTranscriptItem,
  buildPairedToolResultItem,
  CODING_SESSION_CONTINUITY_REASONS,
  CODING_SESSION_CONTINUITY_STATUSES,
} from "./codingSessionTranscriptItems.ts";

const IDENTITY = {
  id: "item-1",
  sessionId: "session-1",
  targetKey: "target-1",
  channelId: "channel-1",
  timestamp: "2026-08-19T00:00:00.000Z",
};

function statusItem(status, reason) {
  const item = { kind: "status", status };
  if (reason !== undefined) {
    item.reason = reason;
  }
  return item;
}

function render(status, reason) {
  return buildBaseTranscriptItem(statusItem(status, reason), IDENTITY);
}

test("a fresh status with a known reason renders the reason clause", () => {
  const item = render("session_fresh", "no_prior_execution");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("no_prior_execution");
  assert.equal(item.title, "Session continuity");
  assert.equal(item.text, `${base} — ${clause}`);
});

test("a restarted-without-context status with a known reason renders the reason clause", () => {
  const item = render("session_restarted_without_context", "relay_unavailable");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get(
    "session_restarted_without_context",
  );
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("relay_unavailable");
  assert.equal(item.title, "Session continuity");
  assert.equal(item.text, `${base} — ${clause}`);
});

test("the resume-path umbrella slug never claims a first execution", () => {
  const item = render(
    "session_restarted_without_context",
    "no_umbrella_context",
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get(
    "session_restarted_without_context",
  );
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("no_umbrella_context");
  assert.equal(item.text, `${base} — ${clause}`);
  assert.ok(
    !item.text.includes("first execution"),
    "a resumed execution has prior work; the row must not deny it",
  );
});

test("a fresh status with an unknown reason renders the raw slug", () => {
  const item = render(
    "session_fresh",
    "some_future_slug_this_build_has_never_seen",
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(
    item.text,
    `${base} (some_future_slug_this_build_has_never_seen)`,
  );
});

test("a status item with no reason renders exactly as before", () => {
  const withoutReasonField = render("session_fresh", undefined);
  const withExplicitUndefined = buildBaseTranscriptItem(
    { kind: "status", status: "session_fresh" },
    IDENTITY,
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(withoutReasonField.text, base);
  assert.equal(withExplicitUndefined.text, base);
});

test("every reason clause in the map is exercised by at least one known slug", () => {
  for (const [slug, clause] of CODING_SESSION_CONTINUITY_REASONS) {
    const item = render("session_fresh", slug);
    const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
    assert.equal(item.text, `${base} — ${clause}`);
  }
});

test("a non-reason-carrying continuity status ignores a reason field", () => {
  const item = render("session_rehydrated", "no_prior_execution");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_rehydrated");
  assert.equal(item.text, base);
});

test("a non-string reason is treated as absent", () => {
  const item = render("session_fresh", 42);
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(item.text, base);
});

test("an unrecognized status is unaffected by CODING_SESSION_CONTINUITY_REASONS", () => {
  const item = render("some_unrecognized_status", "no_prior_execution");
  assert.equal(item.title, "Status");
  assert.equal(item.text, "some_unrecognized_status");
});

// ── Per-turn usage on the wire ───────────────────────────────────────────────
//
// The provider now stamps an additive `usage` block on the terminal `result`
// item. This observer must accept it and keep projecting the turn: a reader
// that rejected the item on an unknown field would blank the end of every turn
// the moment the provider started measuring context.

test("a result item carrying a usage block still projects", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1000,
      result: "completed",
      inputTokens: 101200,
      outputTokens: 340,
      usage: {
        inputTokens: 1200,
        outputTokens: 340,
        cacheReadTokens: 96000,
        cacheWriteTokens: 4000,
        toolCalls: 7,
        contextWindow: 1000000,
      },
    },
    IDENTITY,
  );
  assert.equal(item.title, "Turn result");
  assert.equal(item.text, "completed");
  assert.equal(item.unknownKind, undefined);
});

test("a result item with no usage block projects exactly as before", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1000,
      result: "completed",
    },
    IDENTITY,
  );
  assert.equal(item.title, "Turn result");
  assert.equal(item.text, "completed");
});

test("a paired edit keeps the result's final args, discriminant, and paths", () => {
  const paired = buildPairedToolResultItem(
    {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolId: "edit-1",
        input: {},
      },
    },
    IDENTITY,
    {
      kind: "tool_result",
      toolId: "edit-1",
      toolName: "Edit",
      toolKind: "edit",
      input: { file_path: "desktop/src/App.tsx" },
      edit: { paths: ["desktop/src/App.tsx"] },
      content: "updated",
      isError: false,
    },
    { ...IDENTITY, id: "item-2" },
  );

  assert.equal(paired.toolKind, "edit");
  assert.deepEqual(paired.args, { file_path: "desktop/src/App.tsx" });
  assert.deepEqual(paired.editPaths, ["desktop/src/App.tsx"]);
});

test("a context_window_updated item carrying the driver's occupancy still projects", () => {
  const item = buildBaseTranscriptItem(
    { kind: "context_window_updated", usage: { size: 1000000, used: 137498 } },
    IDENTITY,
  );
  assert.equal(item.unknownKind, undefined);
  assert.ok(
    JSON.stringify(item).includes("137498"),
    `the occupancy the driver reported must survive projection: ${JSON.stringify(item)}`,
  );
});
