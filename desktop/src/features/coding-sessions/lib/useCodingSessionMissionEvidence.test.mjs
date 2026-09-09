import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import {
  buildCodingSessionMissionEvidenceFilters,
  CodingSessionMissionEvidenceStore,
  CODING_SESSION_MISSION_EVIDENCE_KINDS,
  MISSION_EVIDENCE_HISTORY_LIMIT,
  MISSION_EVIDENCE_MAX_EVENTS_PER_KIND,
  MISSION_EVIDENCE_REJECTION_LIMIT,
  projectCodingSessionMissionEvidence,
} from "./codingSessionMissionEvidenceModel.ts";
import {
  CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
} from "./codingSessionTeamTransactionWire.ts";
import { useCodingSessionMissionEvidence } from "./useCodingSessionMissionEvidence.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const ipcHandlers = new Map();
const tauriInternals = {
  invoke(command, args) {
    const handler = ipcHandlers.get(command);
    if (!handler) return Promise.reject(new Error(`unmocked: ${command}`));
    return handler(args);
  },
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const SESSION_A = "683d55b3-d34e-410d-874a-55f9082d2631";
const SESSION_B = "783d55b3-d34e-410d-874a-55f9082d2632";
const GENESIS_A = "ab".repeat(32);
const GENESIS_B = "cd".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const RELAY_SECRET = new Uint8Array(32).fill(2);
const SEAT_SECRET = new Uint8Array(32).fill(3);
const OTHER_SECRET = new Uint8Array(32).fill(4);
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY = getPublicKey(RELAY_SECRET);
const SEAT = getPublicKey(SEAT_SECRET);

function scope(sessionRef = SESSION_A, genesisRef = GENESIS_A) {
  return {
    channelRef: CHANNEL,
    sessionRef,
    genesisRef,
    founderPubkey: FOUNDER,
  };
}

function sign({ kind, tags, content, secret = FOUNDER_SECRET, createdAt = 1 }) {
  return finalizeEvent({ kind, tags, content, created_at: createdAt }, secret);
}

function transaction({
  type,
  body,
  sessionRef = SESSION_A,
  genesisRef = GENESIS_A,
  secret = FOUNDER_SECRET,
  createdAt = 1,
}) {
  return sign({
    kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
    tags: [
      ["h", CHANNEL],
      ["d", sessionRef],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", genesisRef],
      ["cstx-type", type],
    ],
    content: JSON.stringify({
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef,
      genesisRef,
      type,
      supersedes: null,
      deliveryCommandId: null,
      body,
    }),
    secret,
    createdAt,
  });
}

function assignment(options = {}) {
  return transaction({
    ...options,
    type: "assignment",
    body: {
      assigneeActor: SEAT,
      assigneeRole: "builder",
      objective: "Prove Mission evidence",
      brief: "Use only signed facts.",
      branch: null,
      baseSha: null,
      fileOwnership: [],
      acceptanceSteps: ["run focused tests"],
    },
  });
}

function report(assignmentRef, summary, options = {}) {
  return transaction({
    ...options,
    type: "report",
    body: {
      assignmentRef,
      summary,
      branch: null,
      baseSha: null,
      headSha: null,
      files: [],
      tests: [],
      redBeforeGreen: null,
      deviations: [],
      residuals: [],
      anomalies: [],
    },
  });
}

function blocked(assignmentRef, options = {}) {
  return transaction({
    ...options,
    type: "mission.blocked",
    body: {
      assignmentRefs: [assignmentRef],
      summary: "Waiting for signing",
      blockers: ["Keychain locked"],
      heldOn: "founder",
      requiredAction: "Unlock signing keys",
    },
  });
}

function transition(payload, secret = FOUNDER_SECRET) {
  return sign({
    kind: 44228,
    tags: [
      ["h", CHANNEL],
      ["csat-v", "csat1-1"],
      ["csat-genesis", GENESIS_A],
    ],
    content: JSON.stringify({ genesisRef: GENESIS_A, ...payload }),
    secret,
  });
}

function receipt(event, payload, secret = RELAY_SECRET, extra = {}) {
  return sign({
    kind: 40099,
    tags: [["h", CHANNEL]],
    content: JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: GENESIS_A,
      acceptedEventId: event.id,
      seq: payload.seq,
      transitionType: payload.type,
      granteePubkey: payload.granteePubkey,
      ...(payload.role ? { role: payload.role } : {}),
      ...extra,
    }),
    secret,
  });
}

