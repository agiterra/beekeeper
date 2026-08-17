import assert from "node:assert/strict";
import test from "node:test";

import { finalizeEvent } from "nostr-tools/pure";
import {
  buildCodingSessionNameEvent,
  buildCodingSessionNameFilter,
  foldLatestCodingSessionNames,
  foldLatestCodingSessionNamesByFounder,
  parseCodingSessionName,
  publishCodingSessionName,
} from "./codingSessionName.ts";

const CHANNEL_ID = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER_KEY = new Uint8Array(32).fill(7);
const FOREIGN_KEY = new Uint8Array(32).fill(8);

function name(content, created_at, key = FOUNDER_KEY) {
  return finalizeEvent(
    {
      ...buildCodingSessionNameEvent({
        channelId: CHANNEL_ID,
        content,
        sessionRef: SESSION_REF,
      }),
      created_at,
    },
    key,
  );
}

test("name uses the exact regular-event envelope and scoped filter", () => {
  assert.deepEqual(
    buildCodingSessionNameEvent({
      channelId: CHANNEL_ID,
      content: " Authority phase ",
      sessionRef: SESSION_REF,
    }),
    {
      kind: 44229,
      content: "Authority phase",
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["csnm-v", "csnm1-1"],
      ],
    },
  );
  assert.deepEqual(buildCodingSessionNameFilter([CHANNEL_ID]), {
    kinds: [44229],
    "#h": [CHANNEL_ID],
    limit: 1000,
  });
});

test("name parser rejects smuggled, blank, multiline, and invalid events", () => {
  const valid = name("Authority phase", 10);
  assert.equal(parseCodingSessionName(valid)?.content, "Authority phase");
  assert.equal(
    parseCodingSessionName({ ...valid, tags: [...valid.tags, ["p", "x"]] }),
    null,
  );
  assert.equal(parseCodingSessionName({ ...valid, content: " " }), null);
  assert.equal(
    parseCodingSessionName({ ...valid, content: "first\nsecond" }),
    null,
  );
  assert.equal(
    parseCodingSessionName({ ...valid, sig: "0".repeat(128) }),
    null,
  );
});

test("append-only renames fold by timestamp then event id", () => {
  const old = name("Old", 10);
  const newer = name("New", 11);
  assert.equal(
    [...foldLatestCodingSessionNames([newer, old]).values()][0]?.content,
    "New",
  );
  const tiedA = name("A", 12);
  const tiedB = name("B", 12);
  const expected = tiedA.id > tiedB.id ? "A" : "B";
  assert.equal(
    [...foldLatestCodingSessionNames([tiedA, tiedB]).values()][0]?.content,
    expected,
  );
});

test("founder fold keeps a foreign signer from shadowing the session name", () => {
  const founder = name("Founder name", 10);
  const foreign = name("Foreign name", 20, FOREIGN_KEY);
  const folded = foldLatestCodingSessionNamesByFounder([founder, foreign]);
  assert.equal(folded.size, 2);
  assert.ok(
    [...folded.values()].some(
      (candidate) => candidate.content === "Founder name",
    ),
  );
});

test("name validates byte/line bounds and publishes the signed event", async () => {
  for (const content of ["  ", "first\nsecond", "x".repeat(257)]) {
    assert.throws(
      () =>
        buildCodingSessionNameEvent({
          channelId: CHANNEL_ID,
          content,
          sessionRef: SESSION_REF,
        }),
      /one line between 1 and 256/,
    );
  }
  const signed = name("Publish me", 10);
  let published = null;
  const accepted = await publishCodingSessionName(
    { channelId: CHANNEL_ID, content: "Publish me", sessionRef: SESSION_REF },
    {
      signer: async () => signed,
      publisher: {
        async publishEvent(event) {
          published = event;
          return event;
        },
      },
    },
  );
  assert.equal(published, signed);
  assert.equal(accepted.id, signed.id);
});
