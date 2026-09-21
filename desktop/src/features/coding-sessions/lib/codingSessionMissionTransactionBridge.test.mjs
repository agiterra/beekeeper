import assert from "node:assert/strict";
import { test } from "node:test";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { projectCodingSessionMissionAuthority } from "./codingSessionMissionAuthority.ts";
import { projectNativeTeamFoldToMissionInspector } from "./codingSessionMissionTransactionProjection.ts";
import {
  CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
  decodeVerifiedCodingSessionTeamTransaction,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
} from "./codingSessionTeamTransactionWire.ts";
import {
  CODING_SESSION_TEAM_FOLD_COMMAND,
  CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA,
  CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
  invokeCodingSessionTeamFold,
} from "./invokeCodingSessionTeamFold.ts";

const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const SESSION = "683d55b3-d34e-410d-874a-55f9082d2631";
const GENESIS = "ab".repeat(32);
const OTHER_GENESIS = "ef".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const LEAD_SECRET = new Uint8Array(32).fill(2);
const VERIFIER_SECRET = new Uint8Array(32).fill(3);
const RELAY_SECRET = new Uint8Array(32).fill(4);
const STRANGER_SECRET = new Uint8Array(32).fill(5);
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const LEAD = getPublicKey(LEAD_SECRET);
const VERIFIER = getPublicKey(VERIFIER_SECRET);
const RELAY = getPublicKey(RELAY_SECRET);
const STRANGER = getPublicKey(STRANGER_SECRET);

function sign({ kind, content, tags, secret = FOUNDER_SECRET, createdAt = 1 }) {
  return finalizeEvent({ kind, content, tags, created_at: createdAt }, secret);
}

function transaction(payload, secret = FOUNDER_SECRET, createdAt = 1) {
  return sign({
    kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
    content: JSON.stringify(payload),
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", GENESIS],
      ["cstx-type", payload.type],
    ],
    secret,
    createdAt,
  });
}

function transition({
  type,
  granteePubkey,
  role,
  seq,
  prevAccepted,
  secret = FOUNDER_SECRET,
  genesisRef = GENESIS,
}) {
  return sign({
    kind: 44228,
    content: JSON.stringify({
      genesisRef,
      prevAccepted,
      seq,
      type,
      granteePubkey,
      ...(role ? { role } : {}),
    }),
    tags: [
      ["h", CHANNEL],
      ["csat-v", "csat1-1"],
      ["csat-genesis", genesisRef],
    ],
    secret,
  });
}

function receipt(
  accepted,
  transitionPayload,
  { secret = RELAY_SECRET, genesisRef = GENESIS } = {},
) {
  return sign({
    kind: 40099,
    content: JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef,
      acceptedEventId: accepted.id,
      seq: transitionPayload.seq,
      transitionType: transitionPayload.type,
      granteePubkey: transitionPayload.granteePubkey,
      ...(transitionPayload.role ? { role: transitionPayload.role } : {}),
    }),
    tags: [["h", CHANNEL]],
    secret,
  });
}

function authorityInput(transitions, receipts) {
  return {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    transitions,
    receipts,
  };
}

async function invokeWithTauriFoldMock(input, handler) {
  const previousWindow = globalThis.window;
  globalThis.window = {
    ...(previousWindow ?? {}),
    __TAURI_INTERNALS__: { invoke: handler },
  };
  try {
    return await invokeCodingSessionTeamFold(input);
  } finally {
    globalThis.window = previousWindow;
  }
}

async function projectAcceptedNonterminal(events, assignments) {
  const verifiedTransactions = events.map((event) => {
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    });
    assert.equal(decoded.ok, true);
    if (!decoded.ok) throw new Error(decoded.error);
    return decoded.value;
  });
  const inputEventIds = verifiedTransactions
    .map((event) => event.eventId)
    .sort();
  const authority = {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    headEventId: null,
    headSeq: 0,
    acceptedEventIds: [],
    activeGrants: [],
    activeSeats: [],
  };
  const nativeFold = await invokeWithTauriFoldMock(
    {
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      authority,
      verifiedTransactions,
    },
    async () => ({
      schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
      implementation: "buzz-core",
      inputEventIds,
      context: {
        channelRef: CHANNEL,
        sessionRef: SESSION,
        genesisRef: GENESIS,
        founderPubkey: FOUNDER,
        authorityHeadEventId: null,
        authorityHeadSeq: 0,
        // L8.3: the adapter echoes the `verifierRequired` it was asked
        // with, and the decoder requires the key. These callers read no
        // policy, so they ask with `false` and are echoed `false`.
        verifierRequired: false,
      },
      includedEventIds: inputEventIds,
      excluded: [],
      conflicts: [],
      assignments,
      unseatedReports: [],
      notes: [],
      decisions: [],
      waitingOnDecision: null,
      pendingCompletion: null,
      canonicalTerminal: null,
    }),
  );
  return projectNativeTeamFoldToMissionInspector({ nativeFold });
}