function nativeResponse(request) {
  const payloads = request.events.map((event) => ({
    id: event.id,
    payload: JSON.parse(event.content),
  }));
  const terminal = payloads
    .filter(({ payload }) => payload.type.startsWith("mission."))
    .at(-1);
  return {
    schema: "buzz-coding-session-team-fold-adapter/v1",
    implementation: "buzz-core",
    inputEventIds: [...request.inputEventIds],
    context: {
      channelRef: request.context.channelRef,
      sessionRef: request.context.sessionRef,
      genesisRef: request.context.genesisRef,
      founderPubkey: request.context.founderPubkey,
      authorityHeadEventId: request.context.authorityHeadEventId,
      authorityHeadSeq: request.context.authorityHeadSeq,
      // L8.3: the adapter echoes the `verifierRequired` it was asked
      // with, and the decoder requires the key. Echo it rather than
      // restating a default, so this fixture cannot drift from the
      // boundary it stands in for.
      verifierRequired: request.context.verifierRequired,
    },
    includedEventIds: [...request.inputEventIds],
    excluded: [],
    conflicts: [],
    assignments: [],
    unseatedReports: [],
    notes: [],
    decisions: [],
    waitingOnDecision: null,
    canonicalTerminal: terminal
      ? { eventId: terminal.id, type: terminal.payload.type }
      : null,
  };
}

/**
 * A fake client at the transport's bundled surface: every history read is one
 * `fetchEventsBatch` (recorded filter by filter in `fetches`) and the live
 * fence is one `subscribeLiveMany` (recorded filter by filter in
 * `subscriptions`, all sharing the one REQ's callback and close).
 */
function clientFor(history) {
  const fetches = [];
  const subscriptions = [];
  const batches = [];
  return {
    fetches,
    subscriptions,
    batches,
    fetchEventsBatch: async (filters) => {
      batches.push(filters);
      fetches.push(...filters);
      return history.filter((event) =>
        filters.some((filter) => filter.kinds.includes(event.kind)),
      );
    },
    subscribeLiveMany: async (filters, onEvent) => {
      const shared = { closed: false };
      for (const filter of filters) {
        subscriptions.push({
          filter,
          onEvent,
          get closed() {
            return shared.closed;
          },
        });
      }
      return () => {
        shared.closed = true;
      };
    },
  };
}

async function harness() {
  const { act, renderHook } = await import("@testing-library/react");
  const settleUntil = async (predicate, label) => {
    for (let round = 0; round < 300; round += 1) {
      if (predicate()) return;
      await act(async () => new Promise((resolve) => setTimeout(resolve, 1)));
    }
    throw new Error(`timed out waiting for ${label}`);
  };
  return { act, renderHook, settleUntil };
}

function installNative({ relayPubkey = RELAY, fold = nativeResponse } = {}) {
  ipcHandlers.set("get_relay_self", async () => relayPubkey);
  ipcHandlers.set(
    "fold_coding_session_team_transactions",
    async ({ request }) => fold(request),
  );
}

