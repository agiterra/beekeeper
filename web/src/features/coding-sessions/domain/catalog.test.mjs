import assert from "node:assert/strict";
import { test } from "node:test";
import { CodingSessionObserverStore } from "./catalog.ts";
import {
  CHANNEL_ID,
  createEvent,
  metadataEvent,
  newSigner,
  receiptEvent,
  transcriptEvent,
  target,
} from "./testFixtures.mjs";

const channels = [CHANNEL_ID];

function storeWith(events) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  return store.facts(channels);
}

/** A create + its confirming receipt: the minimum a generation needs. */
function establishedGeneration(operator, provider, options = {}) {
  const commandId = options.commandId ?? "command-1";
  return [
    createEvent(operator, {
      commandId,
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: options.sessionRef,
      genesisRef: options.genesisRef,
    }),
    receiptEvent(provider, {
      commandId,
      status: "created",
      target: options.target ?? target(),
    }),
  ];
}

test("a turn receipt never creates a generation", () => {
  const operator = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    createEvent(operator, {
      commandId: "turn-command",
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(provider, {
      commandId: "turn-command",
      status: "turn_started",
      turnId: "t1",
    }),
    metadataEvent(provider),
  ]);
  assert.deepEqual(facts.generations, []);
});

test("a lifecycle receipt joined to its create does create one", () => {
  const operator = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    metadataEvent(provider, { status: "running" }),
  ]);
  assert.equal(facts.generations.length, 1);
  assert.equal(facts.generations[0].status, "running");
  assert.equal(facts.generations[0].providerAuthorityPubkey, provider.pubkey);
  assert.equal(facts.generations[0].authoritySource, "create");
});

test("a receipt signed by anyone but the named provider proves nothing", () => {
  const operator = newSigner();
  const provider = newSigner();
  const impostor = newSigner();
  const facts = storeWith([
    createEvent(operator, {
      commandId: "command-1",
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(impostor, { commandId: "command-1", status: "created" }),
    metadataEvent(impostor),
  ]);
  assert.equal(
    facts.generations.length,
    0,
    "the impostor's receipt must not mint a generation under the create",
  );
});

test("without a readable create the first-seen metadata signer stands in, disclosed", () => {
  const provider = newSigner();
  const facts = storeWith([
    receiptEvent(provider, { commandId: "unseen-command", status: "created" }),
    metadataEvent(provider, { status: "idle" }),
  ]);
  assert.equal(facts.generations.length, 1);
  assert.equal(facts.generations[0].authoritySource, "disclosed-fallback");
  assert.equal(facts.generations[0].providerAuthorityPubkey, provider.pubkey);
});

test("the newest metadata wins, and a same-second tie breaks on event id", () => {
  const operator = newSigner();
  const provider = newSigner();
  const older = metadataEvent(provider, {
    status: "starting",
    created_at: 1_700_000_000,
  });
  const newerA = metadataEvent(provider, {
    status: "running",
    created_at: 1_700_000_010,
  });
  const newerB = metadataEvent(provider, {
    status: "waiting_for_input",
    created_at: 1_700_000_010,
  });
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    older,
    newerA,
    newerB,
  ]);
  const winner = newerA.id < newerB.id ? newerA : newerB;
  const expected = winner === newerA ? "running" : "waiting_for_input";
  assert.equal(facts.generations[0].status, expected);
  assert.equal(
    facts.generations[0].conflictCount,
    1,
    "the disagreement inside one second stays visible",
  );
});

test("ingest order cannot change which metadata wins", () => {
  const operator = newSigner();
  const provider = newSigner();
  const established = establishedGeneration(operator, provider);
  const a = metadataEvent(provider, {
    status: "running",
    created_at: 1_700_000_010,
  });
  const b = metadataEvent(provider, {
    status: "idle",
    created_at: 1_700_000_010,
  });
  const forward = storeWith([...established, a, b]);
  const reversed = storeWith([...established, b, a]);
  assert.equal(forward.generations[0].status, reversed.generations[0].status);
});