test("authority comes only from the contiguous relay-receipted founder/seat chain", () => {
  const leadPayload = {
    type: "grant-seat",
    granteePubkey: LEAD,
    role: "lead",
    seq: 1,
    prevAccepted: null,
  };
  const leadGrant = transition(leadPayload);
  const verifierPayload = {
    type: "grant-seat",
    granteePubkey: VERIFIER,
    role: "verifier",
    seq: 2,
    prevAccepted: leadGrant.id,
    secret: LEAD_SECRET,
  };
  const verifierGrant = transition(verifierPayload);
  const projected = projectCodingSessionMissionAuthority(
    authorityInput(
      [leadGrant, verifierGrant],
      [
        receipt(leadGrant, leadPayload),
        receipt(verifierGrant, verifierPayload),
      ],
    ),
  );
  assert.equal(projected.ok, true);
  if (!projected.ok) return;
  assert.deepEqual(
    projected.value.activeSeats.map(({ actorPubkey, role }) => ({
      actorPubkey,
      role,
    })),
    [
      { actorPubkey: LEAD, role: "lead" },
      { actorPubkey: VERIFIER, role: "verifier" },
    ].sort((a, b) => a.actorPubkey.localeCompare(b.actorPubkey)),
  );
  assert.equal(projected.value.headEventId, verifierGrant.id);
  assert.equal(projected.value.headSeq, 2);
});

test("forged receipts, self-nomination, and lifecycle self-description confer no authority", () => {
  const leadPayload = {
    type: "grant-seat",
    granteePubkey: LEAD,
    role: "lead",
    seq: 1,
    prevAccepted: null,
  };
  const leadGrant = transition(leadPayload);
  const forged = projectCodingSessionMissionAuthority(
    authorityInput(
      [leadGrant],
      [receipt(leadGrant, leadPayload, { secret: STRANGER_SECRET })],
    ),
  );
  assert.equal(forged.ok, false);

  const selfPayload = {
    type: "grant-seat",
    granteePubkey: LEAD,
    role: "builder",
    seq: 2,
    prevAccepted: leadGrant.id,
    secret: LEAD_SECRET,
  };
  const selfGrant = transition(selfPayload);
  const selfNominated = projectCodingSessionMissionAuthority(
    authorityInput(
      [leadGrant, selfGrant],
      [receipt(leadGrant, leadPayload), receipt(selfGrant, selfPayload)],
    ),
  );
  assert.equal(selfNominated.ok, false);
  if (!selfNominated.ok)
    assert.match(selfNominated.error, /nominate its own signer/);

  const viewerPayload = {
    type: "grant-viewer",
    granteePubkey: STRANGER,
    seq: 1,
    prevAccepted: null,
  };
  const viewerGrant = transition(viewerPayload);
  const unauthorizedSeatPayload = {
    type: "grant-seat",
    granteePubkey: VERIFIER,
    role: "verifier",
    seq: 2,
    prevAccepted: viewerGrant.id,
    secret: STRANGER_SECRET,
  };
  const unauthorizedSeat = transition(unauthorizedSeatPayload);
  const viewerEscalation = projectCodingSessionMissionAuthority(
    authorityInput(
      [viewerGrant, unauthorizedSeat],
      [
        receipt(viewerGrant, viewerPayload),
        receipt(unauthorizedSeat, unauthorizedSeatPayload),
      ],
    ),
  );
  assert.equal(viewerEscalation.ok, false);
  if (!viewerEscalation.ok)
    assert.match(viewerEscalation.error, /unauthorized signer/);

  const lifecycleClaim = { ...leadGrant, kind: 44223 };
  const lifecycle = projectCodingSessionMissionAuthority(
    authorityInput([lifecycleClaim], [receipt(leadGrant, leadPayload)]),
  );
  assert.equal(lifecycle.ok, false);
  if (!lifecycle.ok) assert.match(lifecycle.error, /wrong kind|signature/);
});

