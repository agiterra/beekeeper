import assert from "node:assert/strict";
import { test } from "node:test";
import { CodingSessionObserverStore } from "./catalog.ts";
import {
  CHANNEL_ID,
  closureEvent,
  corruptSignature,
  createEvent,
  leaseEvent,
  metadataEvent,
  nameEvent,
  newSigner,
  OTHER_SESSION_REF,
  receiptEvent,
  SESSION_REF,
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

test("a lone stop receipt does not mint a generation that never existed", () => {
  const provider = newSigner();
  // The create and its confirming receipt fell outside the history page; only
  // the stop survived. D6 says a generation exists iff created,
  // created_with_failed_initial_turn, resumed, or resumed_without_context
  // names it — a stop names an end, and an end is not an existence proof.
  const facts = storeWith([
    receiptEvent(provider, { commandId: "stop-command", status: "stopped" }),
    metadataEvent(provider, { status: "stopped" }),
  ]);
  assert.deepEqual(facts.generations, []);
});

test("a stop still lands on a generation a create established", () => {
  const operator = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    ...establishedGeneration(operator, provider),
    receiptEvent(provider, {
      commandId: "stop-command",
      status: "stopped",
      created_at: 1_700_000_500,
    }),
    metadataEvent(provider, { status: "stopped" }),
  ]);
  assert.equal(facts.generations.length, 1);
  assert.equal(
    facts.generations[0].lastEventAt,
    1_700_000_500 * 1000,
    "the stop still carries the stream's last activity",
  );
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

test("two creates under one commandId that disagree bind neither, and the session still renders", () => {
  const operator = newSigner();
  const impostor = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    createEvent(operator, {
      commandId: "command-1",
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
    }),
    // The commandId is public in the channel, so copying it is trivial.
    createEvent(impostor, {
      commandId: "command-1",
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: OTHER_SESSION_REF,
    }),
    receiptEvent(provider, { commandId: "command-1", status: "created" }),
    metadataEvent(provider, { status: "running" }),
  ]);
  assert.equal(
    facts.generations.length,
    1,
    "a copied commandId must never make a live session disappear",
  );
  assert.equal(
    facts.generations[0].authoritySource,
    "disclosed-fallback",
    "with no create left to believe, the authority is disclosed as unverified",
  );
  assert.equal(
    facts.conflictCount,
    1,
    "and the dispute is disclosed, not hidden",
  );
  assert.deepEqual(facts.creates, [], "neither create binds anything");
});

test("republishing the same create under one commandId is not a dispute", () => {
  const operator = newSigner();
  const provider = newSigner();
  const facts = storeWith([
    createEvent(operator, {
      commandId: "command-1",
      providerAuthorityPubkey: provider.pubkey,
      created_at: 1_699_999_000,
    }),
    createEvent(operator, {
      commandId: "command-1",
      providerAuthorityPubkey: provider.pubkey,
      created_at: 1_699_999_100,
    }),
    receiptEvent(provider, { commandId: "command-1", status: "created" }),
    metadataEvent(provider),
  ]);
  assert.equal(facts.conflictCount, 0);
  assert.equal(facts.generations[0].authoritySource, "create");
  assert.equal(facts.creates.length, 1);
  assert.equal(
    facts.creates[0].createdAt,
    1_699_999_000,
    "the earliest signed claim wins, never whichever arrived first",
  );
});

test("a create no provider answered is not exported as a fact", () => {
  const operator = newSigner();
  const provider = newSigner();
  const stranger = newSigner();
  const facts = storeWith([
    createEvent(operator, {
      commandId: "never-answered",
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
    }),
    // A receipt from anyone but the provider the create named joins nothing.
    receiptEvent(stranger, {
      commandId: "never-answered",
      status: "created",
    }),
  ]);
  assert.deepEqual(facts.creates, []);
});

test("a forged name, closure, create or lease is counted and never becomes a fact", () => {
  const operator = newSigner();
  const provider = newSigner();
  const founder = newSigner();
  const store = new CodingSessionObserverStore();
  store.ingest(
    [
      ...establishedGeneration(operator, provider, {
        sessionRef: SESSION_REF,
      }),
      corruptSignature(nameEvent(founder, { sessionRef: SESSION_REF })),
      corruptSignature(closureEvent(founder, { sessionRef: SESSION_REF })),
      corruptSignature(
        createEvent(founder, {
          commandId: "command-1",
          providerAuthorityPubkey: provider.pubkey,
        }),
      ),
      corruptSignature(leaseEvent(provider)),
    ],
    channels,
  );
  const facts = store.facts(channels);
  assert.equal(facts.invalidSignatureCount, 4);
  assert.deepEqual(facts.names, []);
  assert.deepEqual(facts.closures, []);
  assert.equal(facts.leasesByTarget.size, 0);
  assert.equal(
    facts.creates.length,
    1,
    "only the genuinely signed create survives",
  );
  assert.equal(facts.creates[0].signerPubkey, operator.pubkey);
});
