import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  buildCodingSessionAuthorityTransitionEvent,
  codingSessionRosterEntries,
  foldCodingSessionRoster,
  grantTypeForEntityRole,
  isCodingSessionAuthorityHeadConflict,
  publishCodingSessionAuthorityTransition,
} from "./codingSessionRoster.ts";

const CHANNEL_ID = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a1".repeat(32);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY_SECRET = generateSecretKey();
const RELAY = getPublicKey(RELAY_SECRET);
const ALICE = "aa".repeat(32);
const BOB = "bb".repeat(32);

let eventCounter = 0;
function transitionEvent({
  seq,
  prevAccepted,
  type,
  granteePubkey,
  role,
  extraContent,
  channelId = CHANNEL_ID,
  genesisRef = GENESIS_REF,
  signerSecret = FOUNDER_SECRET,
}) {
  eventCounter += 1;
  const content = {
    genesisRef,
    prevAccepted,
    seq,
    type,
    granteePubkey,
  };
  if (role !== undefined) content.role = role;
  if (extraContent) Object.assign(content, extraContent);
  return finalizeEvent(
    {
      created_at: 1000 + eventCounter,
      kind: 44228,
      tags: [
        ["h", channelId],
        ["csat-v", "csat1-1"],
        ["csat-genesis", genesisRef],
      ],
      content: JSON.stringify(content),
    },
    signerSecret,
  );
}

function receiptEvent({
  acceptedEventId,
  seq,
  transitionType,
  granteePubkey,
  role,
  extraContent,
  channelId = CHANNEL_ID,
  genesisRef = GENESIS_REF,
  type = "coding_session_authority_transition_accepted",
  signerSecret = RELAY_SECRET,
}) {
  eventCounter += 1;
  const content = {
    type,
    genesisRef,
    acceptedEventId,
    seq,
    transitionType,
    granteePubkey,
  };
  if (role !== undefined) content.role = role;
  if (extraContent) Object.assign(content, extraContent);
  return finalizeEvent(
    {
      created_at: 2000 + eventCounter,
      kind: 40099,
      tags: [["h", channelId]],
      content: JSON.stringify(content),
    },
    signerSecret,
  );
}

function foldRoster(input) {
  return foldCodingSessionRoster({
    expectedChannel: CHANNEL_ID,
    trustedRelayPubkey: RELAY,
    genesisRef: GENESIS_REF,
    ...input,
  });
}

function acceptedChain(links) {
  const transitions = [];
  const receipts = [];
  let prev = null;
  links.forEach((link, index) => {
    const seq = index + 1;
    const transition = transitionEvent({
      seq,
      prevAccepted: prev,
      type: link.type,
      granteePubkey: link.granteePubkey,
      role: link.role,
      extraContent: link.extraTransitionContent,
    });
    transitions.push(transition);
    receipts.push(
      receiptEvent({
        acceptedEventId: transition.id,
        seq,
        transitionType: link.receiptType ?? link.type,
        granteePubkey: link.receiptGrantee ?? link.granteePubkey,
        role: Object.hasOwn(link, "receiptRole") ? link.receiptRole : link.role,
        extraContent: link.extraReceiptContent,
        channelId: link.receiptChannelId,
      }),
    );
    prev = transition.id;
  });
  return { transitions, receipts };
}

test("empty chain folds to an empty roster with no head", () => {
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions: [],
    receipts: [],
  });
  assert.equal(fold.accepted.size, 0);
  assert.equal(fold.acceptedHead, null);
  assert.deepEqual(fold.pending, []);
});

test("accepted grants fold in seq order; revoke removes the live grant", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "grant-viewer", granteePubkey: BOB },
    { type: "revoke", granteePubkey: ALICE },
  ]);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[BOB, "viewer"]]);
  assert.deepEqual(fold.acceptedHead, {
    eventId: transitions[2].id,
    seq: 3,
  });
  assert.deepEqual(fold.pending, []);
});