test("cross-genesis accepted facts are ignored rather than borrowed", () => {
  const payload = {
    type: "grant-seat",
    granteePubkey: LEAD,
    role: "lead",
    seq: 1,
    prevAccepted: null,
    genesisRef: OTHER_GENESIS,
  };
  const other = transition(payload);
  const projected = projectCodingSessionMissionAuthority(
    authorityInput(
      [other],
      [receipt(other, payload, { genesisRef: OTHER_GENESIS })],
    ),
  );
  assert.equal(projected.ok, true);
  if (!projected.ok) return;
  assert.equal(projected.value.headSeq, 0);
  assert.deepEqual(projected.value.activeSeats, []);
});

test("a malformed signed receipt cannot preserve stale authority", () => {
  const grantPayload = {
    type: "grant-operator",
    granteePubkey: STRANGER,
    seq: 1,
    prevAccepted: null,
  };
  const grant = transition(grantPayload);
  const revokePayload = {
    type: "revoke",
    granteePubkey: STRANGER,
    seq: 2,
    prevAccepted: grant.id,
  };
  const revoke = transition(revokePayload);
  const malformedRevokeReceipt = sign({
    kind: 40099,
    content: JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: GENESIS,
      acceptedEventId: revoke.id,
      seq: 2,
      transitionType: "revoke",
      granteePubkey: STRANGER,
      unexpected: "must not be ignored",
    }),
    tags: [["h", CHANNEL]],
    secret: RELAY_SECRET,
  });
  const projected = projectCodingSessionMissionAuthority(
    authorityInput(
      [grant, revoke],
      [receipt(grant, grantPayload), malformedRevokeReceipt],
    ),
  );
  assert.equal(projected.ok, false);
  if (!projected.ok) assert.match(projected.error, /strict CSAT receipt shape/);
});

