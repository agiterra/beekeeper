import assert from "node:assert/strict";
import test from "node:test";

import { parseChannelWindowResponse } from "./channelWindowResponse.ts";
import {
  codingSessionLaneObservedChannelIds,
  codingSessionLaneRenderableRefs,
  codingSessionLaneRenderableRefsFromUmbrellas,
  isCodingSessionLaneMessageHiddenFromChannel,
  observeCodingSessionLaneRefs,
  publishCodingSessionLaneRenderableRefs,
  resetCodingSessionLaneVisibility,
} from "./codingSessionLaneVisibility.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_REF = "6c8f2d3b-a1e5-4c1f-b2a4-8d3e9f7a5b21";
const CHANNEL_ID = "channel-1";

function laneEvent(sessionRef = SESSION_REF, channelId = CHANNEL_ID) {
  return {
    kind: 9,
    tags: [
      ["h", channelId],
      ["cs-session", sessionRef],
    ],
  };
}

const plainChat = { kind: 9, tags: [["h", CHANNEL_ID]] };

test.beforeEach(() => {
  resetCodingSessionLaneVisibility();
});

test("a lane message stays visible until its ref resolves to an openable lane", () => {
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, laneEvent()),
    false,
  );
  publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF]));
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, laneEvent()),
    true,
  );
  // Another member's forged ref, ordinary chat, and other channels are all
  // unaffected — hiding always means "renderable in a lane you can open".
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(
      CHANNEL_ID,
      laneEvent(OTHER_REF),
    ),
    false,
  );
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, plainChat),
    false,
  );
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel("channel-2", laneEvent()),
    false,
  );
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(null, laneEvent()),
    false,
  );
});

test("observation marks a channel worth resolving without making refs renderable", () => {
  assert.equal(observeCodingSessionLaneRefs(CHANNEL_ID, [plainChat]), false);
  assert.deepEqual(codingSessionLaneObservedChannelIds(), []);

  assert.equal(observeCodingSessionLaneRefs(CHANNEL_ID, [laneEvent()]), true);
  assert.deepEqual(codingSessionLaneObservedChannelIds(), [CHANNEL_ID]);
  // Observation alone never hides anything.
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, laneEvent()),
    false,
  );
  // Repeat observations are not new work.
  assert.equal(observeCodingSessionLaneRefs(CHANNEL_ID, [laneEvent()]), false);
  assert.equal(observeCodingSessionLaneRefs(null, [laneEvent()]), false);
});

test("publishing reports real changes so callers can re-project once", () => {
  assert.equal(
    publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF])),
    true,
  );
  assert.equal(
    publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF])),
    false,
  );
  assert.equal(
    publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set()),
    true,
  );
  assert.deepEqual([...codingSessionLaneRenderableRefs(CHANNEL_ID)], []);
});

test("only an umbrella that renders a lane contributes a renderable ref", () => {
  const refs = codingSessionLaneRenderableRefsFromUmbrellas([
    { sessionRef: SESSION_REF, executions: [{}, {}] },
    // An umbrella of one renders no lane, so its tagged chat stays in chat.
    { sessionRef: OTHER_REF, executions: [{}] },
    // Implicit (pre-Step-4) umbrellas have no lane at all.
    { sessionRef: null, executions: [{}, {}] },
  ]);
  assert.deepEqual([...refs], [SESSION_REF]);
});

test("unread and the channel timeline never disagree about a lane message", () => {
  // `useUnreadChannels` gates both its live handler and its catch-up pass on
  // `isCodingSessionLaneMessageHiddenFromChannel`, and the window parse gates
  // rows on the same resolved refs — so a message that stays in the timeline
  // still counts toward unread, and one that leaves it stops counting.
  const relayEvent = (id, tags) => ({
    id: id.padEnd(64, "0"),
    pubkey: "a".repeat(64),
    created_at: 100,
    kind: 9,
    tags: [["h", CHANNEL_ID], ...tags],
    content: id,
    sig: "b".repeat(128),
  });
  const chat = relayEvent("chat", []);
  const lane = relayEvent("lane", [["cs-session", SESSION_REF]]);
  const forged = relayEvent("forged", [["cs-session", OTHER_REF]]);
  const bounds = {
    ...relayEvent("bounds", [["d", `${CHANNEL_ID}:head`]]),
    kind: 39006,
    content: JSON.stringify({ has_more: false, next_cursor: null }),
  };
  const visibleIds = () =>
    parseChannelWindowResponse(
      [chat, lane, forged, bounds],
      CHANNEL_ID,
      null,
      codingSessionLaneRenderableRefs(CHANNEL_ID),
    ).rows.map((row) => row.event.id);
  const unreadIds = () =>
    [chat, lane, forged]
      .filter(
        (event) =>
          !isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, event),
      )
      .map((event) => event.id);

  assert.deepEqual(visibleIds(), [chat.id, lane.id, forged.id]);
  assert.deepEqual(unreadIds(), visibleIds());

  publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF]));
  assert.deepEqual(visibleIds(), [chat.id, forged.id]);
  assert.deepEqual(unreadIds(), visibleIds());
});

test("reset drops every channel's lane knowledge", () => {
  observeCodingSessionLaneRefs(CHANNEL_ID, [laneEvent()]);
  publishCodingSessionLaneRenderableRefs(CHANNEL_ID, new Set([SESSION_REF]));
  resetCodingSessionLaneVisibility();
  assert.deepEqual(codingSessionLaneObservedChannelIds(), []);
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(CHANNEL_ID, laneEvent()),
    false,
  );
});