test("filters are explicit and history plus live evidence stays exactly scoped", async () => {
  installNative();
  const firstAssignment = assignment();
  const crossGenesis = assignment({ genesisRef: GENESIS_B, createdAt: 2 });
  const malformed = sign({
    kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
    tags: [
      ["h", CHANNEL],
      ["d", SESSION_A],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", GENESIS_A],
      ["cstx-type", "assignment"],
    ],
    content: "{}",
    createdAt: 3,
  });
  const forged = {
    ...assignment({ createdAt: 4 }),
    sig: "00".repeat(64),
  };
  const client = clientFor([firstAssignment, crossGenesis, malformed, forged]);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading && client.fetches.length === 6,
    "history fold",
  );
  assert.deepEqual(
    client.fetches.map((filter) => filter.kinds[0]),
    [
      ...CODING_SESSION_MISSION_EVIDENCE_KINDS,
      ...CODING_SESSION_MISSION_EVIDENCE_KINDS,
    ],
  );
  assert.deepEqual(
    client.fetches.slice(0, 3),
    buildCodingSessionMissionEvidenceFilters(
      scope(),
      MISSION_EVIDENCE_HISTORY_LIMIT,
      RELAY,
    ),
  );
  assert.deepEqual(client.fetches[2].authors, [RELAY]);
  assert.equal(
    client.batches.length,
    2,
    "cold and post-fence reads each bundle three filters",
  );
  assert.deepEqual(client.batches[0], client.batches[1]);
  assert.equal(client.subscriptions.length, 3);
  assert.equal(mounted.result.current.retainedEventCount, 2);
  assert.equal(mounted.result.current.inspectorInput.rejectedEventCount, 2);
  assert.deepEqual(
    mounted.result.current.inspectorInput.rejectedReasons.map(
      ({ eventIds }) => eventIds[0],
    ),
    [malformed.id, forged.id],
  );
  assert.equal(
    mounted.result.current.inspectorInput.missionState.kind,
    "unknown",
  );

  const liveBlocked = blocked(firstAssignment.id, { createdAt: 5 });
  await act(async () => {
    client.subscriptions[0].onEvent(liveBlocked);
  });
  await settleUntil(
    () => mounted.result.current.inspectorInput.missionState.kind === "blocked",
    "live canonical terminal",
  );
  assert.equal(mounted.result.current.retainedEventCount, 3);
  assert.equal(mounted.result.current.inspectorInput.rejectedEventCount, 2);
  mounted.unmount();
  assert.ok(client.subscriptions.every(({ closed }) => closed));
});

test("live is buffered until every history read settles and supplies its causal parent", async () => {
  const historicalAssignment = assignment();
  const earlyLiveReport = report(
    historicalAssignment.id,
    "Live report after historical assignment",
    { createdAt: 2 },
  );
  let foldCalls = 0;
  installNative({
    fold: (request) => {
      foldCalls += 1;
      return nativeResponse(request);
    },
  });
  let resolveTransactions;
  const client = clientFor([]);
  client.fetchEventsBatch = (filters) =>
    filters.some((f) => f.kinds[0] === KIND_CODING_SESSION_TEAM_TRANSACTION)
      ? new Promise((resolve) => {
          resolveTransactions = resolve;
        })
      : Promise.resolve([]);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => client.subscriptions.length === 3 && Boolean(resolveTransactions),
    "live fence and pending history",
  );
  await act(async () => client.subscriptions[0].onEvent(earlyLiveReport));
  assert.equal(mounted.result.current.isLoading, true);
  assert.equal(mounted.result.current.errorMessage, null);
  assert.equal(foldCalls, 0);

  await act(async () => resolveTransactions([historicalAssignment]));
  await settleUntil(
    () =>
      mounted.result.current.inspectorInput.reports[0]?.summary ===
      "Live report after historical assignment",
    "one post-history canonical fold",
  );
  assert.equal(foldCalls, 1);
  assert.equal(mounted.result.current.retainedEventCount, 2);
  mounted.unmount();
});

test("history failure remains explicit and never folds buffered live evidence", async () => {
  let foldCalls = 0;
  installNative({
    fold: (request) => {
      foldCalls += 1;
      return nativeResponse(request);
    },
  });
  let rejectTransactions;
  const client = clientFor([]);
  client.fetchEventsBatch = (filters) =>
    filters.some((f) => f.kinds[0] === KIND_CODING_SESSION_TEAM_TRANSACTION)
      ? new Promise((_, reject) => {
          rejectTransactions = reject;
        })
      : Promise.resolve([]);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => client.subscriptions.length === 3 && Boolean(rejectTransactions),
    "live fence and failing history",
  );
  await act(async () =>
    client.subscriptions[0].onEvent(assignment({ createdAt: 2 })),
  );
  assert.equal(foldCalls, 0);
  assert.equal(mounted.result.current.isLoading, true);
  await act(async () => rejectTransactions(new Error("history unavailable")));
  await settleUntil(
    () => mounted.result.current.errorMessage === "history unavailable",
    "explicit history error",
  );
  assert.equal(foldCalls, 0);
  await act(async () =>
    client.subscriptions[0].onEvent(assignment({ createdAt: 3 })),
  );
  assert.equal(foldCalls, 0);
  assert.equal(mounted.result.current.errorMessage, "history unavailable");
  mounted.unmount();
});

