import assert from "node:assert/strict";
import { test } from "node:test";
import { CodingSessionObserverStore } from "./catalog.ts";
import {
  buildCodingSessionObserverSnapshot,
  codingSessionFounderLabel,
  codingSessionReachabilityLine,
  codingSessionStatusChipLabel,
  foldUmbrellaStatus,
} from "./umbrella.ts";
import {
  CHANNEL_ID,
  closureEvent,
  createEvent,
  genesisEvent,
  metadataEvent,
  nameEvent,
  newSigner,
  OTHER_SESSION_REF,
  receiptEvent,
  SESSION_REF,
  target,
} from "./testFixtures.mjs";

const channels = [CHANNEL_ID];

function snapshotOf(events, options = {}) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  return buildCodingSessionObserverSnapshot(store.facts(channels), {
    nowMs: options.nowMs ?? 1_700_000_000_000,
    leasesRead: options.leasesRead ?? false,
    historyTruncated: options.historyTruncated ?? false,
  });
}

/** One execution: a create, its confirming receipt, and its metadata. */
function execution(operator, provider, options = {}) {
  const commandId = options.commandId ?? "command-1";
  const executionTarget =
    options.target ?? target({ sessionId: options.sessionId ?? "session-1" });
  return [
    createEvent(operator, {
      commandId,
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: options.sessionRef,
      genesisRef: options.genesisRef,
      created_at: options.createdAt ?? 1_699_999_000,
    }),
    receiptEvent(provider, {
      commandId,
      status: "created",
      target: executionTarget,
    }),
    metadataEvent(provider, {
      target: executionTarget,
      status: options.status ?? "idle",
      sessionRef: options.sessionRef,
      created_at: options.metadataAt ?? 1_700_000_000,
    }),
  ];
}

test("executions sharing a sessionRef form one umbrella", () => {
  const founder = newSigner();
  const providerA = newSigner();
  const providerB = newSigner();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesis,
    ...execution(founder, providerA, {
      commandId: "cmd-a",
      sessionId: "session-a",
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    ...execution(founder, providerB, {
      commandId: "cmd-b",
      sessionId: "session-b",
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
  ]);
  assert.equal(snapshot.umbrellas.length, 1);
  assert.equal(snapshot.umbrellas[0].executions.length, 2);
  assert.equal(snapshot.umbrellas[0].founderResolution, "governed");
  assert.equal(snapshot.umbrellas[0].founderPubkey, founder.pubkey);
});

test("a record with no sessionRef is an implicit umbrella of one", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf(execution(operator, provider));
  assert.equal(snapshot.umbrellas.length, 1);
  assert.equal(snapshot.umbrellas[0].sessionRef, null);
  assert.equal(snapshot.umbrellas[0].founderResolution, "legacy");
});

test("two distinct genesisRefs make the founder a disclosed conflict", () => {
  const founderA = newSigner();
  const founderB = newSigner();
  const provider = newSigner();
  const genesisA = genesisEvent(founderA, { sessionRef: SESSION_REF });
  const genesisB = genesisEvent(founderB, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesisA,
    genesisB,
    ...execution(founderA, provider, {
      commandId: "cmd-a",
      sessionId: "session-a",
      sessionRef: SESSION_REF,
      genesisRef: genesisA.id,
    }),
    ...execution(founderB, provider, {
      commandId: "cmd-b",
      sessionId: "session-b",
      sessionRef: SESSION_REF,
      genesisRef: genesisB.id,
    }),
  ]);
  const umbrella = snapshot.umbrellas[0];
  assert.equal(umbrella.founderResolution, "conflict");
  assert.equal(umbrella.founderPubkey, null);
  assert.equal(codingSessionFounderLabel(umbrella), "founder conflict");
});

test("a ref-bearing umbrella with no readable create is unresolved, not legacy", () => {
  const provider = newSigner();
  const snapshot = snapshotOf([
    receiptEvent(provider, { commandId: "unseen", status: "created" }),
    metadataEvent(provider, { sessionRef: SESSION_REF }),
  ]);
  const umbrella = snapshot.umbrellas[0];
  assert.equal(umbrella.sessionRef, SESSION_REF);
  assert.equal(umbrella.founderResolution, "unresolved");
  assert.equal(codingSessionFounderLabel(umbrella), "founder unresolved");
  assert.equal(
    umbrella.executions[0].authoritySource,
    "disclosed-fallback",
    "and the execution discloses that its authority is unverified",
  );
});