test("a grant re-issued for the same pubkey updates its role", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "grant-viewer", granteePubkey: ALICE },
  ]);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "viewer"]]);
});

test("accepted seat links advance the head without entering the legacy roster", () => {
  const { transitions, receipts } = acceptedChain([
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
    },
    {
      type: "revoke-seat",
      granteePubkey: ALICE,
      role: "verifier",
    },
    { type: "grant-viewer", granteePubkey: BOB },
  ]);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[BOB, "viewer"]]);
  assert.deepEqual(fold.acceptedHead, {
    eventId: transitions[2].id,
    seq: 3,
  });
  assert.deepEqual(fold.pending, []);
});

test("seat links fail closed on role mismatch, invalid role, or extra facts", () => {
  const attacks = [
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
      receiptRole: "builder",
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "Not_Normalized",
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
      receiptRole: undefined,
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
      extraTransitionContent: { authority: "founder" },
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
      extraReceiptContent: { authority: "relay" },
    },
    {
      type: "grant-seat",
      granteePubkey: ALICE,
      role: "verifier",
      receiptChannelId: "8f67f00c-4eef-49e5-97e7-4fbed178de4f",
    },
  ];

  for (const attack of attacks) {
    const { transitions, receipts } = acceptedChain([attack]);
    const fold = foldRoster({
      genesisRef: GENESIS_REF,
      transitions,
      receipts,
    });
    assert.equal(fold.acceptedHead, null);
    assert.equal(fold.accepted.size, 0);
  }
});

test("legacy links reject a seat role smuggled into either exact shape", () => {
  const attacks = [
    {
      type: "grant-viewer",
      granteePubkey: ALICE,
      role: "verifier",
      receiptRole: undefined,
    },
    {
      type: "grant-viewer",
      granteePubkey: ALICE,
      extraReceiptContent: { role: "verifier" },
    },
  ];

  for (const attack of attacks) {
    const { transitions, receipts } = acceptedChain([attack]);
    const fold = foldRoster({
      genesisRef: GENESIS_REF,
      transitions,
      receipts,
    });
    assert.equal(fold.acceptedHead, null);
    assert.equal(fold.accepted.size, 0);
  }
});

test("a transition with no receipt past the head is pending, not accepted", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
  ]);
  const pending = transitionEvent({
    seq: 2,
    prevAccepted: transitions[0].id,
    type: "grant-viewer",
    granteePubkey: BOB,
  });
  transitions.push(pending);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: transitions[0].id, seq: 1 });
  assert.deepEqual(fold.pending, [
    { pubkey: BOB, role: "viewer", eventId: pending.id },
  ]);
});

test("an unknown transition type stops the fold at that link (fail closed)", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "transfer", granteePubkey: BOB },
    { type: "grant-viewer", granteePubkey: BOB },
  ]);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: transitions[0].id, seq: 1 });
});

test("a receipt whose facts disagree with its transition stops the fold", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    {
      type: "grant-viewer",
      granteePubkey: BOB,
      // Receipt claims a different transition type than the signed link.
      receiptType: "grant-operator",
    },
  ]);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: transitions[0].id, seq: 1 });
});

test("transitions and receipts for another genesis are ignored", () => {
  const foreign = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
    granteePubkey: ALICE,
    genesisRef: "c3".repeat(32),
  });
  const foreignReceipt = receiptEvent({
    acceptedEventId: foreign.id,
    seq: 1,
    transitionType: "grant-operator",
    granteePubkey: ALICE,
    genesisRef: "c3".repeat(32),
  });
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions: [foreign],
    receipts: [foreignReceipt],
  });
  assert.equal(fold.accepted.size, 0);
  assert.equal(fold.acceptedHead, null);
  assert.deepEqual(fold.pending, []);
});