test("raw overflow and rejection retention are deterministic and disclosed", async () => {
  installNative();
  const signedEvents = Array.from(
    { length: MISSION_EVIDENCE_MAX_EVENTS_PER_KIND + 1 },
    (_, index) => assignment({ createdAt: index + 1 }),
  );
  const forward = new CodingSessionMissionEvidenceStore(scope());
  const reverse = new CodingSessionMissionEvidenceStore(scope());
  forward.ingest(signedEvents);
  reverse.ingest([...signedEvents].reverse());
  const forwardSnapshot = forward.snapshot();
  const reverseSnapshot = reverse.snapshot();
  assert.equal(forwardSnapshot.overflowed, true);
  assert.deepEqual(
    forwardSnapshot.transactions.map(({ id }) => id),
    reverseSnapshot.transactions.map(({ id }) => id),
  );
  assert.equal(
    forwardSnapshot.transactions.length,
    MISSION_EVIDENCE_MAX_EVENTS_PER_KIND,
  );
  const overflowErrors = [];
  for (const snapshot of [forwardSnapshot, reverseSnapshot]) {
    await assert.rejects(
      projectCodingSessionMissionEvidence({
        scope: scope(),
        relayPubkey: RELAY,
        snapshot,
      }),
      (error) => {
        overflowErrors.push(error.message);
        return /exceeded the 1000-event per-kind bound/.test(error.message);
      },
    );
  }
  assert.equal(overflowErrors[0], overflowErrors[1]);

  const invalidEvents = Array.from({ length: 1500 }, (_, index) => ({
    ...assignment({ createdAt: index + 1 }),
    id: index.toString(16).padStart(64, "0"),
    sig: "00".repeat(64),
  }));
  const invalidForward = new CodingSessionMissionEvidenceStore(scope());
  const invalidReverse = new CodingSessionMissionEvidenceStore(scope());
  const exactStore = new CodingSessionMissionEvidenceStore(scope());
  assert.equal(
    exactStore.ingest(invalidEvents.slice(0, MISSION_EVIDENCE_REJECTION_LIMIT)),
    true,
  );
  assert.equal(
    exactStore.snapshot().rejectedTotal,
    MISSION_EVIDENCE_REJECTION_LIMIT,
  );
  assert.equal(exactStore.snapshot().rejectedOmitted, 0);
  assert.equal(exactStore.snapshot().rejectionsTruncated, false);
  invalidForward.ingest(invalidEvents);
  invalidReverse.ingest([...invalidEvents].reverse());
  const invalidForwardSnapshot = invalidForward.snapshot();
  const invalidReverseSnapshot = invalidReverse.snapshot();
  assert.equal(invalidForwardSnapshot.rejectedTotal, null);
  assert.equal(invalidForwardSnapshot.rejectedOmitted, null);
  assert.equal(invalidForwardSnapshot.rejectionsTruncated, true);
  assert.equal(invalidReverseSnapshot.rejectedTotal, null);
  assert.equal(invalidReverseSnapshot.rejectedOmitted, null);
  assert.equal(invalidReverseSnapshot.rejectionsTruncated, true);
  assert.deepEqual(
    invalidForwardSnapshot.rejected.map(({ eventId }) => eventId),
    invalidReverseSnapshot.rejected.map(({ eventId }) => eventId),
  );
  const omittedSnapshot = invalidForward.snapshot();
  assert.equal(invalidForward.ingest([invalidEvents.at(-1)]), false);
  assert.deepEqual(invalidForward.snapshot(), omittedSnapshot);
  const laterOmitted = {
    ...assignment({ createdAt: 2000 }),
    id: "f0".repeat(32),
    sig: "00".repeat(64),
  };
  assert.equal(invalidForward.ingest([laterOmitted]), false);
  assert.deepEqual(invalidForward.snapshot(), omittedSnapshot);
  const olderDisplacement = {
    ...assignment({ createdAt: 0 }),
    id: "ff".repeat(32),
    sig: "00".repeat(64),
  };
  assert.equal(invalidForward.ingest([olderDisplacement]), true);
  assert.equal(
    invalidForward.snapshot().rejected[0].eventId,
    olderDisplacement.id,
  );
  assert.equal(invalidForward.snapshot().rejectedTotal, null);

  const { inspectorInput: projectedForward } =
    await projectCodingSessionMissionEvidence({
      scope: scope(),
      relayPubkey: RELAY,
      snapshot: invalidForward.snapshot(),
    });
  const { inspectorInput: projectedReverse } =
    await projectCodingSessionMissionEvidence({
      scope: scope(),
      relayPubkey: RELAY,
      snapshot: invalidReverseSnapshot,
    });
  assert.equal(projectedForward.rejectedEventCount, null);
  assert.equal(projectedForward.rejectionsTruncated, true);
  assert.equal(
    projectedForward.rejectedReasons.length,
    MISSION_EVIDENCE_REJECTION_LIMIT,
  );
  assert.equal(projectedReverse.rejectedEventCount, null);
  assert.equal(projectedReverse.rejectionsTruncated, true);
  assert.equal(
    projectedReverse.rejectedReasons.length,
    MISSION_EVIDENCE_REJECTION_LIMIT,
  );
  assert.deepEqual(
    invalidForwardSnapshot.rejected.map(({ eventId }) => eventId),
    projectedReverse.rejectedReasons.map(({ eventIds }) => eventIds[0]),
  );
});

