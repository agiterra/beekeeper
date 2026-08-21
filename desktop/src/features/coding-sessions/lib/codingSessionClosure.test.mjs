import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";
import {
  buildCodingSessionClosureEvent,
  buildCodingSessionClosureFilter,
  codingSessionClosureKey,
  foldAuthorizedCodingSessionClosures,
  parseCodingSessionClosure,
  publishCodingSessionClosure,
} from "./codingSessionClosure.ts";

const CHANNEL_ID = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const FOUNDER_KEY = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_KEY);
const MEMBER_KEY = generateSecretKey();
const MEMBER_PUBKEY = getPublicKey(MEMBER_KEY);

function closure(action, created_at, key = FOUNDER_KEY) {
  return finalizeEvent(
    {
      ...buildCodingSessionClosureEvent({
        action,
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        sessionRef: SESSION_REF,
      }),
      created_at,
    },
    key,
  );
}

function signedWith({ action = "closed", content, tags } = {}) {
  const built = buildCodingSessionClosureEvent({
    action,
    channelId: CHANNEL_ID,
    genesisRef: GENESIS_REF,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      ...built,
      content: content ?? built.content,
      tags: tags ?? built.tags,
      created_at: 1_800_000_000,
    },
    FOUNDER_KEY,
  );
}

test("closure uses the exact payload, ordered tags, and scoped filter", () => {
  assert.deepEqual(
    buildCodingSessionClosureEvent({
      action: "closed",
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      sessionRef: SESSION_REF,
    }),
    {
      kind: 44230,
      content: JSON.stringify({
        action: "closed",
        genesisRef: GENESIS_REF,
        sessionRef: SESSION_REF,
        v: 1,
      }),
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["cscl-v", "cscl1-1"],
        ["cscl-genesis", GENESIS_REF],
      ],
    },
  );
  assert.deepEqual(buildCodingSessionClosureFilter([CHANNEL_ID]), {
    kinds: [44230],
    "#h": [CHANNEL_ID],
    limit: 1000,
  });
});

test("closure parser rejects smuggled fields, mismatches, and invalid signatures", () => {
  const valid = closure("closed", 10);
  assert.equal(parseCodingSessionClosure(valid)?.action, "closed");

  const built = buildCodingSessionClosureEvent({
    action: "closed",
    channelId: CHANNEL_ID,
    genesisRef: GENESIS_REF,
    sessionRef: SESSION_REF,
  });
  assert.equal(
    parseCodingSessionClosure(
      signedWith({ tags: [...built.tags, ["p", FOUNDER_PUBKEY]] }),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionClosure(
      signedWith({
        content: JSON.stringify({
          action: "closed",
          genesisRef: GENESIS_REF,
          sessionRef: SESSION_REF,
          v: 1,
          hidden: true,
        }),
      }),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionClosure(
      signedWith({
        content: JSON.stringify({
          action: "open",
          genesisRef: "b".repeat(64),
          sessionRef: SESSION_REF,
          v: 1,
        }),
      }),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionClosure({ ...valid, sig: "0".repeat(128) }),
    null,
  );
});

test("authorized fold permits member reopen but only the founder may close", () => {
  const founderClosed = closure("closed", 10);
  const memberOpened = closure("open", 11, MEMBER_KEY);
  const memberClosed = closure("closed", 12, MEMBER_KEY);
  const founders = new Map([[GENESIS_REF, FOUNDER_PUBKEY]]);
  const key = codingSessionClosureKey(CHANNEL_ID, SESSION_REF, GENESIS_REF);

  const folded = foldAuthorizedCodingSessionClosures(
    [memberClosed, memberOpened, founderClosed],
    founders,
  );
  assert.equal(folded.get(key)?.action, "open");
  assert.equal(folded.get(key)?.signerPubkey, MEMBER_PUBKEY);
  assert.equal(folded.get(key)?.founderPubkey, FOUNDER_PUBKEY);
});

test("authorized fold fails closed until the explicit genesis resolves", () => {
  const memberOpened = closure("open", 11, MEMBER_KEY);
  assert.equal(
    foldAuthorizedCodingSessionClosures([memberOpened], new Map()).size,
    0,
  );
  assert.equal(
    foldAuthorizedCodingSessionClosures(
      [memberOpened],
      new Map([[GENESIS_REF, "not-a-pubkey"]]),
    ).size,
    0,
  );
});

test("authorized revisions fold deterministically by timestamp then event id", () => {
  const tiedClosed = closure("closed", 20);
  const tiedOpen = closure("open", 20, MEMBER_KEY);
  const expected = tiedClosed.id > tiedOpen.id ? "closed" : "open";
  const folded = foldAuthorizedCodingSessionClosures(
    [tiedOpen, tiedClosed],
    new Map([[GENESIS_REF, FOUNDER_PUBKEY]]),
  );
  assert.equal([...folded.values()][0]?.action, expected);
});

test("closure validates identifiers and publishes the signed event", async () => {
  assert.throws(
    () =>
      buildCodingSessionClosureEvent({
        action: "open",
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF.toUpperCase(),
        sessionRef: SESSION_REF,
      }),
    /canonical lowercase event id/,
  );
  const signed = closure("open", 10, MEMBER_KEY);
  let published = null;
  const accepted = await publishCodingSessionClosure(
    {
      action: "open",
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      sessionRef: SESSION_REF,
    },
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
