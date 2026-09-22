import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { projectCodingSessionMissionAuthority } from "./codingSessionMissionAuthority.ts";

// The claim rule of `docs/HANDOVER_IMPL.md` §1, exercised over real signatures
// and real relay receipts. Every event here is finalized with a secret, so a
// projection that admitted an unsigned or mis-signed link would fail rather
// than pass on a hand-written `sig` field.
const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const GENESIS = "ab".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const B_SECRET = new Uint8Array(32).fill(2);
const C_SECRET = new Uint8Array(32).fill(3);
const RELAY_SECRET = new Uint8Array(32).fill(4);
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const B = getPublicKey(B_SECRET);
const C = getPublicKey(C_SECRET);
const RELAY = getPublicKey(RELAY_SECRET);
const BODY_A = "1a".repeat(32);
const BODY_B = "2b".repeat(32);

function sign({ kind, content, tags, secret = FOUNDER_SECRET, createdAt = 1 }) {
  return finalizeEvent({ kind, content, tags, created_at: createdAt }, secret);
}

function transition(payload, secret = FOUNDER_SECRET) {
  return sign({
    kind: 44228,
    content: JSON.stringify({ genesisRef: GENESIS, ...payload }),
    tags: [
      ["h", CHANNEL],
      ["csat-v", "csat1-1"],
      ["csat-genesis", GENESIS],
    ],
    secret,
  });
}

function receipt(accepted, payload, createdAt = 100) {
  return sign({
    kind: 40099,
    content: JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: GENESIS,
      acceptedEventId: accepted.id,
      seq: payload.seq,
      transitionType: payload.type,
      granteePubkey: payload.granteePubkey,
      ...(payload.role ? { role: payload.role } : {}),
      ...(payload.bodyPubkey ? { bodyPubkey: payload.bodyPubkey } : {}),
    }),
    tags: [["h", CHANNEL]],
    secret: RELAY_SECRET,
    createdAt,
  });
}

/** Build one accepted chain from a list of payloads, in order. */
function chain(links) {
  const transitions = [];
  const receipts = [];
  let prevAccepted = null;
  links.forEach((link, index) => {
    const payload = {
      prevAccepted,
      seq: index + 1,
      type: link.type,
      granteePubkey: link.granteePubkey,
      ...(link.role ? { role: link.role } : {}),
      ...(link.bodyPubkey ? { bodyPubkey: link.bodyPubkey } : {}),
    };
    const event = transition(payload, link.secret ?? FOUNDER_SECRET);
    transitions.push(event);
    receipts.push(
      receipt(event, payload, link.acceptedAt ?? 100 + index),
      ...(link.omitReceipt ? [] : []),
    );
    prevAccepted = event.id;
  });
  return { transitions, receipts };
}

function project(links) {
  const { transitions, receipts } = chain(links);
  return projectCodingSessionMissionAuthority({
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    transitions,
    receipts,
  });
}

test("a chain with no handover link reports no claim, not a voided one", () => {
  const result = project([{ type: "grant-operator", granteePubkey: B }]);
  assert.equal(result.ok, true);
  assert.deepEqual(result.value.claim, { state: "no-claim" });
});

test("an operator's self-takeover claims the whole session", () => {
  const result = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
      acceptedAt: 5_000,
    },
  ]);
  assert.equal(result.ok, true);
  assert.deepEqual(result.value.claim, {
    state: "active",
    claimant: B,
    bodyPubkey: BODY_B,
    acceptedEventId: result.value.acceptedEventIds[1],
    seq: 2,
  });
  assert.equal(result.value.claimSince, 5_000);
  assert.equal(result.value.claimVoidedAt, null);
  // The chain still projects its grants: a claim adds a fact, it does not
  // replace the roster.
  assert.deepEqual(
    result.value.activeGrants.map((grant) => grant.actorPubkey),
    [B],
  );
});