test("omitted live rejections do not refold unless retained detail changes", async () => {
  let foldCalls = 0;
  installNative({
    fold: (request) => {
      foldCalls += 1;
      return nativeResponse(request);
    },
  });
  const invalidHistory = Array.from(
    { length: MISSION_EVIDENCE_REJECTION_LIMIT + 1 },
    (_, index) => ({
      ...assignment({ createdAt: index + 1 }),
      sig: "00".repeat(64),
    }),
  );
  const client = clientFor(invalidHistory);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading && foldCalls === 1,
    "initial bounded rejection fold",
  );
  const before = mounted.result.current.inspectorInput;
  const laterOmitted = {
    ...assignment({ createdAt: 2000 }),
    sig: "00".repeat(64),
  };
  await act(async () => client.subscriptions[0].onEvent(laterOmitted));
  await act(async () => client.subscriptions[0].onEvent(laterOmitted));
  assert.equal(foldCalls, 1);
  assert.equal(mounted.result.current.inspectorInput, before);

  const olderDisplacement = {
    ...assignment({ createdAt: 0 }),
    sig: "00".repeat(64),
  };
  await act(async () => client.subscriptions[0].onEvent(olderDisplacement));
  await settleUntil(() => foldCalls === 2, "changed rejection detail fold");
  assert.equal(mounted.result.current.inspectorInput.rejectedEventCount, null);
  assert.equal(mounted.result.current.inspectorInput.rejectionsTruncated, true);
  mounted.unmount();
});

test("relay-key mismatch and malformed accepted receipt fail closed", async () => {
  const grantPayload = {
    type: "grant-seat",
    granteePubkey: SEAT,
    role: "builder",
    seq: 1,
    prevAccepted: null,
  };
  const grant = transition(grantPayload);
  for (const [label, badReceipt, expected] of [
    [
      "wrong relay",
      receipt(grant, grantPayload, OTHER_SECRET),
      /trusted relay/,
    ],
    [
      "extra receipt field",
      receipt(grant, grantPayload, RELAY_SECRET, { unexpected: true }),
      /strict CSAT receipt shape/,
    ],
  ]) {
    installNative();
    const client = clientFor([grant, badReceipt]);
    const { renderHook, settleUntil } = await harness();
    const mounted = renderHook(() =>
      useCodingSessionMissionEvidence(scope(), client),
    );
    await settleUntil(
      () => mounted.result.current.errorMessage !== null,
      label,
    );
    assert.match(mounted.result.current.errorMessage, expected);
    assert.equal(
      mounted.result.current.inspectorInput.missionState.kind,
      "unknown",
    );
    mounted.unmount();
  }
});