test("fully signed transitions and receipts from another channel are ignored", () => {
  const foreignChannel = "8f67f00c-4eef-49e5-97e7-4fbed178de4f";
  const transition = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
    granteePubkey: ALICE,
    channelId: foreignChannel,
  });
  const receipt = receiptEvent({
    acceptedEventId: transition.id,
    seq: 1,
    transitionType: "grant-operator",
    granteePubkey: ALICE,
    channelId: foreignChannel,
  });

  const fold = foldRoster({ transitions: [transition], receipts: [receipt] });
  assert.equal(fold.acceptedHead, null);
  assert.equal(fold.accepted.size, 0);
});

test("forged transition and receipt signatures cannot advance the head", () => {
  const validTransition = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
    granteePubkey: ALICE,
  });
  const validReceipt = receiptEvent({
    acceptedEventId: validTransition.id,
    seq: 1,
    transitionType: "grant-operator",
    granteePubkey: ALICE,
  });
  const transitionPayload = JSON.parse(validTransition.content);
  const forgedTransition = {
    ...validTransition,
    content: JSON.stringify({ ...transitionPayload, granteePubkey: BOB }),
  };
  const forgedReceipt = {
    ...validReceipt,
    content: `${validReceipt.content} `,
  };

  const transitionFold = foldRoster({
    transitions: [forgedTransition],
    receipts: [validReceipt],
  });
  assert.equal(transitionFold.acceptedHead, null);

  const receiptFold = foldRoster({
    transitions: [validTransition],
    receipts: [forgedReceipt],
  });
  assert.equal(receiptFold.acceptedHead, null);
});

test("a valid receipt from any key except the trusted relay is inert", () => {
  const impostorSecret = generateSecretKey();
  const transition = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-viewer",
    granteePubkey: ALICE,
  });
  const impostorReceipt = receiptEvent({
    acceptedEventId: transition.id,
    seq: 1,
    transitionType: "grant-viewer",
    granteePubkey: ALICE,
    signerSecret: impostorSecret,
  });

  const fold = foldRoster({
    transitions: [transition],
    receipts: [impostorReceipt],
  });
  assert.equal(fold.acceptedHead, null);
  assert.equal(fold.accepted.size, 0);
});

test("non-receipt system messages are filtered out client-side", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
  ]);
  receipts.push({
    id: "9f".repeat(32),
    pubkey: "0e".repeat(32),
    created_at: 3000,
    kind: 40099,
    tags: [["h", CHANNEL_ID]],
    content: JSON.stringify({ type: "user_joined", pubkey: BOB }),
    sig: "00".repeat(64),
  });
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
});

test("roster entries pin the founder first as owner and map wire roles", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "grant-viewer", granteePubkey: BOB },
  ]);
  const pending = transitionEvent({
    seq: 3,
    prevAccepted: transitions[1].id,
    type: "grant-operator",
    granteePubkey: "cc".repeat(32),
  });
  transitions.push(pending);
  const fold = foldRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  const entries = codingSessionRosterEntries(FOUNDER, fold);
  assert.deepEqual(entries, [
    { pubkey: FOUNDER, role: "owner" },
    { pubkey: ALICE, role: "collaborator" },
    { pubkey: BOB, role: "viewer" },
    {
      pubkey: "cc".repeat(32),
      role: "collaborator",
      pending: true,
    },
  ]);
});

test("a stray grant naming the founder never demotes the pinned owner row", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-viewer", granteePubkey: FOUNDER },
  ]);
  const entries = codingSessionRosterEntries(
    FOUNDER,
    foldRoster({ transitions, receipts }),
  );
  assert.deepEqual(entries, [{ pubkey: FOUNDER, role: "owner" }]);
});

test("transition builder emits the exact envelope and five-field payload", () => {
  const event = buildCodingSessionAuthorityTransitionEvent({
    channelId: CHANNEL_ID,
    genesisRef: GENESIS_REF,
    prevAccepted: null,
    seq: 1,
    type: "grant-operator",
    granteePubkey: ALICE.toUpperCase(),
  });
  assert.equal(event.kind, 44228);
  assert.deepEqual(event.tags, [
    ["h", CHANNEL_ID],
    ["csat-v", "csat1-1"],
    ["csat-genesis", GENESIS_REF],
  ]);
  assert.deepEqual(JSON.parse(event.content), {
    genesisRef: GENESIS_REF,
    prevAccepted: null,
    seq: 1,
    type: "grant-operator",
    granteePubkey: ALICE,
  });
});

