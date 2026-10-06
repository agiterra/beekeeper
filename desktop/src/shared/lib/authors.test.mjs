import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  getEventHash,
  getPublicKey,
  verifyEvent,
} from "nostr-tools/pure";

import {
  clearSignatureCheckCache,
  hasValidSignature,
  resolveEventAuthorPubkey,
  signatureCheckCacheSize,
  verifyEventSignatures,
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

// SV-117: batched verification off the main thread. The stand-in below keeps
// the native command's contract (desktop/src-tauri/src/commands/
// event_signatures.rs): recompute the id from the fields received, then check
// the signature; an id that does not hash its fields is `unchecked`.
function nativeStandIn(calls = []) {
  return async (events) => {
    calls.push(events.length);
    return events.map((event) => {
      if (getEventHash(event) !== event.id) return "unchecked";
      return verifyEvent({ ...event }) ? "valid" : "invalid-signature";
    });
  };
}

function signedCopy(content, createdAt = 1_700_100_000) {
  return JSON.parse(
    JSON.stringify(
      finalizeEvent(
        { kind: 44_201, created_at: createdAt, content, tags: [["h", "c"]] },
        SIGNER_SECRET,
      ),
    ),
  );
}

test("batch verdicts follow input order and fill the cache", async () => {
  clearSignatureCheckCache();
  const calls = [];
  const events = [];
  const expected = [];
  for (let i = 0; i < 12; i += 1) {
    const event = signedCopy(`event ${i}`, 1_700_100_000 + i);
    if (i % 3 === 1) event.content = "tampered";
    if (i % 3 === 2) event.sig = "11".repeat(64);
    events.push(event);
    expected.push(i % 3 === 0);
  }
  assert.deepEqual(
    await verifyEventSignatures(events, { verifier: nativeStandIn(calls) }),
    expected,
  );
  assert.deepEqual(calls, [12]);
  // Valid and bad-signature verdicts are cached; tampered content is not.
  assert.equal(signatureCheckCacheSize(), 8);
  for (const [i, event] of events.entries()) {
    assert.equal(hasValidSignature(event), expected[i]);
  }
});

test("tampered content and tampered signatures fail the batch", async () => {
  clearSignatureCheckCache();
  const good = signedCopy("original");
  const badContent = { ...good, content: "forged" };
  const badSig = { ...good, sig: "22".repeat(64) };
  assert.deepEqual(
    await verifyEventSignatures([badContent, badSig, good], {
      verifier: nativeStandIn(),
    }),
    [false, false, true],
  );
});

test("a verified id and sig cannot vouch for different content", async () => {
  clearSignatureCheckCache();
  const real = signedCopy("real");
  const forged = { ...real, content: "forged", tags: [["p", ATTRIBUTED_USER]] };
  // Real first: the forgery shares its (id, sig) key, yet is re-hashed.
  assert.deepEqual(
    await verifyEventSignatures([real, forged], { verifier: nativeStandIn() }),
    [true, false],
  );
  assert.equal(hasValidSignature(forged), false);

  // Forgery first: its `unchecked` answer is never cached, so it cannot
  // knock out the real event that arrives after it.
  clearSignatureCheckCache();
  assert.deepEqual(
    await verifyEventSignatures([forged, real], { verifier: nativeStandIn() }),
    [false, true],
  );
  assert.equal(hasValidSignature(real), true);
  assert.equal(hasValidSignature(forged), false);
});

test("an unchecked native answer is settled by the JavaScript check", async () => {
  clearSignatureCheckCache();
  const real = signedCopy("real");
  const verdicts = await verifyEventSignatures([real], {
    verifier: async (events) => events.map(() => "unchecked"),
  });
  assert.deepEqual(verdicts, [true]);
});

test("a failing or malformed native answer falls back to JavaScript", async () => {
  const real = signedCopy("real");
  const forged = { ...real, content: "forged" };
  for (const verifier of [
    async () => {
      throw new Error("ipc down");
    },
    async () => ["valid"],
    null,
  ]) {
    clearSignatureCheckCache();
    assert.deepEqual(
      await verifyEventSignatures([real, forged], { verifier }),
      [true, false],
    );
  }
});

test("cached verdicts are not sent to the native verifier again", async () => {
  clearSignatureCheckCache();
  const calls = [];
  const a = signedCopy("a", 1_700_100_001);
  const b = signedCopy("b", 1_700_100_002);
  await verifyEventSignatures([a], { verifier: nativeStandIn(calls) });
  assert.deepEqual(
    await verifyEventSignatures([JSON.parse(JSON.stringify(a)), b, { ...b }], {
      verifier: nativeStandIn(calls),
    }),
    [true, true, true],
  );
  assert.deepEqual(calls, [1, 1]);
});

test("events the JavaScript check would refuse never reach the native path", async () => {
  clearSignatureCheckCache();
  const calls = [];
  const real = signedCopy("real");
  const upperPubkey = { ...real, pubkey: real.pubkey.toUpperCase() };
  const loneSurrogate = signedCopy("half \ud800 a pair");
  assert.deepEqual(
    await verifyEventSignatures([upperPubkey, loneSurrogate], {
      verifier: nativeStandIn(calls),
    }),
    [false, hasValidSignature(loneSurrogate)],
  );
  assert.deepEqual(calls, []);
});

test("a batch verdict describes the event as it was when checked", async () => {
  clearSignatureCheckCache();
  const event = signedCopy("original");
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const pending = verifyEventSignatures([event], {
    verifier: async (events) => {
      await gate;
      return nativeStandIn()(events);
    },
  });
  event.tags[0][1] = "mutated";
  release();
  assert.deepEqual(await pending, [true]);
  assert.equal(hasValidSignature(event), false);
});

test("large batches are sent in chunks and answered in input order", async () => {
  clearSignatureCheckCache();
  const calls = [];
  const events = [];
  for (let i = 0; i < 1_203; i += 1) {
    const unsigned = {
      pubkey: SIGNER,
      created_at: 1_700_200_000 + i,
      kind: 1,
      tags: [],
      content: String(i),
    };
    events.push({
      ...unsigned,
      id: getEventHash(unsigned),
      sig: "ff".repeat(64),
    });
  }
  // This stand-in answers by position only, to show each answer lands on the
  // event it was given for, across chunk boundaries.
  const verdicts = await verifyEventSignatures(events, {
    verifier: async (batch) => {
      calls.push(batch.length);
      return batch.map((event) =>
        Number(event.content) % 2 === 0 ? "valid" : "invalid-signature",
      );
    },
  });
  assert.deepEqual(calls, [500, 500, 203]);
  for (const [i, verdict] of verdicts.entries()) {
    assert.equal(verdict, i % 2 === 0, `event ${i}`);
  }
  clearSignatureCheckCache();
});
