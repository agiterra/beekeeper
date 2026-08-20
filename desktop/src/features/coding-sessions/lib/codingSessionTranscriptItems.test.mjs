import assert from "node:assert/strict";
import test from "node:test";

import {
  buildBaseTranscriptItem,
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
