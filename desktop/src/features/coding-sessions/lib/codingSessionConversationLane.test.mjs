import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionLaneFilter,
  buildCodingSessionLaneMessageEvent,
  codingSessionLaneRef,
  isCodingSessionLaneEventForChannel,
  isCodingSessionLaneMessage,
  MAX_CODING_SESSION_LANE_MESSAGE_BYTES,
  projectCodingSessionLaneMessages,
  shouldSuppressCodingSessionLaneMessageFromChannelTimeline,
} from "./codingSessionConversationLane.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_REF = "6c8f2d3b-a1e5-4c1f-b2a4-8d3e9f7a5b21";
const CHANNEL_ID = "channel-1";
const AGENT_PUBKEY = "d".repeat(64);

function laneEvent({
  id = "event-1",
  pubkey = "c".repeat(64),
  createdAt = 1_800_000_000,
  kind = 9,
  content = "Codex, take the failing test Claude found.",
  tags = [
    ["h", CHANNEL_ID],
    ["cs-session", SESSION_REF],
  ],
} = {}) {
  return { id, pubkey, created_at: createdAt, kind, tags, content };
}

test("a lane message is an ordinary kind:9 with the cs-session tag in fixed order", () => {
  const event = buildCodingSessionLaneMessageEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    content: "Take Claude's finding and regenerate the fixture.",
    mentionPubkeys: [AGENT_PUBKEY],
  });
  assert.equal(event.kind, 9);
  assert.deepEqual(event.tags, [
    ["h", CHANNEL_ID],
    ["cs-session", SESSION_REF],
    ["p", AGENT_PUBKEY],
  ]);
});

test("lane message inputs are validated, not coerced", () => {
  const valid = {
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    content: "hello",
  };
  for (const invalid of [
    { ...valid, channelId: " " },
    { ...valid, sessionRef: "not-a-uuid" },
    { ...valid, sessionRef: SESSION_REF.toUpperCase() },
    { ...valid, content: "  " },
    { ...valid, content: "é".repeat(MAX_CODING_SESSION_LANE_MESSAGE_BYTES) },
    { ...valid, mentionPubkeys: ["D".repeat(64)] },
    { ...valid, mentionPubkeys: ["short"] },
  ]) {
    assert.throws(() => buildCodingSessionLaneMessageEvent(invalid));
  }
  assert.doesNotThrow(() =>
    buildCodingSessionLaneMessageEvent({
      ...valid,
      content: "é".repeat(MAX_CODING_SESSION_LANE_MESSAGE_BYTES / 2),
    }),
  );
});

test("the lane ref resolves only for a kind:9 with exactly one well-formed cs-session value", () => {
  assert.equal(codingSessionLaneRef(laneEvent()), SESSION_REF);
  // Duplicated identical tags are one claim.
  assert.equal(
    codingSessionLaneRef(
      laneEvent({
        tags: [
          ["h", CHANNEL_ID],
          ["cs-session", SESSION_REF],
          ["cs-session", SESSION_REF],
        ],
      }),
    ),
    SESSION_REF,
  );
  // Ambiguous, malformed, or wrong-kind events are not lane messages.
  for (const event of [
    laneEvent({ kind: 40002 }),
    laneEvent({ tags: [["h", CHANNEL_ID]] }),
    laneEvent({
      tags: [
        ["h", CHANNEL_ID],
        ["cs-session", SESSION_REF],
        ["cs-session", OTHER_REF],
      ],
    }),
    laneEvent({ tags: [["cs-session", "not-a-uuid"]] }),
    laneEvent({ tags: [["cs-session", SESSION_REF.toUpperCase()]] }),
    laneEvent({ tags: [["cs-session"]] }),
  ]) {
    assert.equal(codingSessionLaneRef(event), null);
  }
});

test("suppression hides a lane message only when its lane is one this client can open", () => {
  const known = new Set([SESSION_REF]);
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent(),
      known,
    ),
    true,
  );
  // A resolver callback is equivalent to the set form.
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent(),
      (ref) => ref === SESSION_REF,
    ),
    true,
  );
  // Ordinary chat stays.
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent({ tags: [["h", CHANNEL_ID]] }),
      known,
    ),
    false,
  );
  // A malformed lane tag degrades to visible, attributable chat rather than
  // vanishing from both surfaces.
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent({ tags: [["cs-session", "not-a-uuid"]] }),
      known,
    ),
    false,
  );
  // The hidden-message hole: a well-formed ref this client cannot resolve to
  // an openable lane must stay visible, or any member could hide a channel
  // message by tagging it with an arbitrary valid UUID.
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent({ tags: [["cs-session", OTHER_REF]] }),
      known,
    ),
    false,
  );
  assert.equal(
    shouldSuppressCodingSessionLaneMessageFromChannelTimeline(
      laneEvent(),
      new Set(),
    ),
    false,
  );
});

test("lane admission requires the exact session ref and this channel's h tag", () => {
  assert.equal(
    isCodingSessionLaneEventForChannel(laneEvent(), CHANNEL_ID, SESSION_REF),
    true,
  );
  // A relay answering `#cs-session` loosely cannot place another channel's
  // chat — or a ref-less event — in this lane.
  assert.equal(
    isCodingSessionLaneEventForChannel(
      laneEvent({
        tags: [
          ["h", "channel-2"],
          ["cs-session", SESSION_REF],
        ],
      }),
      CHANNEL_ID,
      SESSION_REF,
    ),
    false,
  );
  assert.equal(
    isCodingSessionLaneEventForChannel(
      laneEvent({ tags: [["cs-session", SESSION_REF]] }),
      CHANNEL_ID,
      SESSION_REF,
    ),
    false,
  );
  assert.equal(
    isCodingSessionLaneEventForChannel(laneEvent(), CHANNEL_ID, OTHER_REF),
    false,
  );
});

test("membership check requires both a valid target ref and an exact match", () => {
  assert.equal(isCodingSessionLaneMessage(laneEvent(), SESSION_REF), true);
  assert.equal(isCodingSessionLaneMessage(laneEvent(), OTHER_REF), false);
  assert.equal(isCodingSessionLaneMessage(laneEvent(), "not-a-uuid"), false);
});

test("the lane projection filters, orders by created_at then event id, and converts to milliseconds", () => {
  const events = [
    laneEvent({ id: "b", createdAt: 1_800_000_010 }),
    laneEvent({
      id: "a",
      createdAt: 1_800_000_010,
      tags: [["cs-session", SESSION_REF]],
    }),
    laneEvent({ id: "c", createdAt: 1_800_000_000 }),
    laneEvent({ id: "other-session", tags: [["cs-session", OTHER_REF]] }),
    laneEvent({ id: "plain-chat", tags: [["h", CHANNEL_ID]] }),
  ];
  const messages = projectCodingSessionLaneMessages(events, SESSION_REF);
  assert.deepEqual(
    messages.map((message) => message.eventId),
    ["c", "a", "b"],
  );
  assert.equal(messages[0].timestampMs, 1_800_000_000_000);
  assert.equal(messages[0].channelId, CHANNEL_ID);
  assert.equal(messages[0].sessionRef, SESSION_REF);
  assert.equal(messages[1].channelId, null);

  // An invalid target ref projects nothing rather than everything.
  assert.deepEqual(projectCodingSessionLaneMessages(events, "junk"), []);
});

test("the dedicated lane subscription filter always carries explicit kinds", () => {
  assert.deepEqual(buildCodingSessionLaneFilter(CHANNEL_ID, SESSION_REF), {
    kinds: [9],
    "#h": [CHANNEL_ID],
    "#cs-session": [SESSION_REF],
  });
});
