/**
 * The snapshot shape the surfaces render.
 *
 * These go through the real store, so what is asserted here is what a browser
 * would actually have after reading those events off a relay.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { CodingSessionObserverStore } from "../domain/catalog.ts";
import {
  CHANNEL_ID,
  SESSION_REF,
  createEvent,
  metadataEvent,
  newSigner,
  receiptEvent,
  target,
  transcriptEvent,
} from "../domain/testFixtures.mjs";
import {
  buildChannelSessionSnapshot,
  createEmptyChannelSessionSnapshot,
  selectChannelSession,
  selectSessionTranscriptBlocks,
} from "./observerSnapshot.ts";

const NOW_MS = 1_700_000_010_000;

function snapshotOf(events, overrides = {}) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, [CHANNEL_ID]);
  return buildChannelSessionSnapshot(store.facts([CHANNEL_ID]), {
    nowMs: NOW_MS,
    leasesRead: true,
    historyRead: true,
    truncatedAt1000: false,
    connection: "open",
    lastError: null,
    ...overrides,
  });
}

test("the idle snapshot claims nothing it has not read", () => {
  const snapshot = createEmptyChannelSessionSnapshot();
  assert.deepEqual(snapshot.sessions, []);
  assert.equal(snapshot.connection, "idle");
  assert.equal(snapshot.historyRead, false);
  assert.equal(snapshot.leasesRead, false);
  assert.equal(snapshot.truncatedAt1000, false);
  assert.equal(snapshot.lastError, null);
});

test("one execution's rows become one block, keyed by execution", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf([
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, { status: "idle", sessionRef: SESSION_REF }),
    transcriptEvent(provider, {
      eventSeq: 1,
      item: { kind: "user_prompt", content: "do the thing" },
    }),
    transcriptEvent(provider, {
      eventSeq: 2,
      item: { kind: "assistant_text", text: "done" },
    }),
  ]);

  const [session] = snapshot.sessions;
  const [execution] = session.executions;
  const block = snapshot.transcriptBlocksByExecution.get(
    execution.executionKey,
  );
  assert.equal(block.blockKey, execution.executionKey);
  assert.equal(block.label, execution.label);
  assert.deepEqual(
    block.items.map((item) => item.text),
    ["do the thing", "done"],
  );
  assert.deepEqual(selectSessionTranscriptBlocks(snapshot, session), [block]);
  assert.deepEqual(snapshot.counts, {
    sessions: 1,
    executions: 1,
    malformed: 0,
    invalidSignature: 0,
    conflicts: 0,
  });
});

test("a resumed execution keeps its generations in order, not by eventSeq", () => {
  const operator = newSigner();
  const provider = newSigner();
  const second = target({ generation: 2 });
  const snapshot = snapshotOf([
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, { status: "idle", sessionRef: SESSION_REF }),
    transcriptEvent(provider, {
      eventSeq: 9,
      item: { kind: "assistant_text", text: "first life" },
    }),
    createEvent(operator, {
      commandId: "command-2",
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(provider, {
      commandId: "command-2",
      status: "created",
      target: second,
    }),
    metadataEvent(provider, {
      status: "running",
      sessionRef: SESSION_REF,
      target: second,
      created_at: 1_700_000_005,
    }),
    transcriptEvent(provider, {
      eventSeq: 1,
      target: second,
      item: { kind: "assistant_text", text: "second life" },
    }),
  ]);

  assert.equal(snapshot.sessions.length, 1);
  const [execution] = snapshot.sessions[0].executions;
  assert.equal(execution.priorGenerations.length, 1);
  assert.equal(execution.activeGeneration.target.generation, 2);
  const block = snapshot.transcriptBlocksByExecution.get(
    execution.executionKey,
  );
  assert.deepEqual(
    block.items.map((item) => item.text),
    ["first life", "second life"],
  );
});

test("two executions in one session interleave as blocks, never as rows", () => {
  const operator = newSigner();
  const first = newSigner();
  const secondProvider = newSigner();
  const secondTarget = target({ instanceId: "instance-2" });
  const snapshot = snapshotOf([
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: first.pubkey,
    }),
    receiptEvent(first, { status: "created" }),
    metadataEvent(first, { status: "idle", sessionRef: SESSION_REF }),
    transcriptEvent(first, {
      eventSeq: 1,
      timestamp: 1_700_000_000_000,
      item: { kind: "assistant_text", text: "from A" },
    }),
    createEvent(operator, {
      commandId: "command-2",
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: secondProvider.pubkey,
    }),
    receiptEvent(secondProvider, {
      commandId: "command-2",
      status: "created",
      target: secondTarget,
    }),
    metadataEvent(secondProvider, {
      status: "running",
      sessionRef: SESSION_REF,
      target: secondTarget,
    }),
    transcriptEvent(secondProvider, {
      eventSeq: 1,
      target: secondTarget,
      timestamp: 1_700_000_005_000,
      item: { kind: "assistant_text", text: "from B" },
    }),
  ]);

  assert.equal(snapshot.sessions.length, 1, "one sessionRef is one session");
  const blocks = selectSessionTranscriptBlocks(snapshot, snapshot.sessions[0]);
  assert.equal(blocks.length, 2);
  assert.deepEqual(
    blocks.map((block) => block.items.map((item) => item.text)),
    [["from A"], ["from B"]],
  );
  assert.ok(blocks[0].startedAt <= blocks[1].startedAt);
  assert.equal(snapshot.counts.executions, 2);
});

test("a session is addressable by sessionRef and by umbrella key", () => {
  const operator = newSigner();
  const provider = newSigner();
  const snapshot = snapshotOf([
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
    }),
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, { status: "idle", sessionRef: SESSION_REF }),
  ]);
  const [session] = snapshot.sessions;

  assert.equal(selectChannelSession(snapshot, SESSION_REF), session);
  assert.equal(selectChannelSession(snapshot, session.umbrellaKey), session);
  assert.equal(selectChannelSession(snapshot, "no-such-session"), null);
});

test("a truncated read is carried into the snapshot, not smoothed over", () => {
  const snapshot = snapshotOf([], {
    truncatedAt1000: true,
    connection: "error",
    lastError: "relay closed the connection",
  });
  assert.equal(snapshot.truncatedAt1000, true);
  assert.equal(snapshot.connection, "error");
  assert.equal(snapshot.lastError, "relay closed the connection");
  assert.equal(snapshot.signaturesVerified, true);
});
