import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionGenesisEvent,
  publishCodingSessionGenesis,
} from "./codingSessionGenesis.ts";

const CHANNEL_ID = "channel-1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

test("fresh genesis uses the exact two-field payload and three-tag envelope", () => {
  const event = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  assert.equal(event.kind, 44226);
  assert.deepEqual(JSON.parse(event.content), {
    sessionRef: SESSION_REF,
    v: 1,
  });
  assert.deepEqual(event.tags, [
    ["h", CHANNEL_ID],
    ["csg-v", "csg1-1"],
    ["csg-session", SESSION_REF],
  ]);
});

test("genesis publication returns the relay-accepted event id", async () => {
  const order = [];
  const signed = {
    id: "a".repeat(64),
    pubkey: "b".repeat(64),
    created_at: 1,
    kind: 44226,
    tags: [],
    content: "",
    sig: "c".repeat(128),
  };
  const result = await publishCodingSessionGenesis(
    { channelId: CHANNEL_ID, sessionRef: SESSION_REF },
    {
      signer: async (input) => {
        order.push(["sign", input]);
        return signed;
      },
      publisher: {
        async publishEvent(event) {
          order.push(["publish", event]);
          return { ...event, id: "d".repeat(64) };
        },
      },
    },
  );
  assert.equal(result.eventId, "d".repeat(64));
  assert.deepEqual(
    order.map(([step]) => step),
    ["sign", "publish"],
  );
});

test("fresh genesis rejects non-canonical session references", () => {
  assert.throws(
    () =>
      buildCodingSessionGenesisEvent({
        channelId: CHANNEL_ID,
        sessionRef: SESSION_REF.toUpperCase(),
      }),
    /canonical lowercase/,
  );
});