test("a create that names no genesis resolves the founder as legacy", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf(
    execution(operator, provider, { sessionRef: SESSION_REF }),
  );
  const umbrella = snapshot.umbrellas[0];
  assert.equal(umbrella.founderResolution, "legacy");
  assert.equal(umbrella.founderPubkey, operator.pubkey);
  assert.match(codingSessionFounderLabel(umbrella), /\(legacy\)$/);
});

test("a genesisRef pointing at an unobserved event leaves the founder null", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf(
    execution(operator, provider, {
      sessionRef: SESSION_REF,
      genesisRef: "f".repeat(64),
    }),
  );
  assert.equal(snapshot.umbrellas[0].founderResolution, "governed");
  assert.equal(snapshot.umbrellas[0].founderPubkey, null);
});

test("Ended only when EVERY execution is stopped", () => {
  const founder = newSigner();
  const providerA = newSigner();
  const providerB = newSigner();
  const events = [
    ...execution(founder, providerA, {
      commandId: "cmd-a",
      sessionId: "session-a",
      sessionRef: SESSION_REF,
      status: "stopped",
      metadataAt: 1_700_000_500,
    }),
    ...execution(founder, providerB, {
      commandId: "cmd-b",
      sessionId: "session-b",
      sessionRef: SESSION_REF,
      status: "idle",
      metadataAt: 1_700_000_100,
    }),
  ];
  const snapshot = snapshotOf(events);
  assert.equal(snapshot.umbrellas[0].executions.length, 2);
  assert.notEqual(
    snapshot.umbrellas[0].status,
    "stopped",
    "the newest execution stopping must not end a session still holding a live one",
  );
  assert.equal(
    codingSessionStatusChipLabel(snapshot.umbrellas[0].status),
    "Idle",
  );
});

test("running outranks waiting, and waiting outranks quiet", () => {
  const gen = (status, latestEventMs) => ({
    activeGeneration: { status },
    latestEventMs,
  });
  assert.equal(
    foldUmbrellaStatus([gen("stopped", 5), gen("running", 1)]),
    "running",
  );
  assert.equal(
    foldUmbrellaStatus([gen("stopped", 5), gen("waiting_for_input", 1)]),
    "waiting_for_input",
  );
  assert.equal(
    foldUmbrellaStatus([gen("stopped", 5), gen("stopped", 1)]),
    "stopped",
  );
  assert.equal(foldUmbrellaStatus([]), "unknown");
});

test("a resume adds a generation to the same execution, not a second one", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf([
    ...execution(operator, provider, { commandId: "cmd-a" }),
    receiptEvent(provider, {
      commandId: "cmd-a",
      status: "resumed",
      target: target({ generation: 2 }),
      created_at: 1_700_000_400,
    }),
    metadataEvent(provider, {
      target: target({ generation: 2 }),
      status: "running",
      created_at: 1_700_000_400,
    }),
  ]);
  assert.equal(snapshot.umbrellas[0].executions.length, 1);
  const execution0 = snapshot.umbrellas[0].executions[0];
  assert.equal(execution0.activeGeneration.target.generation, 2);
  assert.equal(execution0.priorGenerations.length, 1);
  assert.equal(execution0.priorGenerations[0].target.generation, 1);
});

test("the newest 44229 name wins over the metadata title", () => {
  const founder = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf([
    ...execution(founder, provider, { sessionRef: SESSION_REF }),
    nameEvent(founder, { sessionRef: SESSION_REF, name: "Old", created_at: 1 }),
    nameEvent(founder, {
      sessionRef: SESSION_REF,
      name: "New",
      created_at: 2,
    }),
  ]);
  assert.equal(snapshot.umbrellas[0].name, "New");
});

test("a closure signed by the founder closes the session", () => {
  const founder = newSigner();
  const provider = newSigner();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesis,
    ...execution(founder, provider, {
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    closureEvent(founder, { sessionRef: SESSION_REF, genesisRef: genesis.id }),
  ]);
  assert.equal(snapshot.umbrellas[0].closed, true);
});

test("a closure signed by anyone else is ignored, not believed", () => {
  const founder = newSigner();
  const stranger = newSigner();
  const provider = newSigner();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesis,
    ...execution(founder, provider, {
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    closureEvent(stranger, { sessionRef: SESSION_REF, genesisRef: genesis.id }),
  ]);
  assert.equal(snapshot.umbrellas[0].closed, false);
});

