import assert from "node:assert/strict";
import test from "node:test";

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
const FOUNDER = "f0".repeat(32);
const ALICE = "aa".repeat(32);
const BOB = "bb".repeat(32);

function hexId(prefix) {
  return prefix.repeat(2).slice(0, 2).repeat(32);
}

let eventCounter = 0;
function transitionEvent({
  id,
  seq,
  prevAccepted,
  type,
  granteePubkey,
  genesisRef = GENESIS_REF,
}) {
  eventCounter += 1;
  return {
    id,
    pubkey: FOUNDER,
    created_at: 1000 + eventCounter,
    kind: 44228,
    tags: [
      ["h", CHANNEL_ID],
      ["csat-v", "csat1-1"],
      ["csat-genesis", genesisRef],
    ],
    content: JSON.stringify({
      genesisRef,
      prevAccepted,
      seq,
      type,
      granteePubkey,
    }),
    sig: "00".repeat(64),
  };
}

function receiptEvent({
  acceptedEventId,
  seq,
  transitionType,
  granteePubkey,
  genesisRef = GENESIS_REF,
  type = "coding_session_authority_transition_accepted",
}) {
  eventCounter += 1;
  return {
    id: hexId(String(eventCounter % 10)),
    pubkey: "0e".repeat(32),
    created_at: 2000 + eventCounter,
    kind: 40099,
    tags: [["h", CHANNEL_ID]],
    content: JSON.stringify({
      type,
      genesisRef,
      acceptedEventId,
      seq,
      transitionType,
      granteePubkey,
    }),
    sig: "00".repeat(64),
  };
}

function acceptedChain(links) {
  const transitions = [];
  const receipts = [];
  let prev = null;
  links.forEach((link, index) => {
    const seq = index + 1;
    const id = link.id ?? `${String(seq).padStart(2, "0")}`.repeat(32);
    transitions.push(
      transitionEvent({
        id,
        seq,
        prevAccepted: prev,
        type: link.type,
        granteePubkey: link.granteePubkey,
      }),
    );
    receipts.push(
      receiptEvent({
        acceptedEventId: id,
        seq,
        transitionType: link.receiptType ?? link.type,
        granteePubkey: link.receiptGrantee ?? link.granteePubkey,
      }),
    );
    prev = id;
  });
  return { transitions, receipts };
}

test("empty chain folds to an empty roster with no head", () => {
  const fold = foldCodingSessionRoster({
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
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[BOB, "viewer"]]);
  assert.deepEqual(fold.acceptedHead, {
    eventId: "03".repeat(32),
    seq: 3,
  });
  assert.deepEqual(fold.pending, []);
});

test("a grant re-issued for the same pubkey updates its role", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "grant-viewer", granteePubkey: ALICE },
  ]);
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "viewer"]]);
});

test("a transition with no receipt past the head is pending, not accepted", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
  ]);
  transitions.push(
    transitionEvent({
      id: "e2".repeat(32),
      seq: 2,
      prevAccepted: "01".repeat(32),
      type: "grant-viewer",
      granteePubkey: BOB,
    }),
  );
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: "01".repeat(32), seq: 1 });
  assert.deepEqual(fold.pending, [
    { pubkey: BOB, role: "viewer", eventId: "e2".repeat(32) },
  ]);
});

test("an unknown transition type stops the fold at that link (fail closed)", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-operator", granteePubkey: ALICE },
    { type: "transfer", granteePubkey: BOB },
    { type: "grant-viewer", granteePubkey: BOB },
  ]);
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: "01".repeat(32), seq: 1 });
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
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  assert.deepEqual([...fold.accepted.entries()], [[ALICE, "operator"]]);
  assert.deepEqual(fold.acceptedHead, { eventId: "01".repeat(32), seq: 1 });
});

test("transitions and receipts for another genesis are ignored", () => {
  const foreign = transitionEvent({
    id: "d1".repeat(32),
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
    granteePubkey: ALICE,
    genesisRef: "c3".repeat(32),
  });
  const foreignReceipt = receiptEvent({
    acceptedEventId: "d1".repeat(32),
    seq: 1,
    transitionType: "grant-operator",
    granteePubkey: ALICE,
    genesisRef: "c3".repeat(32),
  });
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions: [foreign],
    receipts: [foreignReceipt],
  });
  assert.equal(fold.accepted.size, 0);
  assert.equal(fold.acceptedHead, null);
  assert.deepEqual(fold.pending, []);
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
  const fold = foldCodingSessionRoster({
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
  transitions.push(
    transitionEvent({
      id: "e3".repeat(32),
      seq: 3,
      prevAccepted: "02".repeat(32),
      type: "grant-operator",
      granteePubkey: "cc".repeat(32),
    }),
  );
  const fold = foldCodingSessionRoster({
    genesisRef: GENESIS_REF,
    transitions,
    receipts,
  });
  const entries = codingSessionRosterEntries(FOUNDER, fold);
  assert.deepEqual(entries, [
    { pubkey: FOUNDER, role: "owner" },
    { pubkey: ALICE, role: "collaborator" },
    { pubkey: BOB, role: "viewer" },
    { pubkey: "cc".repeat(32), role: "collaborator", pending: true },
  ]);
});

test("a stray grant naming the founder never demotes the pinned owner row", () => {
  const { transitions, receipts } = acceptedChain([
    { type: "grant-viewer", granteePubkey: FOUNDER },
  ]);
  const entries = codingSessionRosterEntries(
    FOUNDER,
    foldCodingSessionRoster({ genesisRef: GENESIS_REF, transitions, receipts }),
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
