import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { KIND_CODING_SESSION_AUTHORITY_TRANSITION } from "@/shared/constants/kinds";

import { projectCodingSessionMissionAuthority } from "./codingSessionMissionAuthority.ts";

const KIND_SYSTEM_MESSAGE = 40099;
const CSAT_VERSION = "csat1-1";
const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const RELAY_SECRET = new Uint8Array(32).fill(2);
const RELAY = getPublicKey(RELAY_SECRET);
const FOUNDER = getPublicKey(FOUNDER_SECRET);

/**
 * The decoder's own refusals — the strict-shape grain the shared vectors
 * describe, as opposed to the fold's separate question of whether a
 * *semantically* complete chain follows.
 */
const SHAPE_ERRORS = new Set([
  "authority transition does not match the strict CSAT shape",
  "authority receipt does not match the strict CSAT receipt shape",
  "authority transition crosses or disagrees with its supplied scope",
]);
const BINDING_ERROR =
  "authority receipt facts do not match its accepted transition";

function sign(kind, content, tags, secret) {
  return finalizeEvent({ kind, content, tags, created_at: 1 }, secret);
}

/**
 * The second desktop strict reader of the 44228/40099 pair.
 *
 * `codingSessionAuthorityTimeline.ts` has run these vectors since ledger 204;
 * this projection — the one Mission, Decisions and Settlement read — never
 * did, and so repeated ledger 204's bug key for key: it knew nothing of
 * `grant-project-actions`, refused the `projectRef` the relay adds on purpose
 * (`crates/buzz-relay/src/handlers/side_effects.rs`, ledger 186), and read
 * "unknown" over every team session an owner founded (ledger 238). A strict
 * reader that does not load the shared fixtures is the defect; this test is
 * the fix.
 */
test("desktop mission-authority projection runs the shared authority-chain vectors", () => {
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
        ["csat-v", CSAT_VERSION],
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
    const result = projectCodingSessionMissionAuthority({
      channelRef: CHANNEL,
      genesisRef,
      founderPubkey: FOUNDER,
      relayPubkey: RELAY,
      transitions: [transition],
      receipts: [receipt],
    });
    const error = result.ok ? null : result.error;

    if (!vector.transitionValid || !vector.receiptValid) {
      assert.ok(
        error !== null && SHAPE_ERRORS.has(error),
        `${vector.name}: expected a strict-shape refusal, got ${
          error ?? "an accepted projection"
        }`,
      );
      continue;
    }
    if (!vector.binds) {
      assert.equal(error, BINDING_ERROR, `${vector.name}: receipt binding`);
      continue;
    }
    // A well-formed, binding pair must never be refused *for its shape*. It
    // may still be refused by the fold — a lone `revoke` names no active
    // grant, a `takeover` whose grantee is not its signer has no standing —
    // and that is a different question, asked elsewhere.
    assert.ok(
      error === null || (!SHAPE_ERRORS.has(error) && error !== BINDING_ERROR),
      `${vector.name}: a valid binding pair was refused for its shape: ${error}`,
    );
  }
});

/**
 * The control run's live shape (ledger 238): the desktop signs this
 * delegation at launch for every team session a project owner founds with
 * "Use roles" on, and the relay echoes `projectRef` onto the receipt. Before
 * this lane the projection failed outright, so Mission, Decisions and
 * Settlement rendered "unknown" for the whole session.
 */
test("an owner-founded delegation chain projects instead of blanking", () => {
  const genesisRef = "ab".repeat(32);
  const projectRef = `30621:${"ef".repeat(32)}:kettle-control`;
  const grantee = "11".repeat(32);
  const transition = sign(
    KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    JSON.stringify({
      genesisRef,
      prevAccepted: null,
      seq: 1,
      type: "grant-project-actions",
      granteePubkey: grantee,
      projectRef,
    }),
    [
      ["h", CHANNEL],
      ["csat-v", CSAT_VERSION],
      ["csat-genesis", genesisRef],
    ],
    FOUNDER_SECRET,
  );
  const receipt = sign(
    KIND_SYSTEM_MESSAGE,
    JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef,
      acceptedEventId: transition.id,
      seq: 1,
      transitionType: "grant-project-actions",
      granteePubkey: grantee,
      projectRef,
    }),
    [["h", CHANNEL]],
    RELAY_SECRET,
  );
  const result = projectCodingSessionMissionAuthority({
    channelRef: CHANNEL,
    genesisRef,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    transitions: [transition],
    receipts: [receipt],
  });
  assert.equal(result.ok, true, result.ok ? "" : result.error);
  assert.equal(result.value.headEventId, transition.id);
  assert.equal(result.value.headSeq, 1);
  // A delegation of one project's actions is the narrowest link in the
  // chain: it confers no steering authority and no seat, exactly as core's
  // policy fold reads it.
  assert.deepEqual(result.value.activeGrants, []);
  assert.deepEqual(result.value.activeSeats, []);
  assert.equal(result.value.claim.state, "no-claim");
  assert.equal(
    result.value.policyGrants[0]?.transitionType,
    "grant-project-actions",
  );
});
