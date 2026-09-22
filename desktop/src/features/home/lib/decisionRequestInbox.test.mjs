import assert from "node:assert/strict";
import { test } from "node:test";

import {
  inboxDecisionModel,
  readInboxDecisionRequest,
} from "@/features/home/lib/decisionRequestInbox";

const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS = "ab".repeat(32);
const CHANNEL = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";

/** Run 2's `85266e05…` shape: five NIP-CSTX tags, no `p`. */
function request(body) {
  return {
    id: "85266e05".repeat(8),
    kind: 44244,
    pubkey: "c".repeat(64),
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["cstx-v", "buzz-coding-session-team-transaction/v1"],
      ["cstx-genesis", GENESIS],
      ["cstx-type", "decision.request"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-team-transaction/v1",
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "decision.request",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        question: "Which identity?",
        options: ["Seat", "Founder"],
        heldOn: "founder",
        blocks: [],
        recommendation: null,
        ...body,
      },
    }),
  };
}

test("a founder-held request reads, and the founder may answer it", () => {
  const read = readInboxDecisionRequest(request({}));
  assert.ok(read);
  assert.deepEqual(read.options, ["Seat", "Founder"]);
  assert.equal(read.channelRef, CHANNEL);
  const model = inboxDecisionModel(read, "f".repeat(64));
  assert.equal(model.viewerIsHolder, true);
  assert.equal(model.heldElsewhereSentence, null);
});

test("a request held on the viewer's agent is shown disabled, saying whose", () => {
  const agent = "a".repeat(64);
  const read = readInboxDecisionRequest(request({ heldOn: agent }));
  const model = inboxDecisionModel(read, "f".repeat(64));
  assert.equal(model.viewerIsHolder, false);
  assert.match(model.heldElsewhereSentence, /aaaaaaaa/);
});

test("anything not a fully read decision.request carries no control", () => {
  assert.equal(readInboxDecisionRequest({ ...request({}), kind: 9 }), null);
  const answer = request({});
  answer.tags[4] = ["cstx-type", "decision.answer"];
  assert.equal(readInboxDecisionRequest(answer), null);
  assert.equal(readInboxDecisionRequest(request({ options: "Seat" })), null);
});