test("native Rust-fold wrapper binds exact inputs before Mission projection", async () => {
  const assignment = transaction({
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "assignment",
    supersedes: null,
    deliveryCommandId: "wake-builder-1",
    body: {
      assigneeActor: LEAD,
      assigneeRole: "builder",
      objective: "Build the bridge",
      brief: "Use the canonical fold.",
      branch: "singularity-stream",
      baseSha: "11".repeat(20),
      fileOwnership: ["desktop/src/features/coding-sessions/lib"],
      acceptanceSteps: ["pnpm test"],
    },
  });
  const report = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "report",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        assignmentRef: assignment.id,
        summary: "Bridge is green",
        branch: "singularity-stream",
        baseSha: "11".repeat(20),
        headSha: "22".repeat(20),
        files: ["desktop/src/features/coding-sessions/lib/bridge.ts"],
        tests: [
          {
            name: "desktop",
            command: "pnpm test",
            outcome: "passed",
            evidence: "exit 0",
          },
        ],
        redBeforeGreen: true,
        deviations: [],
        residuals: [],
        anomalies: [],
      },
    },
    LEAD_SECRET,
    2,
  );
  const blocked = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "mission.blocked",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        assignmentRefs: [assignment.id],
        summary: "Signing is held",
        blockers: ["Keychain is locked"],
        heldOn: "founder",
        requiredAction: "Unlock signing keys",
      },
    },
    FOUNDER_SECRET,
    3,
  );
  const verified = [assignment, report, blocked].map((event) => {
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    });
    assert.equal(decoded.ok, true);
    if (!decoded.ok) throw new Error(decoded.error);
    return decoded.value;
  });
  const authority = {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    headEventId: null,
    headSeq: 0,
    acceptedEventIds: [],
    activeGrants: [],
    activeSeats: [],
  };
  const inputEventIds = verified.map((event) => event.eventId).sort();
  const fold = {
    schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds,
    context: {
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      founderPubkey: FOUNDER,
      authorityHeadEventId: null,
      authorityHeadSeq: 0,
      // L8.3: the adapter echoes the `verifierRequired` it was asked
      // with, and the decoder requires the key. These callers read no
      // policy, so they ask with `false` and are echoed `false`.
      verifierRequired: false,
    },
    includedEventIds: [assignment.id, report.id, blocked.id],
    excluded: [],
    conflicts: [],
    assignments: [
      {
        assignmentEventId: assignment.id,
        governedReportEventId: null,
        dispositionEventId: null,
        acknowledgementEventId: null,
        settled: false,
        settledBy: null,
        awaiting: {
          link: "disposition",
          owedByRole: "lead",
          owedByActor: null,
        },
      },
    ],
    unseatedReports: [],
    notes: [],
    decisions: [],
    waitingOnDecision: null,
    pendingCompletion: null,
    canonicalTerminal: { eventId: blocked.id, type: "mission.blocked" },
  };
  const invocationInput = {
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority,
    verifiedTransactions: verified,
  };
  await assert.rejects(
    invokeWithTauriFoldMock(invocationInput, async () => ({
      ...fold,
      inputEventIds: fold.inputEventIds.slice(1),
    })),
    /does not match its request/,
  );
  await assert.rejects(
    invokeWithTauriFoldMock(invocationInput, async () => ({
      ...fold,
      inputEventIds: [...fold.inputEventIds, "ff".repeat(32)],
    })),
    /does not match its request/,
  );
  await assert.rejects(
    invokeWithTauriFoldMock(invocationInput, async () => ({
      ...fold,
      context: { ...fold.context, authorityHeadSeq: 1 },
    })),
    /does not match its request/,
  );
  await assert.rejects(
    invokeWithTauriFoldMock(invocationInput, async () => ({
      ...fold,
      excluded: [
        {
          eventId: "ff".repeat(32),
          code: "native-only-code",
          reason: "Unknown input must not cross the adapter.",
        },
      ],
    })),
    /does not partition its input ids/,
  );

  const mutableReport = verified.find(
    (event) => event.payload.type === "report",
  );
  assert.ok(mutableReport);
  mutableReport.payload.body.summary = "Mutable sidecar before invoke";
  let capturedRequest;
  const nativeFold = await invokeWithTauriFoldMock(
    {
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      authority,
      verifiedTransactions: verified,
    },
    async (command, args) => {
      assert.equal(command, CODING_SESSION_TEAM_FOLD_COMMAND);
      assert.equal(
        args.request.schema,
        CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA,
      );
      assert.deepEqual(args.request.inputEventIds, inputEventIds);
      assert.deepEqual(
        args.request.events.map((event) => event.id),
        inputEventIds,
      );
      capturedRequest = args.request;
      assert.ok(Object.isFrozen(args.request));
      assert.ok(Object.isFrozen(args.request.context));
      assert.ok(Object.isFrozen(args.request.inputEventIds));
      assert.ok(Object.isFrozen(args.request.events));
      assert.ok(Object.isFrozen(args.request.events[0]));
      assert.ok(Object.isFrozen(args.request.events[0].tags));
      assert.ok(Object.isFrozen(args.request.events[0].tags[0]));
      assert.throws(() => {
        args.request.events[0].content = "mutation must fail";
      }, TypeError);
      mutableReport.payload.body.summary = "Mutable sidecar during invoke";
      mutableReport.wireEvent.content = "Mutated original wire after clone";
      return fold;
    },
  );
  assert.ok(capturedRequest);
  assert.ok(Object.isFrozen(nativeFold));
  assert.ok(Object.isFrozen(nativeFold.fold));
  assert.ok(Object.isFrozen(nativeFold.fold.context));
  assert.ok(Object.isFrozen(nativeFold.fold.canonicalTerminal));
  assert.ok(Object.isFrozen(nativeFold.wireEvents));
  assert.ok(nativeFold.wireEvents.every(Object.isFrozen));
  assert.ok(
    nativeFold.wireEvents.every((event) =>
      event.tags.every((tag) => Object.isFrozen(tag)),
    ),
  );
  assert.throws(() => {
    nativeFold.wireEvents[0].content = "fabricated after branding";
  }, TypeError);
  assert.throws(() => {
    nativeFold.fold.canonicalTerminal.type = "mission.completed";
  }, TypeError);
  assert.throws(() => {
    nativeFold.fold = fold;
  }, TypeError);
  assert.throws(
    () =>
      projectNativeTeamFoldToMissionInspector({
        nativeFold: {
          fold: nativeFold.fold,
          wireEvents: nativeFold.wireEvents,
        },
      }),
    /not issued by the native wrapper/,
  );
  fold.canonicalTerminal.type = "mission.completed";
  const projected = projectNativeTeamFoldToMissionInspector({
    nativeFold,
  });
  assert.equal(projected.reports[0].sourceEventId, report.id);
  assert.equal(projected.reports[0].summary, "Bridge is green");
  assert.equal(projected.reports[0].tests[0].outcome, "passed");
  assert.deepEqual(projected.missionState, {
    kind: "blocked",
    sourceEventId: blocked.id,
    summary: "Signing is held",
    blockers: ["Keychain is locked"],
    requiredAction: "Unlock signing keys",
    canonicalChain: [
      {
        type: "assignment",
        sourceEventId: assignment.id,
        authorPubkey: FOUNDER,
        createdAt: 1,
        summary: "Build the bridge",
      },
      {
        type: "report",
        sourceEventId: report.id,
        authorPubkey: LEAD,
        createdAt: 2,
        summary: "Bridge is green",
      },
    ],
  });
  assert.deepEqual(projected.acceptedPlan, {
    kind: "available",
    steps: [
      {
        text: "pnpm test",
        sourceEventId: assignment.id,
        authorLabel: FOUNDER,
        sourceCreatedAt: 1,
        sourceIndex: 0,
      },
    ],
  });
  assert.deepEqual(projected.assignments, [
    {
      sourceEventId: assignment.id,
      authorLabel: FOUNDER,
      assigneeRole: "builder",
      objective: "Build the bridge",
      brief: "Use the canonical fold.",
      fileOwnership: ["desktop/src/features/coding-sessions/lib"],
    },
  ]);
});

