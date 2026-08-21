import assert from "node:assert/strict";
import test from "node:test";
import {
  parseChannelWindowResponse,
  parseLiveThreadSummary,
} from "./channelWindowResponse.ts";

// No lane of any session is openable unless a test says so; that is the
// production default too, and it keeps every lane-tagged message visible.
const NO_LANES = new Set();

function event(id, kind, createdAt, content = "", tags = []) {
  return {
    id: id.padEnd(64, "0"),
    pubkey: "a".repeat(64),
    created_at: createdAt,
    kind,
    tags: [["h", "channel"], ...tags],
    content,
    sig: "b".repeat(128),
  };
}

test("partitions flat rows, summaries, aux, and authoritative bounds", () => {
  const root = event("a", 9, 100);
  const summary = event(
    "s",
    39005,
    200,
    JSON.stringify({
      reply_count: 2,
      descendant_count: 3,
      last_reply_at: 150,
      participants: ["c".repeat(64)],
    }),
    [
      ["e", root.id],
      ["d", root.id],
    ],
  );
  const aux = event("x", 7, 300, "+", [["e", root.id]]);
  const bounds = event(
    "b",
    39006,
    400,
    JSON.stringify({
      has_more: true,
      next_cursor: { created_at: 100, id: root.id },
    }),
    [["d", "channel:head"]],
  );
  const page = parseChannelWindowResponse(
    [summary, aux, bounds, root],
    "channel",
    null,
    NO_LANES,
  );
  assert.equal(page.rows.length, 1);
  assert.equal(page.rows[0].thread.replyCount, 2);
  assert.deepEqual(
    page.aux.map((item) => item.id),
    [aux.id],
  );
  assert.deepEqual(page.nextCursor, { createdAt: 100, eventId: root.id });
  assert.equal(page.hasMore, true);
});

test("metadata timestamps never influence row cursor math", () => {
  const root = event("a", 9, 100);
  const bounds = event(
    "b",
    39006,
    9999,
    JSON.stringify({ has_more: false, next_cursor: null }),
    [["d", "channel:head"]],
  );
  const page = parseChannelWindowResponse(
    [bounds, root],
    "channel",
    null,
    NO_LANES,
  );
  assert.equal(page.nextCursor, null);
  assert.deepEqual(
    page.rows.map((row) => row.event.id),
    [root.id],
  );
});

test("rejects bounds signed for a different request cursor", () => {
  const root = event("a", 9, 100);
  const cursor = { createdAt: 200, eventId: "c".repeat(64) };
  const bounds = event(
    "b",
    39006,
    300,
    JSON.stringify({ has_more: false, next_cursor: null }),
    [["d", `channel:${cursor.createdAt}:${"d".repeat(64)}`]],
  );

  assert.throws(
    () =>
      parseChannelWindowResponse([root, bounds], "channel", cursor, NO_LANES),
    /do not match the request cursor/,
  );
});

test("accepts canonical composite request cursor binding", () => {
  const root = event("a", 9, 100);
  const cursor = { createdAt: 200, eventId: "C".repeat(64) };
  const bounds = event(
    "b",
    39006,
    300,
    JSON.stringify({ has_more: false, next_cursor: null }),
    [["d", `channel:200:${"c".repeat(64)}`]],
  );

  assert.doesNotThrow(() =>
    parseChannelWindowResponse([root, bounds], "CHANNEL", cursor, NO_LANES),
  );
});

test("rejects absent or contradictory signed bounds", () => {
  const root = event("a", 9, 100);
  assert.throws(
    () => parseChannelWindowResponse([root], "channel", null, NO_LANES),
    /exactly one bounds/,
  );
  const bad = event(
    "b",
    39006,
    200,
    JSON.stringify({ has_more: true, next_cursor: null }),
    [["d", "channel:head"]],
  );
  assert.throws(
    () => parseChannelWindowResponse([root, bad], "channel", null, NO_LANES),
    /disagree/,
  );
});

test("parses a relay-pushed live thread summary", () => {
  const rootId = "a".padEnd(64, "0");
  const push = event(
    "s",
    39005,
    700,
    JSON.stringify({
      reply_count: 4,
      descendant_count: 6,
      last_reply_at: 650,
      participants: ["c".repeat(64)],
    }),
    [
      ["e", rootId],
      ["d", rootId],
    ],
  );
  const parsed = parseLiveThreadSummary(push);
  assert.equal(parsed.rootId, rootId);
  assert.equal(parsed.live.createdAt, 700);
  assert.deepEqual(parsed.live.summary, {
    replyCount: 4,
    descendantCount: 6,
    lastReplyAt: 650,
    participantPubkeys: ["c".repeat(64)],
  });
});

test("drops malformed or mistargeted live thread summaries", () => {
  const rootId = "a".padEnd(64, "0");
  // Wrong kind.
  assert.equal(parseLiveThreadSummary(event("x", 9, 700, "hi")), null);
  // No e-tag root.
  assert.equal(parseLiveThreadSummary(event("s", 39005, 700, "{}")), null);
  // Unparseable content only skips one refresh instead of throwing.
  assert.equal(
    parseLiveThreadSummary(event("s", 39005, 700, "not json", [["e", rootId]])),
    null,
  );
});

test("only a lane this client can open suppresses its kind:9 from timeline rows", () => {
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const unknownRef = "6c8f2d3b-a1e5-4c1f-b2a4-8d3e9f7a5b21";
  const chat = event("a", 9, 100, "ordinary chat");
  const lane = event("l", 9, 110, "session-scoped chat", [
    ["cs-session", sessionRef],
  ]);
  // A ref this client cannot scope must degrade to visible chat, not vanish.
  const malformedLane = event("m", 9, 120, "degraded chat", [
    ["cs-session", "NOT-A-UUID"],
  ]);
  // A well-formed ref naming no lane we can open is the injection case: any
  // member could otherwise hide a channel message behind a random UUID.
  const unknownLane = event("u", 9, 130, "not really a lane", [
    ["cs-session", unknownRef],
  ]);
  const bounds = event(
    "b",
    39006,
    400,
    JSON.stringify({ has_more: false, next_cursor: null }),
    [["d", "channel:head"]],
  );
  const events = [chat, lane, malformedLane, unknownLane, bounds];

  assert.deepEqual(
    parseChannelWindowResponse(events, "channel", null, NO_LANES).rows.map(
      (row) => row.event.id,
    ),
    [chat.id, lane.id, malformedLane.id, unknownLane.id],
  );
  assert.deepEqual(
    parseChannelWindowResponse(
      events,
      "channel",
      null,
      new Set([sessionRef]),
    ).rows.map((row) => row.event.id),
    [chat.id, malformedLane.id, unknownLane.id],
  );
});
