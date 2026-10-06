import assert from "node:assert/strict";
import test from "node:test";
import { finalizeEvent, getEventHash, getPublicKey } from "nostr-tools/pure";

import {
  clearSignatureCheckCache,
  hasValidSignature,
  resolveEventAuthorPubkey,
  signatureCheckCacheSize,
} from "./authors.ts";

const SIGNER_SECRET = new Uint8Array(32).fill(1);
const RELAY_SECRET = new Uint8Array(32).fill(2);
const SIGNER = getPublicKey(SIGNER_SECRET);
const RELAY = getPublicKey(RELAY_SECRET);
const ATTRIBUTED_USER = "33".repeat(32);
const CHANNEL_ID = "36411e44-0e2d-4cfe-bd6e-567eb169db9f";

function resolve({
  signer = "user",
  tags,
  relaySelfPubkey = RELAY,
  preferActorTag = true,
  tamperAfterSigning = false,
}) {
  const event = finalizeEvent(
    {
      kind: 9,
      created_at: 1_700_000_000,
      content: "hello",
      tags,
    },
    signer === "relay" ? RELAY_SECRET : SIGNER_SECRET,
  );
  const eventToResolve = tamperAfterSigning
    ? { ...JSON.parse(JSON.stringify(event)), content: "tampered" }
    : event;

  return resolveEventAuthorPubkey({
    event: eventToResolve,
    preferActorTag,
    relaySelfPubkey,
    requireChannelTagForPTags: true,
  });
}

test("user-signed actor tag cannot replace the visible author", () => {
  assert.equal(
    resolve({
      tags: [
        ["h", CHANNEL_ID],
        ["actor", ATTRIBUTED_USER],
      ],
    }),
    SIGNER,
  );
});

test("user-signed first p tag cannot replace the visible author", () => {
  assert.equal(
    resolve({
      tags: [
        ["p", ATTRIBUTED_USER],
        ["h", CHANNEL_ID],
      ],
    }),
    SIGNER,
  );
});

test("relay-signed actor tag resolves to the delegated author", () => {
  assert.equal(
    resolve({
      signer: "relay",
      tags: [
        ["h", CHANNEL_ID],
        ["actor", ATTRIBUTED_USER],
      ],
    }),
    ATTRIBUTED_USER,
  );
});

test("relay-signed author p tag resolves to the delegated author", () => {
  assert.equal(
    resolve({
      signer: "relay",
      preferActorTag: false,
      tags: [
        ["p", ATTRIBUTED_USER],
        ["h", CHANNEL_ID],
      ],
    }),
    ATTRIBUTED_USER,
  );
});

test("missing or malformed relay identity fails closed to the signer", () => {
  const tags = [
    ["h", CHANNEL_ID],
    ["actor", ATTRIBUTED_USER],
  ];

  assert.equal(resolve({ tags, relaySelfPubkey: null }), SIGNER);
  assert.equal(resolve({ tags, relaySelfPubkey: "not-a-pubkey" }), SIGNER);
});

test("malformed relay-signed attribution fails closed to the signer", () => {
  assert.equal(
    resolve({
      signer: "relay",
      tags: [
        ["h", CHANNEL_ID],
        ["actor", "not-a-pubkey"],
      ],
    }),
    RELAY,
  );
});

test("invalid relay event signature fails closed to the signer", () => {
  assert.equal(
    resolve({
      signer: "relay",
      tags: [
        ["h", CHANNEL_ID],
        ["actor", ATTRIBUTED_USER],
      ],
      tamperAfterSigning: true,
    }),
    RELAY,
  );
});

test("signature memoization checks exact bytes after nested mutations", async () => {
  const { hasValidSignature } = await import("./authors.ts");
  const event = finalizeEvent(
    {
      kind: 9,
      created_at: 1700000000,
      content: "original",
      tags: [["h", "channel"]],
    },
    SIGNER_SECRET,
  );
  assert.equal(hasValidSignature(event), true);
  assert.equal(hasValidSignature(event), true);
  event.tags[0][1] = "different-channel";
  assert.equal(hasValidSignature(event), false);
  event.tags[0][1] = "channel";
  assert.equal(hasValidSignature(event), true);
  event.content = "tampered";
  assert.equal(hasValidSignature(event), false);
});

// Ledger 310: the signature cache is keyed by (id, sig), so fresh copies of the
// same event from each IPC response hit it, while tampering still fails.
test("signature cache hits across distinct copies of one event", () => {
  clearSignatureCheckCache();
  const event = finalizeEvent(
    { kind: 1, created_at: 1_700_000_001, content: "cached", tags: [] },
    SIGNER_SECRET,
  );
  const first = JSON.parse(JSON.stringify(event));
  const second = JSON.parse(JSON.stringify(event));
  assert.equal(hasValidSignature(first), true);
  assert.equal(signatureCheckCacheSize(), 1);
  assert.equal(hasValidSignature(second), true);
  assert.equal(signatureCheckCacheSize(), 1);
});

test("a cached id and sig over tampered content is still rejected", () => {
  clearSignatureCheckCache();
  const event = finalizeEvent(
    { kind: 1, created_at: 1_700_000_002, content: "original", tags: [] },
    SIGNER_SECRET,
  );
  assert.equal(hasValidSignature(JSON.parse(JSON.stringify(event))), true);
  const tampered = { ...JSON.parse(JSON.stringify(event)), content: "forged" };
  assert.equal(hasValidSignature(tampered), false);
  const retagged = {
    ...JSON.parse(JSON.stringify(event)),
    tags: [["p", ATTRIBUTED_USER]],
  };
  assert.equal(hasValidSignature(retagged), false);
  // The untouched event still verifies after the forgeries were refused.
  assert.equal(hasValidSignature(JSON.parse(JSON.stringify(event))), true);
});

test("a bad signature under a correct id is cached as invalid", () => {
  clearSignatureCheckCache();
  const event = finalizeEvent(
    { kind: 1, created_at: 1_700_000_003, content: "bad sig", tags: [] },
    SIGNER_SECRET,
  );
  const badSig = { ...JSON.parse(JSON.stringify(event)), sig: "11".repeat(64) };
  assert.equal(hasValidSignature(badSig), false);
  assert.equal(hasValidSignature({ ...badSig }), false);
  assert.equal(signatureCheckCacheSize(), 1);
  assert.equal(hasValidSignature(JSON.parse(JSON.stringify(event))), true);
});

test("signature cache is size-capped", () => {
  clearSignatureCheckCache();
  const pubkey = SIGNER;
  for (let i = 0; i < 20_005; i += 1) {
    const unsigned = {
      pubkey,
      created_at: 1_700_000_000 + i,
      kind: 1,
      tags: [],
      content: "",
    };
    // Correct id, garbage signature: exercises the cache without signing.
    hasValidSignature({
      ...unsigned,
      id: getEventHash(unsigned),
      sig: "ff".repeat(64),
    });
  }
  assert.equal(signatureCheckCacheSize(), 20_000);
  clearSignatureCheckCache();
});