test("accepted assignment chains preserve signed chronology and acknowledgement state", async () => {
  const assignment = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "assignment",
      supersedes: null,
      deliveryCommandId: "wake-builder-2",
      body: {
        assigneeActor: LEAD,
        assigneeRole: "builder",
        objective: "Preserve canonical Mission state",
        brief: "Project every accepted stage.",
        branch: "singularity-stream",
        baseSha: null,
        fileOwnership: ["desktop/src/features/coding-sessions/lib"],
        acceptanceSteps: ["pnpm test"],
      },
    },
    FOUNDER_SECRET,
    10,
  );
  const report = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "report",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        assignmentRef: assignment.id,
        summary: "Canonical projection implemented",
        branch: "singularity-stream",
        baseSha: null,
        headSha: null,
        files: ["desktop/src/features/coding-sessions/lib/projection.ts"],
        tests: [],
        redBeforeGreen: true,
        deviations: [],
        residuals: [],
        anomalies: [],
      },
    },
    LEAD_SECRET,
    20,
  );
  const disposition = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "verdict",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        subtype: "disposition",
        assignmentRef: assignment.id,
        reportRef: report.id,
        refutationRef: null,
        decision: "approve",
        summary: "Canonical projection approved",
        findings: [],
        requiredAction: null,
      },
    },
    FOUNDER_SECRET,
    30,
  );
  const acknowledgement = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "acknowledgement",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        acknowledgedEventRef: disposition.id,
        status: "received",
        note: "Approval received.",
      },
    },
    LEAD_SECRET,
    40,
  );
  const baseSettlement = {
    assignmentEventId: assignment.id,
    governedReportEventId: null,
    dispositionEventId: null,
    acknowledgementEventId: null,
    settled: false,
    settledBy: null,
    awaiting: { link: "disposition", owedByRole: "lead", owedByActor: null },
  };

  const assigned = (
    await projectAcceptedNonterminal([assignment], [baseSettlement])
  ).missionState;
  assert.equal(assigned.kind, "running");
  assert.equal(assigned.phase, "assigned");
  assert.deepEqual(
    assigned.canonicalChain.map(({ type }) => type),
    ["assignment"],
  );

  const reported = (
    await projectAcceptedNonterminal([report, assignment], [baseSettlement])
  ).missionState;
  assert.equal(reported.kind, "running");
  assert.equal(reported.phase, "reported");
  assert.deepEqual(
    reported.canonicalChain.map(({ type }) => type),
    ["assignment", "report"],
  );

  const awaiting = (
    await projectAcceptedNonterminal(
      [disposition, assignment, report],
      [baseSettlement],
    )
  ).missionState;
  assert.equal(awaiting.kind, "acknowledgement-required");
  assert.equal(awaiting.sourceEventId, disposition.id);
  assert.equal(awaiting.assignmentRef, assignment.id);
  assert.equal(awaiting.heldOn, "builder");
  assert.match(awaiting.requiredAction, /builder seat must acknowledge/);
  assert.deepEqual(
    awaiting.canonicalChain.map(({ type }) => type),
    ["assignment", "report", "disposition"],
  );

  const acknowledged = (
    await projectAcceptedNonterminal(
      [acknowledgement, report, disposition, assignment],
      [
        {
          assignmentEventId: assignment.id,
          governedReportEventId: report.id,
          dispositionEventId: disposition.id,
          acknowledgementEventId: acknowledgement.id,
          settled: true,
          settledBy: "acknowledgement",
          awaiting: null,
        },
      ],
    )
  ).missionState;
  assert.equal(acknowledged.kind, "running");
  assert.equal(acknowledged.phase, "acknowledged");
  assert.equal(acknowledged.sourceEventId, acknowledgement.id);
  assert.deepEqual(
    acknowledged.canonicalChain.map(({ type }) => type),
    ["assignment", "report", "disposition", "acknowledgement"],
  );
});