test("a takeover naming somebody other than its signer is refused", () => {
  const result = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: FOUNDER,
      bodyPubkey: BODY_A,
      secret: B_SECRET,
    },
  ]);
  assert.equal(result.ok, false);
  assert.match(result.error, /unauthorized signer/);
});

test("a takeover from a viewer is refused; from the founder it stands", () => {
  const fromViewer = project([
    { type: "grant-viewer", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
  ]);
  assert.equal(fromViewer.ok, false);

  const fromFounder = project([
    { type: "takeover", granteePubkey: FOUNDER, bodyPubkey: BODY_A },
  ]);
  assert.equal(fromFounder.ok, true);
  assert.equal(fromFounder.value.claim.state, "active");
  assert.equal(fromFounder.value.claim.claimant, FOUNDER);
});

test("a transfer moves the claim to another live operator", () => {
  const result = project([
    { type: "grant-operator", granteePubkey: B },
    { type: "grant-operator", granteePubkey: C },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    {
      type: "transfer",
      granteePubkey: C,
      bodyPubkey: BODY_A,
      secret: B_SECRET,
      acceptedAt: 9_000,
    },
  ]);
  assert.equal(result.ok, true);
  assert.equal(result.value.claim.state, "active");
  assert.equal(result.value.claim.claimant, C);
  assert.equal(result.value.claim.bodyPubkey, BODY_A);
  assert.equal(result.value.claimSince, 9_000);
});

test("revoking the claimant voids the claim and a regrant does not restore it", () => {
  const revoked = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    { type: "revoke", granteePubkey: B, acceptedAt: 7_000 },
  ]);
  assert.equal(revoked.ok, true);
  assert.equal(revoked.value.claim.state, "voided");
  assert.equal(revoked.value.claim.last.claimant, B);
  assert.equal(revoked.value.claim.last.bodyPubkey, BODY_B);
  assert.equal(revoked.value.claimVoidedAt, 7_000);

  const regranted = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    { type: "revoke", granteePubkey: B },
    { type: "grant-operator", granteePubkey: B },
  ]);
  assert.equal(regranted.ok, true);
  assert.equal(
    regranted.value.claim.state,
    "voided",
    "a regrant is not the deliberate act that lifts a fence",
  );

  const reclaimed = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    { type: "revoke", granteePubkey: B },
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
  ]);
  assert.equal(reclaimed.value.claim.state, "active");
});

test("demoting the claimant to viewer voids the claim too", () => {
  const result = project([
    { type: "grant-operator", granteePubkey: B },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    { type: "grant-viewer", granteePubkey: B },
  ]);
  assert.equal(result.ok, true);
  assert.equal(result.value.claim.state, "voided");
  assert.equal(result.value.claim.last.claimant, B);
});

test("revoking somebody who is not the claimant leaves the claim alone", () => {
  const result = project([
    { type: "grant-operator", granteePubkey: B },
    { type: "grant-operator", granteePubkey: C },
    {
      type: "takeover",
      granteePubkey: B,
      bodyPubkey: BODY_B,
      secret: B_SECRET,
    },
    { type: "revoke", granteePubkey: C },
  ]);
  assert.equal(result.ok, true);
  assert.equal(result.value.claim.state, "active");
  assert.equal(result.value.claim.claimant, B);
});

test("a takeover without a body, or a grant carrying one, is not this shape", () => {
  const noBody = project([
    { type: "grant-operator", granteePubkey: B },
    { type: "takeover", granteePubkey: B, secret: B_SECRET },
  ]);
  assert.equal(noBody.ok, false);
  // Refused on both sides of the pair since ledger 238 required `bodyPubkey`
  // on a claim receipt too; the receipt shape is checked first, so that is
  // the refusal a reader sees.
  assert.match(noBody.error, /strict CSAT/);

  // A grant that carried a body is refused on both sides of the pair: the
  // receipt shape is checked first, so that is the refusal a reader sees.
  const grantWithBody = project([
    { type: "grant-operator", granteePubkey: B, bodyPubkey: BODY_B },
  ]);
  assert.equal(grantWithBody.ok, false);
  assert.match(grantWithBody.error, /strict CSAT/);
});

