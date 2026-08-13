import assert from "node:assert/strict";
import test from "node:test";

import { publishCodingSessionLaneMessage } from "./codingSessionLanePublish.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

test("lane publish signs the built kind:9 event and returns the accepted id", async () => {
  const signed = [];
  const published = [];
  const result = await publishCodingSessionLaneMessage(
    {
      channelId: "channel-1",
      content: "Codex, take the failing test Claude found.",
      sessionRef: SESSION_REF,
    },
    {
      signer: async (input) => {
        signed.push(input);
        return { id: "event-1", kind: input.kind, tags: input.tags };
      },
      publisher: {
        publishEvent: async (event) => {
          published.push(event);
          return event;
        },
      },
    },
  );

  assert.deepEqual(result, { eventId: "event-1" });
  assert.equal(signed.length, 1);
  assert.equal(signed[0].kind, 9);
  assert.deepEqual(signed[0].tags, [
    ["h", "channel-1"],
    ["cs-session", SESSION_REF],
  ]);
  assert.equal(published.length, 1);
});

test("lane publish refuses malformed input before any signing", async () => {
  await assert.rejects(
    publishCodingSessionLaneMessage(
      { channelId: "channel-1", content: "hi", sessionRef: "not-a-uuid" },
      {
        signer: async () => {
          throw new Error("must not sign");
        },
      },
    ),
    /canonical lowercase hyphenated UUID/,
  );
});

test("a relay rejection is a failure of the write", async () => {
  await assert.rejects(
    publishCodingSessionLaneMessage(
      { channelId: "channel-1", content: "hi", sessionRef: SESSION_REF },
      {
        signer: async (input) => ({ id: "event-1", kind: input.kind }),
        publisher: {
          publishEvent: async () => {
            throw new Error("blocked: not a member");
          },
        },
      },
    ),
    /not a member/,
  );
});