test("multi-assignment accepted steps retain their own source and author regardless of input order", async () => {
  const assignmentA = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "assignment",
      supersedes: null,
      deliveryCommandId: "wake-builder-a",
      body: {
        assigneeActor: LEAD,
        assigneeRole: "builder",
        objective: "Build the projection",
        brief: "Keep builder provenance.",
        branch: null,
        baseSha: null,
        fileOwnership: ["desktop/src/a.ts"],
        acceptanceSteps: ["Builder criterion"],
      },
    },
    FOUNDER_SECRET,
    10,
  );
  const assignmentB = transaction(
    {
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type: "assignment",
      supersedes: null,
      deliveryCommandId: "wake-verifier-b",
      body: {
        assigneeActor: FOUNDER,
        assigneeRole: "verifier",
        objective: "Verify the projection",
        brief: "Keep verifier provenance.",
        branch: null,
        baseSha: null,
        fileOwnership: ["desktop/src/b.ts"],
        acceptanceSteps: ["Verifier criterion"],
      },
    },
    LEAD_SECRET,
    20,
  );
  const settlements = [assignmentA, assignmentB].map((assignment) => ({
    assignmentEventId: assignment.id,
    governedReportEventId: null,
    dispositionEventId: null,
    acknowledgementEventId: null,
    settled: false,
    settledBy: null,
    awaiting: { link: "disposition", owedByRole: "lead", owedByActor: null },
  }));
  const forward = await projectAcceptedNonterminal(
    [assignmentA, assignmentB],
    settlements,
  );
  const reversed = await projectAcceptedNonterminal(
    [assignmentB, assignmentA],
    [...settlements].reverse(),
  );

  assert.deepEqual(forward.acceptedPlan, reversed.acceptedPlan);
  assert.deepEqual(forward.acceptedPlan.steps, [
    {
      text: "Builder criterion",
      sourceEventId: assignmentA.id,
      authorLabel: FOUNDER,
      sourceCreatedAt: 10,
      sourceIndex: 0,
    },
    {
      text: "Verifier criterion",
      sourceEventId: assignmentB.id,
      authorLabel: LEAD,
      sourceCreatedAt: 20,
      sourceIndex: 0,
    },
  ]);
});

function payload(type, body, overrides = {}) {
  return {
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type,
    supersedes: null,
    deliveryCommandId: null,
    body,
    ...overrides,
  };
}

async function projectWithFold(events, foldOverrides) {
  const verifiedTransactions = events.map((event) => {
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    });
    assert.equal(decoded.ok, true);
    if (!decoded.ok) throw new Error(decoded.error);
    return decoded.value;
  });
  const inputEventIds = verifiedTransactions
    .map((event) => event.eventId)
    .sort();
  const authority = {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    headEventId: null,
    headSeq: 0,
    acceptedEventIds: [],
    activeGrants: [],
    activeSeats: [],
  };
  const nativeFold = await invokeWithTauriFoldMock(
    {
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      authority,
      verifiedTransactions,
    },
    async () => ({
      schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
      implementation: "buzz-core",
      inputEventIds,
      context: {
        channelRef: CHANNEL,
        sessionRef: SESSION,
        genesisRef: GENESIS,
        founderPubkey: FOUNDER,
        authorityHeadEventId: null,
        authorityHeadSeq: 0,
        // L8.3: the adapter echoes the `verifierRequired` it was asked
        // with, and the decoder requires the key. These callers read no
        // policy, so they ask with `false` and are echoed `false`.
        verifierRequired: false,
      },
      includedEventIds: inputEventIds,
      excluded: [],
      conflicts: [],
      assignments: [],
      unseatedReports: [],
      notes: [],
      decisions: [],
      waitingOnDecision: null,
      pendingCompletion: null,
      canonicalTerminal: null,
      ...foldOverrides(inputEventIds),
    }),
  );
  return projectNativeTeamFoldToMissionInspector({ nativeFold });
}