test("transition builder rejects seq/prevAccepted disagreement", () => {
  assert.throws(
    () =>
      buildCodingSessionAuthorityTransitionEvent({
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        prevAccepted: null,
        seq: 2,
        type: "revoke",
        granteePubkey: ALICE,
      }),
    /seq must be exactly 1 if and only if prevAccepted is null/,
  );
  assert.throws(
    () =>
      buildCodingSessionAuthorityTransitionEvent({
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        prevAccepted: "01".repeat(32),
        seq: 1,
        type: "revoke",
        granteePubkey: ALICE,
      }),
    /seq must be exactly 1 if and only if prevAccepted is null/,
  );
});

test("transition builder rejects sequence values outside u32", () => {
  assert.throws(
    () =>
      buildCodingSessionAuthorityTransitionEvent({
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        prevAccepted: "01".repeat(32),
        seq: 0x1_0000_0000,
        type: "revoke",
        granteePubkey: ALICE,
      }),
    /seq must be a u32 integer starting at 1/,
  );
});

test("role mapping: collaborator ⇒ grant-operator, viewer ⇒ grant-viewer", () => {
  assert.equal(grantTypeForEntityRole("collaborator"), "grant-operator");
  assert.equal(grantTypeForEntityRole("viewer"), "grant-viewer");
  assert.throws(() => grantTypeForEntityRole("owner"), /one owner/);
});

test("publish extends the current head: seq = head+1, prevAccepted = head id", async () => {
  const signed = [];
  const published = [];
  await publishCodingSessionAuthorityTransition(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      type: "grant-operator",
      granteePubkey: ALICE,
    },
    {
      fetchFold: async () => ({
        accepted: new Map(),
        acceptedHead: { eventId: "07".repeat(32), seq: 7 },
        pending: [],
      }),
      signer: async (input) => {
        signed.push(input);
        return {
          ...input,
          id: "ee".repeat(32),
          pubkey: FOUNDER,
          created_at: 1,
          sig: "",
        };
      },
      publisher: {
        async publishEvent(event) {
          published.push(event);
          return event;
        },
      },
    },
  );
  assert.equal(signed.length, 1);
  assert.equal(published.length, 1);
  const payload = JSON.parse(signed[0].content);
  assert.equal(payload.seq, 8);
  assert.equal(payload.prevAccepted, "07".repeat(32));
});

test("publish refuses an exhausted u32 head before signing or emitting", async () => {
  let fetches = 0;
  let signs = 0;
  let publishes = 0;

  await assert.rejects(
    publishCodingSessionAuthorityTransition(
      {
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        type: "grant-operator",
        granteePubkey: ALICE,
      },
      {
        fetchFold: async () => {
          fetches += 1;
          return {
            accepted: new Map(),
            acceptedHead: {
              eventId: "ff".repeat(32),
              seq: 0xffff_ffff,
            },
            pending: [],
          };
        },
        signer: async (input) => {
          signs += 1;
          return {
            ...input,
            id: "ee".repeat(32),
            pubkey: FOUNDER,
            created_at: 1,
            sig: "",
          };
        },
        publisher: {
          async publishEvent(event) {
            publishes += 1;
            return event;
          },
        },
      },
    ),
    /exhausted at its u32 maximum/,
  );
  assert.equal(fetches, 1);
  assert.equal(signs, 0);
  assert.equal(publishes, 0);
});

