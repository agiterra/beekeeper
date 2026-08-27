import assert from "node:assert/strict";
import { test } from "node:test";

import { CodingSessionObserverStore } from "../domain/catalog.ts";
import {
  CHANNEL_ID,
  createEvent,
  metadataEvent,
  newSigner,
  receiptEvent,
  SESSION_REF,
  target,
  transcriptEvent,
} from "../domain/testFixtures.mjs";
import { buildCodingSessionObserverSnapshot } from "../domain/umbrella.ts";
import {
  buildCodingSessionTranscriptBlocks,
  codingSessionObserverViewFromSnapshot,
  codingSessionRouteRef,
  emptyCodingSessionObserverView,
  selectCodingSessionTranscriptBlocks,
  selectCodingSessionUmbrella,
  selectHeadlineExecution,
  selectReachability,
} from "./observer-contract.ts";

const channels = [CHANNEL_ID];
const NOW_MS = 1_700_000_000_000;

function status(overrides = {}) {
  return {
    channelId: CHANNEL_ID,
    connection: "live",
    lastError: null,
    historyLoaded: true,
    refresh: () => {},
    ...overrides,
  };
}

function viewOf(events, options = {}) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  const snapshot = buildCodingSessionObserverSnapshot(store.facts(channels), {
    nowMs: options.nowMs ?? NOW_MS,
    leasesRead: options.leasesRead ?? false,
    historyTruncated: options.historyTruncated ?? false,
  });
  return codingSessionObserverViewFromSnapshot(
    snapshot,
    status(options.status),
  );
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

test("a repo with no session channel renders nothing and claims nothing", () => {
  const view = emptyCodingSessionObserverView();
  assert.equal(view.channelId, null);
  assert.equal(view.sessions.length, 0);
  assert.equal(view.connection, "idle");
  assert.equal(view.lastError, null);
  // Zero facts rendered means zero unverified facts rendered.
  assert.equal(view.signaturesVerified, true);
  assert.equal(view.truncatedAt1000, false);
});

test("the view carries every fact a screen is allowed to state", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf(
    execution(operator, provider, { sessionRef: SESSION_REF }),
    { historyTruncated: true },
  );

  assert.equal(view.sessions.length, 1);
  assert.equal(view.channelId, CHANNEL_ID);
  assert.equal(view.signaturesVerified, true);
  assert.equal(view.truncatedAt1000, true);
  assert.deepEqual(view.counts, {
    malformed: 0,
    invalidSignature: 0,
    conflict: 0,
  });
  const [session] = view.sessions;
  assert.equal(session.sessionRef, SESSION_REF);
  assert.equal(view.executions.size, session.executions.length);
  for (const item of session.executions) {
    assert.ok(view.executions.has(item.executionKey));
    assert.ok(view.transcriptBlocksByExecution.has(item.executionKey));
  }
});

test("sessions are ordered by last activity, newest first", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf([
    ...execution(operator, provider, {
      commandId: "command-old",
      sessionId: "session-old",
      target: target({ sessionId: "session-old" }),
      metadataAt: 1_699_999_500,
    }),
    ...execution(operator, provider, {
      commandId: "command-new",
      sessionId: "session-new",
      target: target({ sessionId: "session-new" }),
      metadataAt: 1_700_000_500,
    }),
  ]);
  assert.equal(view.sessions.length, 2);
  assert.ok(view.sessions[0].lastEventAt >= view.sessions[1].lastEventAt);
});

test("a session is addressable by sessionRef and by umbrella key", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf(
    execution(operator, provider, { sessionRef: SESSION_REF }),
  );
  const [session] = view.sessions;
  assert.equal(codingSessionRouteRef(session), SESSION_REF);
  assert.equal(
    selectCodingSessionUmbrella(view.sessions, SESSION_REF),
    session,
  );
  assert.equal(
    selectCodingSessionUmbrella(view.sessions, session.umbrellaKey),
    session,
  );
  assert.equal(selectCodingSessionUmbrella(view.sessions, "nope"), null);
});