test("facts from different signers never merge into one generation", () => {
  const operatorA = newSigner();
  const providerA = newSigner();
  const operatorB = newSigner();
  const providerB = newSigner();
  const facts = storeWith([
    ...establishedGeneration(operatorA, providerA, { commandId: "cmd-a" }),
    metadataEvent(providerA, { status: "running" }),
    ...establishedGeneration(operatorB, providerB, { commandId: "cmd-b" }),
    metadataEvent(providerB, { status: "stopped" }),
  ]);
  assert.equal(facts.generations.length, 2);
  const signers = new Set(
    facts.generations.map((record) => record.providerAuthorityPubkey),
  );
  assert.equal(signers.size, 2);
  const byStatus = Object.fromEntries(
    facts.generations.map((record) => [
      record.providerAuthorityPubkey,
      record.status,
    ]),
  );
  assert.equal(byStatus[providerA.pubkey], "running");
  assert.equal(byStatus[providerB.pubkey], "stopped");
});

test("two distinct transcript payloads at one eventSeq render neither", () => {
  const operator = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    transcriptEvent(provider, {
      eventSeq: 1,
      item: { kind: "assistant_text", text: "one story" },
    }),
    transcriptEvent(provider, {
      eventSeq: 1,
      item: { kind: "assistant_text", text: "another story" },
    }),
    transcriptEvent(provider, {
      eventSeq: 2,
      item: { kind: "assistant_text", text: "undisputed" },
    }),
  ]);
  const transcript = facts.generations[0].transcript;
  assert.deepEqual(
    transcript.map((item) => item.text),
    ["undisputed"],
  );
  assert.equal(facts.generations[0].conflictCount, 1);
});

test("a byte-identical duplicate collapses by event id and is not a conflict", () => {
  const operator = newSigner();
  const provider = newSigner();
  const once = transcriptEvent(provider, { eventSeq: 1 });
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    once,
    { ...once },
  ]);
  assert.equal(facts.generations[0].transcript.length, 1);
  assert.equal(facts.generations[0].conflictCount, 0);
});

test("re-delivery after a reconnect is idempotent", () => {
  const operator = newSigner();
  const provider = newSigner();
  const events = [
    ...establishedGeneration(operator, provider),
    metadataEvent(provider, { status: "running" }),
    transcriptEvent(provider, { eventSeq: 1 }),
  ];
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  store.ingest(events, channels);
  const facts = store.facts(channels);
  assert.equal(facts.generations.length, 1);
  assert.equal(facts.generations[0].transcript.length, 1);
  assert.equal(facts.generations[0].conflictCount, 0);
});

test("raw retention evicts the oldest past the cap", () => {
  const operator = newSigner();
  const provider = newSigner();
  const store = new CodingSessionObserverStore(3);
  store.ingest(
    [
      ...establishedGeneration(operator, provider),
      transcriptEvent(provider, { eventSeq: 1 }),
      transcriptEvent(provider, { eventSeq: 2 }),
      transcriptEvent(provider, { eventSeq: 3 }),
      transcriptEvent(provider, { eventSeq: 4 }),
    ],
    channels,
  );
  const facts = store.facts(channels);
  const targetKey = facts.generations[0].target;
  assert.ok(targetKey);
  const retained = store.retainedRawEvents(
    CHANNEL_ID,
    "coding-session/v1|10:provider-a10:instance-19:session-11:1",
    provider.pubkey,
  );
  assert.equal(retained.length, 3);
});

test("an invalid signature is counted and never reaches a record", () => {
  const operator = newSigner();
  const provider = newSigner();
  const good = metadataEvent(provider, { status: "running" });
  const tampered = {
    ...good,
    content: good.content.replace("running", "idle"),
  };
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    tampered,
  ]);
  assert.equal(facts.invalidSignatureCount, 1);
  assert.equal(facts.generations[0].status, "unknown");
});
