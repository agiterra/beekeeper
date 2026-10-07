import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";

import {
  analyzeCodingSessionAuthorityChain,
  CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
} from "./codingSessionAuthorityTimeline.ts";

const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const RELAY_SECRET = new Uint8Array(32).fill(2);
const RELAY = getPublicKey(RELAY_SECRET);

function sign(kind, content, tags, secret) {
  return finalizeEvent({ kind, content, tags, created_at: 1 }, secret);
}

/**
 * The shared cross-reader vectors — the same file buzz-core, buzz-cli and
 * beekeeper-session-provider run (`conformance/authority-chain/README.md`).
 *
 * A future lane that adds a key to the 44228 content or the 40099 receipt
 * adds a vector here, and this test fails until this decoder has been taught
 * the key. That is what did not happen in ledger 186 and what ledger 204 cost.
 */
test("desktop authority decoder runs the shared authority-chain vectors", () => {
  const fixture = JSON.parse(
    readFileSync(
      resolve(
        process.cwd(),
        "../conformance/authority-chain/fixtures/chain-vectors.json",
      ),
      "utf8",
    ),
  );
  assert.equal(
    fixture.schema,
    "buzz-coding-session-authority-chain-conformance/v1",
  );
  assert.ok(fixture.vectors.length > 0);
  const genesisRef = fixture.constants.genesisRef;

  for (const vector of fixture.vectors) {
    const transition = sign(
      KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      JSON.stringify(vector.transition),
      [
        ["h", CHANNEL],
        ["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION],
        ["csat-genesis", genesisRef],
      ],
      FOUNDER_SECRET,
    );
    // The relay stamps the receipt onto the transition it just accepted, so
    // the fixture's placeholder id is replaced by the real one here.
    const receipt = sign(
      KIND_SYSTEM_MESSAGE,
      JSON.stringify({
        ...vector.receipt,
        acceptedEventId: transition.id,
      }),
      [["h", CHANNEL]],
      RELAY_SECRET,
    );
    const analysis = analyzeCodingSessionAuthorityChain({
      expectedChannel: CHANNEL,
      trustedRelayPubkey: RELAY,
      genesisRef,
      founderPubkey: getPublicKey(FOUNDER_SECRET),
      transitions: [transition],
      receipts: [receipt],
    });
    // Asserted at the decoder's own grain, which is what every reader shares.
    // Whether a *semantically* complete chain follows is the fold's separate
    // question: a lone `revoke-seat` decodes perfectly and still conflicts,
    // because no accepted seat precedes it.
    assert.equal(
      analysis.parsedTransitions.length === 1,
      vector.transitionValid,
      `${vector.name}: transition decode`,
    );
    assert.equal(
      analysis.receiptBackedTransitionIds.size === 1,
      vector.receiptValid,
      `${vector.name}: receipt decode`,
    );
    if (vector.transitionValid && vector.receiptValid) {
      const unbound =
        analysis.disposition === "conflicted" &&
        (analysis.reason ?? "").includes("does not bind the exact facts");
      assert.equal(unbound, !vector.binds, `${vector.name}: receipt binding`);
    }
    if (vector.binds && vector.transition.projectRef !== undefined) {
      assert.equal(
        analysis.links[0]?.projectRef,
        vector.transition.projectRef,
        `${vector.name} must carry its scope onto the accepted link`,
      );
    }
  }
});