test("revoked seats reach native as inactive and refresh recovers a fold error", async () => {
  const grantPayload = {
    type: "grant-seat",
    granteePubkey: SEAT,
    role: "builder",
    seq: 1,
    prevAccepted: null,
  };
  const grant = transition(grantPayload);
  const revokePayload = {
    type: "revoke-seat",
    granteePubkey: SEAT,
    role: "builder",
    seq: 2,
    prevAccepted: grant.id,
  };
  const revoke = transition(revokePayload);
  let failFold = true;
  let nativeContext;
  installNative({
    fold: (request) => {
      nativeContext = request.context;
      if (failFold) throw new Error("x".repeat(5000));
      return nativeResponse(request);
    },
  });
  const client = clientFor([
    grant,
    receipt(grant, grantPayload),
    revoke,
    receipt(revoke, revokePayload),
  ]);
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => mounted.result.current.errorMessage?.endsWith("… [truncated]"),
    "native error",
  );
  assert.ok(mounted.result.current.errorMessage.length < 4200);
  assert.deepEqual(nativeContext.activeSeats, []);
  assert.equal(nativeContext.authorityHeadEventId, revoke.id);

  failFold = false;
  await act(async () => mounted.result.current.refresh());
  await settleUntil(
    () =>
      mounted.result.current.errorMessage === null &&
      !mounted.result.current.isLoading,
    "successful refresh",
  );
  assert.equal(
    client.fetches.length,
    9,
    "failed cold fold plus refreshed cold and catch-up reads",
  );
  mounted.unmount();
});

test("a late native result from an old scope cannot overwrite the new scope", async () => {
  const assignmentA = assignment();
  const reportA = report(assignmentA.id, "Report A");
  const assignmentB = assignment({
    sessionRef: SESSION_B,
    genesisRef: GENESIS_B,
  });
  const reportB = report(assignmentB.id, "Report B", {
    sessionRef: SESSION_B,
    genesisRef: GENESIS_B,
  });
  let resolveFirst;
  installNative({
    fold: (request) => {
      if (request.context.sessionRef === SESSION_A) {
        return new Promise((resolve) => {
          resolveFirst = () => resolve(nativeResponse(request));
        });
      }
      return nativeResponse(request);
    },
  });
  const client = {
    ...clientFor([]),
    fetchEventsBatch: async (filters) =>
      filters.some((filter) => filter["#d"]?.includes(SESSION_A))
        ? [assignmentA, reportA]
        : filters.some((filter) => filter["#d"]?.includes(SESSION_B))
          ? [assignmentB, reportB]
          : [],
  };
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(
    ({ value }) => useCodingSessionMissionEvidence(value, client),
    { initialProps: { value: scope() } },
  );
  await settleUntil(() => Boolean(resolveFirst), "first native fold");
  mounted.rerender({ value: scope(SESSION_B, GENESIS_B) });
  await settleUntil(
    () =>
      mounted.result.current.inspectorInput.reports[0]?.summary === "Report B",
    "second scope result",
  );
  await act(async () => resolveFirst());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 1)));
  assert.equal(
    mounted.result.current.inspectorInput.reports[0].summary,
    "Report B",
  );
  mounted.unmount();
});

