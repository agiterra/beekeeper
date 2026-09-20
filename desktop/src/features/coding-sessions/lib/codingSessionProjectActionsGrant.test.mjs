/**
 * The project-action delegation, with stubbed reads and writes (ledger 186,
 * finding 178(f)). Three things are pinned: a confirmed grant, a replayed
 * launch that publishes nothing a second time, and a relay refusal reaching
 * the caller in the relay's own words.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionProjectActionGrantKey } from "./codingSessionRoster.ts";
import {
  codingSessionProjectActionsGrantDisclosure,
  codingSessionProjectActionsGrantUnconfirmedDisclosure,
  ensureCodingSessionProjectActionsGrant,
} from "./codingSessionProjectActionsGrant.ts";

const CHANNEL_ID = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a1".repeat(32);
const LEAD = "aa".repeat(32);
const FOUNDER = "cc".repeat(32);
const PROJECT_REF = `30621:${"11".repeat(32)}:kettle`;
const KEY = codingSessionProjectActionGrantKey(LEAD, PROJECT_REF);

const INPUT = {
  channelId: CHANNEL_ID,
  genesisRef: GENESIS_REF,
  actorPubkey: LEAD,
  projectRef: PROJECT_REF,
};

function fold(grants) {
  return {
    accepted: new Map(),
    activeSeats: new Map(),
    projectActionGrants: new Map(grants),
    acceptedHead: null,
    pending: [],
  };
}

function liveGrant() {
  return [
    [
      KEY,
      {
        granteePubkey: LEAD,
        projectRef: PROJECT_REF,
        grantedBy: FOUNDER,
        grantEventId: "ee".repeat(32),
      },
    ],
  ];
}

test("a delegation is published once and reported only after the relay accepts it", async () => {
  const published = [];
  let reads = 0;
  const result = await ensureCodingSessionProjectActionsGrant(INPUT, {
    fetchFold: async (channelId, genesisRef) => {
      assert.equal(channelId, CHANNEL_ID);
      assert.equal(genesisRef, GENESIS_REF);
      reads += 1;
      // Read 1 is the pre-check, read 2 is the relay still silent, read 3
      // carries the receipt-backed grant.
      return reads >= 3 ? fold(liveGrant()) : fold([]);
    },
    publishTransition: async (transition) => {
      published.push(transition);
      return { id: "ee".repeat(32) };
    },
    wait: async () => {},
  });
  assert.deepEqual(result, {
    status: "granted",
    eventId: "ee".repeat(32),
    reason: null,
  });
  assert.deepEqual(published, [
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      type: "grant-project-actions",
      granteePubkey: LEAD,
      projectRef: PROJECT_REF,
    },
  ]);
});

test("an already-delegated lead is not delegated a second time", async () => {
  let publishes = 0;
  const result = await ensureCodingSessionProjectActionsGrant(INPUT, {
    fetchFold: async () => fold(liveGrant()),
    publishTransition: async () => {
      publishes += 1;
      return { id: "ff".repeat(32) };
    },
    wait: async () => {},
  });
  assert.deepEqual(result, {
    status: "already-active",
    eventId: null,
    reason: null,
  });
  assert.equal(publishes, 0);
});

test("the relay's own refusal sentence reaches the caller", async () => {
  await assert.rejects(
    ensureCodingSessionProjectActionsGrant(INPUT, {
      fetchFold: async () => fold([]),
      publishTransition: async () => {
        throw new Error(
          "blocked: only a project owner may delegate this project's actions",
        );
      },
      wait: async () => {},
    }),
    /only a project owner may delegate this project's actions/,
  );
});

test("a published delegation nobody could confirm says exactly that", async () => {
  const result = await ensureCodingSessionProjectActionsGrant(INPUT, {
    fetchFold: async () => fold([]),
    publishTransition: async () => ({ id: "ee".repeat(32) }),
    wait: async () => {},
    receiptPollAttempts: 2,
  });
  assert.equal(result.status, "published-unconfirmed");
  assert.equal(result.eventId, "ee".repeat(32));
  assert.match(
    result.reason,
    /did not appear in this session's authority chain/,
  );
});

test("a rate-limited confirmation read is waited out, never republished", async () => {
  let reads = 0;
  let publishes = 0;
  const waits = [];
  const result = await ensureCodingSessionProjectActionsGrant(INPUT, {
    fetchFold: async () => {
      reads += 1;
      if (reads === 1) return fold([]);
      if (reads === 2) {
        throw new Error("rate-limited: quota exceeded; retry in 2s");
      }
      return fold(liveGrant());
    },
    publishTransition: async () => {
      publishes += 1;
      return { id: "ee".repeat(32) };
    },
    wait: async (ms) => {
      waits.push(ms);
    },
  });
  assert.equal(result.status, "granted");
  assert.equal(publishes, 1);
  assert.ok(waits[0] > 0);
});

test("the two disclosures say different things, and neither claims the other", () => {
  const refused = codingSessionProjectActionsGrantDisclosure({
    leadLabel: "Fable",
    detail: "blocked: not a project owner",
  });
  assert.match(
    refused,
    /Fable cannot publish or trigger this project's actions/,
  );
  assert.match(refused, /a project owner must sign that delegation/);
  assert.match(refused, /blocked: not a project owner/);
  const unconfirmed = codingSessionProjectActionsGrantUnconfirmedDisclosure({
    leadLabel: "Fable",
    detail: null,
  });
  assert.match(unconfirmed, /is unconfirmed on this computer/);
  assert.equal(/cannot publish/.test(unconfirmed), false);
});