test("D-T9: transactions project chronologically with counterparty and parent", async () => {
  const assignmentEvent = transaction(
    payload("assignment", {
      assigneeActor: LEAD,
      assigneeRole: "builder",
      objective: "Land the fence",
      brief: "Exact brief.",
      branch: null,
      baseSha: null,
      fileOwnership: [],
      acceptanceSteps: ["run tests"],
    }),
    FOUNDER_SECRET,
    10,
  );
  const reportEvent = transaction(
    payload("report", {
      assignmentRef: assignmentEvent.id,
      summary: "x".repeat(400),
      branch: null,
      baseSha: null,
      headSha: null,
      files: ["a.ts", "b.ts"],
      tests: [
        {
          name: "unit",
          command: "pnpm test",
          outcome: "passed",
          evidence: null,
        },
      ],
      redBeforeGreen: true,
      deviations: [],
      residuals: [],
      anomalies: [],
    }),
    LEAD_SECRET,
    20,
  );
  const verdictEvent = transaction(
    payload("verdict", {
      subtype: "disposition",
      assignmentRef: assignmentEvent.id,
      reportRef: reportEvent.id,
      refutationRef: null,
      decision: "approve",
      summary: "Good.",
      findings: [],
      requiredAction: null,
    }),
    FOUNDER_SECRET,
    30,
  );
  const projected = await projectWithFold(
    [verdictEvent, reportEvent, assignmentEvent],
    () => ({
      assignments: [
        {
          assignmentEventId: assignmentEvent.id,
          governedReportEventId: reportEvent.id,
          dispositionEventId: verdictEvent.id,
          acknowledgementEventId: null,
          settled: false,
          settledBy: null,
          awaiting: {
            link: "disposition",
            owedByRole: "lead",
            owedByActor: null,
          },
        },
      ],
      unseatedReports: [
        {
          eventId: reportEvent.id,
          authorPubkey: LEAD,
          assignmentRef: assignmentEvent.id,
          assigneeRole: "builder",
        },
      ],
    }),
  );
  assert.deepEqual(
    projected.transactions.map((row) => row.sourceEventId),
    [assignmentEvent.id, reportEvent.id, verdictEvent.id],
  );
  assert.equal(projected.transactionsTruncated, 0);
  const [assignmentRow, reportRow, verdictRow] = projected.transactions;
  assert.equal(assignmentRow.type, "assignment");
  assert.equal(assignmentRow.counterpartyPubkey, LEAD);
  assert.equal(assignmentRow.parentEventId, null);
  assert.equal(reportRow.type, "report");
  assert.equal(reportRow.parentEventId, assignmentEvent.id);
  assert.equal(reportRow.counterpartyPubkey, FOUNDER);
  assert.equal(reportRow.fileCount, 2);
  assert.equal(reportRow.testCount, 1);
  assert.equal(reportRow.unseated, true);
  assert.equal(reportRow.summary.length, 280);
  assert.ok(reportRow.summary.endsWith("…"));
  assert.equal(verdictRow.type, "disposition");
  assert.equal(verdictRow.decision, "approve");
  assert.equal(verdictRow.parentEventId, reportEvent.id);
  assert.equal(verdictRow.counterpartyPubkey, LEAD);
  assert.deepEqual(projected.unseatedReportEventIds, [reportEvent.id]);
});