test("an accepted seat link supplies prevAccepted and seq to later legacy grant and revoke", async () => {
  const firstSeat = {
    type: "grant-seat",
    granteePubkey: ALICE,
    role: "builder",
  };
  const cases = [
    {
      links: [firstSeat],
      type: "grant-viewer",
      expectedSeq: 2,
    },
    {
      links: [{ type: "grant-operator", granteePubkey: BOB }, firstSeat],
      type: "revoke",
      expectedSeq: 3,
    },
  ];

  for (const scenario of cases) {
    const { transitions, receipts } = acceptedChain(scenario.links);
    const fold = foldRoster({
      genesisRef: GENESIS_REF,
      transitions,
      receipts,
    });
    const signed = [];
    await publishCodingSessionAuthorityTransition(
      {
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        type: scenario.type,
        granteePubkey: BOB,
      },
      {
        fetchFold: async () => fold,
        signer: async (input) => {
          signed.push(input);
          return {
            ...input,
            id: "ee".repeat(32),
            pubkey: FOUNDER,
            created_at: 1,
            sig: "00".repeat(64),
          };
        },
        publisher: {
          async publishEvent(event) {
            return event;
          },
        },
      },
    );
    assert.equal(signed.length, 1);
    assert.deepEqual(JSON.parse(signed[0].content), {
      genesisRef: GENESIS_REF,
      prevAccepted: transitions.at(-1).id,
      seq: scenario.expectedSeq,
      type: scenario.type,
      granteePubkey: BOB,
    });
  }
});

test("a head/seq relay refusal refetches the head and retries exactly once", async () => {
  const heads = [
    { eventId: "01".repeat(32), seq: 1 },
    { eventId: "02".repeat(32), seq: 2 },
  ];
  let fetches = 0;
  const publishedSeqs = [];
  const result = await publishCodingSessionAuthorityTransition(
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      type: "grant-viewer",
      granteePubkey: BOB,
    },
    {
      fetchFold: async () => {
        const head = heads[Math.min(fetches, heads.length - 1)];
        fetches += 1;
        return { accepted: new Map(), acceptedHead: head, pending: [] };
      },
      signer: async (input) => ({
        ...input,
        id: "ee".repeat(32),
        pubkey: FOUNDER,
        created_at: 1,
        sig: "",
      }),
      publisher: {
        async publishEvent(event) {
          const payload = JSON.parse(event.content);
          publishedSeqs.push(payload.seq);
          if (publishedSeqs.length === 1) {
            throw new Error(
              "invalid: prevAccepted does not match the chain's current head (expected 02…)",
            );
          }
          return event;
        },
      },
    },
  );
  assert.equal(fetches, 2);
  assert.deepEqual(publishedSeqs, [2, 3]);
  assert.equal(result.kind, 44228);
});

test("a non-linkage relay refusal is not retried", async () => {
  let publishes = 0;
  await assert.rejects(
    publishCodingSessionAuthorityTransition(
      {
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        type: "revoke",
        granteePubkey: BOB,
      },
      {
        fetchFold: async () => ({
          accepted: new Map(),
          acceptedHead: { eventId: "01".repeat(32), seq: 1 },
          pending: [],
        }),
        signer: async (input) => ({
          ...input,
          id: "ee".repeat(32),
          pubkey: FOUNDER,
          created_at: 1,
          sig: "",
        }),
        publisher: {
          async publishEvent() {
            publishes += 1;
            throw new Error(
              "invalid: revoke names a pubkey with no live grant",
            );
          },
        },
      },
    ),
    /no live grant/,
  );
  assert.equal(publishes, 1);
});

test("head-conflict detection matches the relay's linkage refusals only", () => {
  assert.equal(
    isCodingSessionAuthorityHeadConflict(
      new Error(
        "invalid: prevAccepted does not match the chain's current head",
      ),
    ),
    true,
  );
  assert.equal(
    isCodingSessionAuthorityHeadConflict(
      new Error("invalid: seq does not extend the chain (expected 4)"),
    ),
    true,
  );
  assert.equal(
    isCodingSessionAuthorityHeadConflict(new Error("Timed out")),
    false,
  );
  assert.equal(isCodingSessionAuthorityHeadConflict("prevAccepted"), false);
});
