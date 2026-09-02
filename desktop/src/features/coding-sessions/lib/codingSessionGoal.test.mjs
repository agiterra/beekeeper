import assert from "node:assert/strict";
import test from "node:test";

import { finalizeEvent } from "nostr-tools/pure";
import {
  buildCodingSessionGoalEvent,
  buildCodingSessionGoalFilter,
  foldLatestCodingSessionGoals,
  foldLatestCodingSessionGoalsByFounder,
  parseCodingSessionGoal,
  publishCodingSessionGoal,
} from "./codingSessionGoal.ts";

const CHANNEL_ID = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER_KEY = new Uint8Array(32).fill(7);
const FOREIGN_KEY = new Uint8Array(32).fill(8);

function goal(content, created_at, key = FOUNDER_KEY) {
  return finalizeEvent(
    {
      ...buildCodingSessionGoalEvent({
        channelId: CHANNEL_ID,
        content,
        sessionRef: SESSION_REF,
      }),
      created_at,
    },
    key,
  );
}

test("goal uses the exact regular-event envelope and scoped filter", () => {
  assert.deepEqual(
    buildCodingSessionGoalEvent({
      channelId: CHANNEL_ID,
      content: " Ship authority UI ",
      sessionRef: SESSION_REF,
    }),
    {
      kind: 44227,
      content: "Ship authority UI",
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["csgl-v", "csgl1-1"],
      ],
    },
  );
  assert.deepEqual(buildCodingSessionGoalFilter([CHANNEL_ID]), {
    kinds: [44227],
    "#h": [CHANNEL_ID],
    limit: 1000,
  });
});

test("goal parser rejects smuggled tags, blank prose, and invalid signatures", () => {
  const valid = goal("Visible objective", 10);
  assert.equal(parseCodingSessionGoal(valid)?.content, "Visible objective");
  assert.equal(
    parseCodingSessionGoal({ ...valid, tags: [...valid.tags, ["p", "x"]] }),
    null,
  );
  assert.equal(parseCodingSessionGoal({ ...valid, content: " " }), null);
  assert.equal(
    parseCodingSessionGoal({ ...valid, sig: "0".repeat(128) }),
    null,
  );
});

test("append-only revisions fold by timestamp then event id", () => {
  const old = goal("Old", 10);
  const newer = goal("New", 11);
  assert.equal(
    [...foldLatestCodingSessionGoals([newer, old]).values()][0]?.content,
    "New",
  );
  const tiedA = goal("A", 12);
  const tiedB = goal("B", 12);
  const expected = tiedA.id > tiedB.id ? "A" : "B";
  assert.equal(
    [...foldLatestCodingSessionGoals([tiedA, tiedB]).values()][0]?.content,
    expected,
  );
});

test("founder fold keeps a foreign signer from shadowing the founder revision", () => {
  const founder = goal("Founder goal", 10);
  const foreign = goal("Foreign goal", 20, FOREIGN_KEY);
  const folded = foldLatestCodingSessionGoalsByFounder([founder, foreign]);
  assert.equal(folded.size, 2);
  assert.ok(
    [...folded.values()].some(
      (candidate) => candidate.content === "Founder goal",
    ),
  );
});

test("goal validates byte bounds and publishes the signed event", async () => {
  assert.throws(
    () =>
      buildCodingSessionGoalEvent({
        channelId: CHANNEL_ID,
        content: "  ",
        sessionRef: SESSION_REF,
      }),
    /between 1 and 4096/,
  );
  const signed = goal("Publish me", 10);
  let published = null;
  const accepted = await publishCodingSessionGoal(
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

test("F1: an over-cap goal is refused by the launch block, with both numbers", async () => {
  const { codingSessionGoalOverflow, MAX_CODING_SESSION_GOAL_BYTES } =
    await import("./codingSessionGoal.ts");
  const { codingSessionCrewLaunchBlock } = await import(
    "./codingSessionCrew.ts"
  );

  assert.equal(codingSessionGoalOverflow("Close ledger item 104."), null);
  assert.equal(codingSessionGoalOverflow("   "), null, "blank is not overflow");
  assert.equal(
    codingSessionGoalOverflow("a".repeat(MAX_CODING_SESSION_GOAL_BYTES)),
    null,
    "exactly the cap fits",
  );
  // Measured in UTF-8 bytes, not characters: 2,049 two-byte characters is
  // 4,098 bytes, and a character count would have called it well under.
  assert.deepEqual(codingSessionGoalOverflow("é".repeat(2_049)), {
    bytes: 4_098,
    cap: 4_096,
  });

  const ready = {
    hasTeam: true,
    seatCount: 3,
    hasChannel: true,
    canCreateChannel: false,
    createInFlight: false,
    isLaunching: false,
    goal: "Close ledger item 104.",
  };
  assert.equal(codingSessionCrewLaunchBlock(ready), null);

  // The launch is refused before anything is signed, rather than launching a
  // whole team and silently dropping its 44227 (batch 2 review, F1).
  const blocked = codingSessionCrewLaunchBlock({
    ...ready,
    goal: "x".repeat(5_000),
  });
  assert.ok(blocked, "an over-cap goal must block the launch");
  assert.match(blocked, /5,000 UTF-8 bytes/);
  assert.match(blocked, /cap is 4,096/);
  assert.match(blocked, /shorten it by 904 bytes/);
});