test("an umbrella of one is addressable by its opaque key", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf(execution(operator, provider));
  const [session] = view.sessions;
  assert.equal(session.sessionRef, null);
  assert.equal(codingSessionRouteRef(session), session.umbrellaKey);
  assert.equal(
    selectCodingSessionUmbrella(view.sessions, session.umbrellaKey),
    session,
  );
});

test("transcript rows are grouped into blocks, and blocks never merge", () => {
  const operator = newSigner();
  const providerA = newSigner();
  const providerB = newSigner();
  const targetA = target({ sessionId: "session-a" });
  const targetB = target({ driver: "provider-b", sessionId: "session-b" });
  const view = viewOf([
    ...execution(operator, providerA, {
      commandId: "command-a",
      target: targetA,
      sessionRef: SESSION_REF,
    }),
    ...execution(operator, providerB, {
      commandId: "command-b",
      target: targetB,
      sessionRef: SESSION_REF,
    }),
    transcriptEvent(providerA, {
      target: targetA,
      eventSeq: 1,
      timestamp: 1_700_000_001_000,
    }),
    transcriptEvent(providerB, {
      target: targetB,
      eventSeq: 1,
      timestamp: 1_700_000_002_000,
    }),
  ]);

  const [session] = view.sessions;
  const blocks = selectCodingSessionTranscriptBlocks(view, session);
  assert.equal(blocks.length, 2);
  const blockKeys = new Set(blocks.map((block) => block.blockKey));
  assert.equal(blockKeys.size, 2);
  for (const block of blocks) {
    const keys = new Set(block.items.map((item) => item.blockKey));
    assert.deepEqual([...keys], [block.blockKey]);
  }
  // Blocks are ordered by their earliest row, not by signer.
  assert.ok(blocks[0].startedAt <= blocks[1].startedAt);
});

test("one execution's rows collapse into a single labelled block", () => {
  const operator = newSigner();
  const provider = newSigner();
  const executionTarget = target({ sessionId: "session-1" });
  const view = viewOf([
    ...execution(operator, provider, { target: executionTarget }),
    transcriptEvent(provider, { target: executionTarget, eventSeq: 1 }),
    transcriptEvent(provider, { target: executionTarget, eventSeq: 2 }),
  ]);
  const [session] = view.sessions;
  const [item] = session.executions;
  const blocks = buildCodingSessionTranscriptBlocks(item);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].items.length, 2);
  assert.ok(blocks[0].label.length > 0);
  assert.deepEqual(
    blocks[0].items.map((row) => row.eventSeq),
    [1, 2],
  );
});

test("the headline execution is the most recently active one", () => {
  const operator = newSigner();
  const providerA = newSigner();
  const providerB = newSigner();
  const view = viewOf([
    ...execution(operator, providerA, {
      commandId: "command-a",
      target: target({ sessionId: "session-a" }),
      sessionRef: SESSION_REF,
      metadataAt: 1_699_999_500,
    }),
    ...execution(operator, providerB, {
      commandId: "command-b",
      target: target({ driver: "provider-b", sessionId: "session-b" }),
      sessionRef: SESSION_REF,
      metadataAt: 1_700_000_500,
    }),
  ]);
  const [session] = view.sessions;
  const headline = selectHeadlineExecution(session);
  assert.ok(headline !== null);
  for (const other of session.executions) {
    assert.ok(
      headline.activeGeneration.lastEventAt >=
        other.activeGeneration.lastEventAt,
    );
  }
});

test("reachability that was never read renders as unknown, never as silence", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf(execution(operator, provider));
  const [session] = view.sessions;
  const headline = selectHeadlineExecution(session);
  const report = selectReachability(
    view,
    headline.activeGeneration.generationId,
  );
  assert.equal(report.reachability, "unknown");
  assert.equal(
    selectReachability(view, "no-such-generation").reachability,
    "unknown",
  );
});

test("the reader's connection state and error survive into the view", () => {
  const operator = newSigner();
  const provider = newSigner();
  const view = viewOf(execution(operator, provider), {
    status: {
      connection: "reconnecting",
      lastError: "socket closed",
      historyLoaded: false,
    },
  });
  assert.equal(view.connection, "reconnecting");
  assert.equal(view.lastError, "socket closed");
  assert.equal(view.historyLoaded, false);
});