test("empty history remains honest unknown rather than inferred running", async () => {
  installNative();
  const client = clientFor([]);
  const { renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(() => !mounted.result.current.isLoading, "empty fold");
  assert.equal(mounted.result.current.errorMessage, null);
  assert.equal(mounted.result.current.retainedEventCount, 0);
  assert.deepEqual(mounted.result.current.inspectorInput.missionState, {
    kind: "unknown",
    detail:
      "The native canonical fold has no terminal or active assignment; Mission does not infer running or completion from silence.",
  });
  mounted.unmount();
});

test("D6/D-T8: the accepted seat chain and the fold's unseated reports reach the result", async () => {
  const seatPayload = {
    type: "grant-seat",
    granteePubkey: SEAT,
    role: "builder",
    seq: 1,
    prevAccepted: null,
  };
  const seatGrant = transition(seatPayload);
  const seatReceipt = receipt(seatGrant, seatPayload);
  const historicalAssignment = assignment();
  const seatReport = report(historicalAssignment.id, "Built it", {
    secret: SEAT_SECRET,
    createdAt: 2,
  });
  installNative({
    fold: (request) => ({
      ...nativeResponse(request),
      unseatedReports: [
        {
          eventId: seatReport.id,
          authorPubkey: SEAT,
          assignmentRef: historicalAssignment.id,
          assigneeRole: "builder",
        },
      ],
    }),
  });
  const client = clientFor([
    historicalAssignment,
    seatReport,
    seatGrant,
    seatReceipt,
  ]);
  const { renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading,
    "seat authority projection",
  );
  assert.equal(mounted.result.current.errorMessage, null);
  assert.deepEqual(mounted.result.current.authority.activeSeats, [
    { actorPubkey: SEAT, role: "builder" },
  ]);
  assert.deepEqual(
    Object.values(mounted.result.current.authority.seatGrantRefs),
    [seatGrant.id],
  );
  assert.deepEqual(mounted.result.current.unseatedReportEventIds, [
    seatReport.id,
  ]);
  mounted.unmount();
});

test("cold Mission evidence projects before admission and unions gap history with live events", async () => {
  installNative();
  const parent = assignment();
  const gapReport = report(parent.id, "Caught admission gap", { createdAt: 2 });
  const liveTerminal = blocked(parent.id, { createdAt: 3 });
  let releaseLive;
  let onEvent;
  let reads = 0;
  const client = {
    fetchEventsBatch: async () => (++reads === 1 ? [parent] : [gapReport]),
    subscribeLiveMany: async (_, receive) => {
      onEvent = receive;
      return new Promise((resolve) => {
        releaseLive = () => resolve(() => {});
      });
    },
  };
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(
    () => !mounted.result.current.isLoading,
    "cold history without live admission",
  );
  assert.equal(reads, 1);
  assert.equal(mounted.result.current.retainedEventCount, 1);
  await act(async () => {
    onEvent(liveTerminal);
    releaseLive();
  });
  await settleUntil(
    () => mounted.result.current.retainedEventCount === 3,
    "union of cold, gap, and live evidence",
  );
  assert.equal(reads, 2);
  assert.equal(
    mounted.result.current.inspectorInput.reports[0].summary,
    "Caught admission gap",
  );
  assert.equal(
    mounted.result.current.inspectorInput.missionState.kind,
    "blocked",
  );
  mounted.unmount();
});

test("post-fence catch-up failure stays explicit after later live events", async () => {
  installNative();
  const parent = assignment();
  let releaseLive;
  let onEvent;
  let reads = 0;
  const client = {
    fetchEventsBatch: async () => {
      if (++reads === 1) return [parent];
      throw new Error("catch-up unavailable");
    },
    subscribeLiveMany: async (_, receive) => {
      onEvent = receive;
      return new Promise((resolve) => {
        releaseLive = () => resolve(() => {});
      });
    },
  };
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(() => !mounted.result.current.isLoading, "cold history");
  await act(async () => releaseLive());
  await settleUntil(
    () => mounted.result.current.errorMessage === "catch-up unavailable",
    "catch-up failure",
  );
  await act(async () => onEvent(blocked(parent.id, { createdAt: 2 })));
  assert.equal(mounted.result.current.errorMessage, "catch-up unavailable");
  assert.equal(mounted.result.current.authority, null);
  mounted.unmount();
});

test("late cold projection cannot erase a live admission error", async () => {
  let finishFold;
  installNative({
    fold: (request) =>
      new Promise((resolve) => {
        finishFold = () => resolve(nativeResponse(request));
      }),
  });
  let rejectLive;
  const client = {
    fetchEventsBatch: async () => [assignment()],
    subscribeLiveMany: async () =>
      new Promise((_, reject) => {
        rejectLive = reject;
      }),
  };
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(() => Boolean(finishFold), "pending cold projection");
  await act(async () => rejectLive(new Error("admission denied")));
  assert.equal(mounted.result.current.errorMessage, "admission denied");
  await act(async () => finishFold());
  assert.equal(mounted.result.current.errorMessage, "admission denied");
  assert.equal(mounted.result.current.authority, null);
  mounted.unmount();
});

test("unmount closes a late live fence without starting catch-up", async () => {
  installNative();
  let releaseLive;
  let closed = false;
  let reads = 0;
  const client = {
    fetchEventsBatch: async () => {
      reads += 1;
      return [];
    },
    subscribeLiveMany: async () =>
      new Promise((resolve) => {
        releaseLive = () =>
          resolve(() => {
            closed = true;
          });
      }),
  };
  const { act, renderHook, settleUntil } = await harness();
  const mounted = renderHook(() =>
    useCodingSessionMissionEvidence(scope(), client),
  );
  await settleUntil(() => !mounted.result.current.isLoading, "cold projection");
  mounted.unmount();
  await act(async () => releaseLive());
  assert.equal(closed, true);
  assert.equal(reads, 1);
});