test("an execution operated by someone other than the founder is flagged", () => {
  const founder = newSigner();
  const guest = newSigner();
  const providerA = newSigner();
  const providerB = newSigner();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesis,
    ...execution(founder, providerA, {
      commandId: "cmd-a",
      sessionId: "session-a",
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    ...execution(guest, providerB, {
      commandId: "cmd-b",
      sessionId: "session-b",
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
  ]);
  assert.equal(snapshot.umbrellas[0].foreignAttachmentCount, 1);
});

test("an unread lease query never renders as nobody answering", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf(execution(operator, provider), {
    leasesRead: false,
  });
  const generationId =
    snapshot.umbrellas[0].executions[0].activeGeneration.generationId;
  const report = snapshot.reachabilityByGenerationId.get(generationId);
  assert.equal(report.reachability, "unknown");
  assert.equal(
    codingSessionReachabilityLine(report, "Idle"),
    "Reachability not read yet",
  );
});

test("the snapshot states that signatures were verified on this device", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf(execution(operator, provider));
  assert.equal(snapshot.signaturesVerified, true);
});

test("an unanswered create names no founder and closes nothing", () => {
  const provider = newSigner();
  const stranger = newSigner();
  const snapshot = snapshotOf([
    // A live session whose own create fell outside the history window.
    receiptEvent(provider, { commandId: "unseen", status: "created" }),
    metadataEvent(provider, { sessionRef: SESSION_REF }),
    // Any channel member can sign a backdated create bearing this sessionRef.
    // No provider ever answered it, so it is a claim, not a fact.
    createEvent(stranger, {
      commandId: "stranger-command",
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
      created_at: 1_699_000_000,
    }),
    closureEvent(stranger, { sessionRef: SESSION_REF }),
  ]);
  const umbrella = snapshot.umbrellas[0];
  assert.equal(umbrella.sessionRef, SESSION_REF);
  assert.equal(
    umbrella.founderPubkey,
    null,
    "a create nobody's provider acted on cannot make its signer the founder",
  );
  assert.equal(umbrella.founderResolution, "unresolved");
  assert.equal(
    umbrella.closed,
    false,
    "and so cannot hand that member the authority to close the session",
  );
});

test("a genesis anchored to another session names no founder here", () => {
  const operator = newSigner();
  const stranger = newSigner();
  const provider = newSigner();
  const foreign = genesisEvent(stranger, { sessionRef: OTHER_SESSION_REF });
  const snapshot = snapshotOf([
    foreign,
    ...execution(operator, provider, {
      sessionRef: SESSION_REF,
      genesisRef: foreign.id,
    }),
    closureEvent(stranger, { sessionRef: SESSION_REF, genesisRef: foreign.id }),
  ]);
  const umbrella = snapshot.umbrellas[0];
  assert.equal(
    umbrella.founderPubkey,
    null,
    "the anchor a create names must belong to the session it anchors",
  );
  assert.equal(umbrella.closed, false);
});

test("the operator is read from the command the provider actually confirmed", () => {
  const founder = newSigner();
  const guest = newSigner();
  const provider = newSigner();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  const snapshot = snapshotOf([
    genesis,
    ...execution(founder, provider, {
      commandId: "cmd-a",
      sessionId: "session-a",
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    // The guest's create is earlier and names the same provider, but the
    // receipt that minted this execution answered `cmd-a`, not this one.
    createEvent(guest, {
      commandId: "cmd-guest",
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      created_at: 1_690_000_000,
    }),
    receiptEvent(provider, {
      commandId: "cmd-guest",
      status: "created",
      target: target({ sessionId: "session-guest" }),
    }),
    metadataEvent(provider, {
      target: target({ sessionId: "session-guest" }),
      sessionRef: SESSION_REF,
    }),
  ]);
  const umbrella = snapshot.umbrellas[0];
  const byOperator = Object.fromEntries(
    umbrella.executions.map((item) => [
      item.activeGeneration.target.sessionId,
      item.operatorPubkey,
    ]),
  );
  assert.equal(byOperator["session-a"], founder.pubkey);
  assert.equal(byOperator["session-guest"], guest.pubkey);
  assert.equal(umbrella.foreignAttachmentCount, 1);
});