test("D-T9: the decoder requires the fold's unseatedReports field", async () => {
  const assignmentEvent = transaction(
    payload("assignment", {
      assigneeActor: LEAD,
      assigneeRole: "builder",
      objective: "Land the fence",
      brief: "Exact brief.",
      branch: null,
      baseSha: null,
      fileOwnership: [],
      acceptanceSteps: ["run tests"],
    }),
    FOUNDER_SECRET,
    10,
  );
  const decoded = decodeVerifiedCodingSessionTeamTransaction({
    event: assignmentEvent,
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.equal(decoded.ok, true);
  const authority = {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
    relayPubkey: RELAY,
    headEventId: null,
    headSeq: 0,
    acceptedEventIds: [],
    activeGrants: [],
    activeSeats: [],
  };
  const base = {
    schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: [assignmentEvent.id],
    context: {
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      founderPubkey: FOUNDER,
      authorityHeadEventId: null,
      authorityHeadSeq: 0,
      // L8.3: the adapter echoes the `verifierRequired` it was asked
      // with, and the decoder requires the key. These callers read no
      // policy, so they ask with `false` and are echoed `false`.
      verifierRequired: false,
    },
    includedEventIds: [assignmentEvent.id],
    excluded: [],
    conflicts: [],
    assignments: [],
    pendingCompletion: null,
    canonicalTerminal: null,
  };
  const invocationInput = {
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority,
    verifiedTransactions: [decoded.value],
  };
  await assert.rejects(
    invokeWithTauriFoldMock(invocationInput, async () => base),
    /malformed response/,
  );
  const accepted = await invokeWithTauriFoldMock(invocationInput, async () => ({
    ...base,
    unseatedReports: [],
    notes: [],
    decisions: [],
    waitingOnDecision: null,
  }));
  assert.deepEqual(accepted.fold.unseatedReports, []);
  assert.ok(Object.isFrozen(accepted.fold.unseatedReports));
});

// B1c's three verbs, projected. Before this the projection cast every wire
// type it did not name straight through, so a `note` reached the surface with
// an empty summary and a type the surface's own tables had no entry for.
test("B1c: note and decision records project with their own signed words", async () => {
  const noteEvent = transaction(
    payload("note", {
      text: "The sidecar is stale; nothing is blocked.",
      refs: [],
    }),
    LEAD_SECRET,
    10,
  );
  const requestEvent = transaction(
    payload("decision.request", {
      question: "Land lane A now, or hold for the runner's gate?",
      options: ["Land lane A now", "Hold for the runner's gate"],
      heldOn: FOUNDER,
      blocks: [],
      recommendation: null,
    }),
    LEAD_SECRET,
    11,
  );
  const answerEvent = transaction(
    payload("decision.answer", {
      requestRef: requestEvent.id,
      choice: 1,
      note: "The gate is worth the wait.",
    }),
    FOUNDER_SECRET,
    12,
  );
  const projected = await projectWithFold(
    [noteEvent, requestEvent, answerEvent],
    () => ({}),
  );
  const rows = new Map(
    projected.transactions.map((row) => [row.sourceEventId, row]),
  );
  assert.equal(projected.transactions.length, 3);

  const note = rows.get(noteEvent.id);
  assert.equal(note.type, "note");
  assert.equal(note.summary, "The sidecar is stale; nothing is blocked.");
  // A note's `refs` are pointers, not causality: it never claims a parent.
  assert.equal(note.parentEventId, null);
  assert.equal(note.counterpartyPubkey, null);

  const request = rows.get(requestEvent.id);
  assert.equal(request.type, "decision.request");
  assert.equal(
    request.summary,
    "Land lane A now, or hold for the runner's gate?",
  );
  // A ruling held on a named actor names that actor as its counterparty.
  assert.equal(request.counterpartyPubkey, FOUNDER);
  assert.equal(request.parentEventId, null);

  const answer = rows.get(answerEvent.id);
  assert.equal(answer.type, "decision.answer");
  // The chosen option comes from the request's own signed options.
  assert.equal(
    answer.summary,
    "Hold for the runner's gate — The gate is worth the wait.",
  );
  assert.equal(answer.parentEventId, requestEvent.id);
  assert.equal(answer.counterpartyPubkey, LEAD);
});

test("B1c: an answer whose request the fold excluded names the index, not words", async () => {
  const requestEvent = transaction(
    payload("decision.request", {
      question: "Land lane A now, or hold?",
      options: ["Land lane A now", "Hold"],
      heldOn: "founder",
      blocks: [],
      recommendation: null,
    }),
    LEAD_SECRET,
    11,
  );
  const answerEvent = transaction(
    payload("decision.answer", {
      requestRef: requestEvent.id,
      choice: 0,
      note: null,
    }),
    FOUNDER_SECRET,
    12,
  );
  const projected = await projectWithFold([answerEvent], () => ({}));
  assert.equal(projected.transactions.length, 1);
  assert.equal(projected.transactions[0].summary, "option 1");
  // `heldOn: "founder"` names no key, so the request would carry no
  // counterparty; the answer's is its request's author, unresolvable here.
  assert.equal(projected.transactions[0].counterpartyPubkey, null);
});