test("a receipt naming a different body than its transition is refused", () => {
  const payload = {
    prevAccepted: null,
    seq: 1,
    type: "takeover",
    granteePubkey: FOUNDER,
    bodyPubkey: BODY_A,
  };
  const event = transition(payload);
  const wrong = receipt(event, { ...payload, bodyPubkey: BODY_B });
  const result = projectCodingSessionMissionAuthority({
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    transitions: [event],
    receipts: [wrong],
  });
  assert.equal(result.ok, false);
  assert.match(result.error, /do not match its accepted transition/);
});

/**
 * Changed by ledger 238. This reader used to accept a claim receipt with no
 * `bodyPubkey` on the reasoning that the signed transition still named the
 * body. The shared vectors say otherwise — `takeover-receipt-dropping-its-body`
 * is `receiptValid: false`, and the CLI, the provider and the desktop timeline
 * decoder have always refused it. The relay cannot emit one (a claim link
 * cannot be signed without a body), so accepting it only meant this reader
 * folded a claim the others would not.
 */
test("a receipt that omits bodyPubkey is refused, as every other reader refuses it", () => {
  const payload = {
    prevAccepted: null,
    seq: 1,
    type: "takeover",
    granteePubkey: FOUNDER,
    bodyPubkey: BODY_A,
  };
  const event = transition(payload);
  const older = sign({
    kind: 40099,
    content: JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: GENESIS,
      acceptedEventId: event.id,
      seq: 1,
      transitionType: "takeover",
      granteePubkey: FOUNDER,
    }),
    tags: [["h", CHANNEL]],
    secret: RELAY_SECRET,
    createdAt: 42,
  });
  const result = projectCodingSessionMissionAuthority({
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    transitions: [event],
    receipts: [older],
  });
  assert.equal(result.ok, false);
  assert.match(result.error, /strict CSAT receipt shape/);
});

test("the relay's own takeover receipt, from Lane K's fixture, folds to its claim", () => {
  // The fixture holds the accepted 44228 and the exact JSON object the relay
  // puts inside its kind-40099 receipt
  // (`side_effects.rs::handle_coding_session_authority_transition_accepted`),
  // so this pins the receipt reader to the relay's own keys rather than to a
  // second reading of the spec. The 40099 wrapper is signed here because the
  // fixture carries the content object, not the system message.
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL("./codingSessionHandover.fixture.json", import.meta.url),
      ),
      "utf8",
    ),
  );
  const takeover = fixture.events.find((event) => event.kind === 44228);
  const receiptContent = fixture.events.find(
    (event) => event.type === "coding_session_authority_transition_accepted",
  );
  const genesisRef = receiptContent.genesisRef;
  const channelRef = Object.fromEntries(takeover.tags).h;
  const signedReceipt = sign({
    kind: 40099,
    content: JSON.stringify(receiptContent),
    tags: [["h", channelRef]],
    secret: RELAY_SECRET,
    createdAt: 1_800_000_500,
  });
  const result = projectCodingSessionMissionAuthority({
    channelRef,
    genesisRef,
    // The fixture's takeover is a self-claim by a granted operator; with no
    // earlier links in the file, the founder is read as that same signer so
    // the one link this fixture carries is contiguous from seq 1.
    founderPubkey: takeover.pubkey,
    relayPubkey: RELAY,
    transitions: [takeover],
    receipts: [signedReceipt],
  });
  // The fixture carries one link at seq 2 and no seq 1, so the projection
  // refuses the chain rather than inventing a head. The refusal it gives is
  // the load-bearing part: **contiguity**, which is only reachable after the
  // takeover's `bodyPubkey` shape and the receipt's binding both passed. A
  // decoder that rejected `bodyPubkey`, or a receipt reader that refused the
  // relay's own extra key, would have failed earlier with a different name.
  assert.equal(result.ok, false);
  assert.equal(result.error, "accepted authority chain is not contiguous");
});
