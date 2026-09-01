/**
 * The delivery plane, as a pure function of signed evidence plus Desktop's own
 * durable ledger.
 *
 * The cleantest replay (D-T1) is the headline: a provider wake reaching
 * `turn_queued` owns the operation even while its `turn_started` is minutes
 * away behind a long lead turn, and Desktop's 15-second grace must not read
 * that silence as an absent wake.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import {
  CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
  codingSessionSeatAuthorityCopy,
  codingSessionSeatRepairRemedy,
  codingSessionTeamWakeDeliveryCopy,
} from "./codingSessionMissionContracts.ts";
import {
  codingSessionSeatGrantKey,
  deriveCodingSessionSeatAuthorities,
  deriveCodingSessionTeamWakeDeliveries,
  deriveCodingSessionTeamWakeDeliveryPlan,
} from "./codingSessionTeamDeliveryStatus.ts";
import { CodingSessionTeamWakeEvidenceIndex } from "./codingSessionTeamWakeEvidence.ts";
import {
  baselineCodingSessionTeamWakes,
  CODING_SESSION_TEAM_WAKE_GRACE_MS,
  observeCodingSessionTeamWake,
  recordCodingSessionTeamWakeAttempt,
  recordCodingSessionTeamWakeCustody,
  recordCodingSessionTeamWakePublishFailure,
  recordCodingSessionTeamWakeReArm,
} from "./codingSessionTeamWake.ts";

const CHANNEL = "f4829942-15a8-4e74-accd-51c8448f250f";
const SESSION_REF = "9b1d1c40-1c1a-4d34-9c2f-2a1b3c4d5e6f";
const REPORT_ID = "e".repeat(64);
const BUILDER_ACTOR = "c".repeat(64);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const LEAD_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-primary",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const LEAD_TARGET_KEY = buildCodingSessionTargetKey(LEAD_TARGET);
const POINTER = JSON.stringify({ operationId: REPORT_ID, type: "report" });
const CONTEXT = {
  channelId: CHANNEL,
  leadTargetKey: LEAD_TARGET_KEY,
  providerAuthorityPubkey: PROVIDER_PUBKEY,
};

const CANDIDATE = {
  sourceEventId: REPORT_ID,
  sourceCreatedAtMs: 1_788_290_033_000,
  sourceEventSeq: null,
  sourceTargetKey: null,
  sourceActorPubkey: BUILDER_ACTOR,
  kind: "operation_ready",
  operationType: "report",
  seatRole: "builder",
  causedByCommandId: null,
  preferredCommandId: null,
};

function command({ secret, commandId, text = POINTER, createdAt = 1 }) {
  return finalizeEvent(
    {
      kind: 44220,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL],
        ["cs-v", "csc1-1"],
        ["cs-target", LEAD_TARGET_KEY],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-command/v1",
        commandId,
        target: LEAD_TARGET,
        action: { type: "thread.turn.start", text },
      }),
    },
    secret,
  );
}

function receipt({
  commandId,
  status,
  error = null,
  turnId = null,
  secret = null,
}) {
  return finalizeEvent(
    {
      kind: 44224,
      created_at: 2,
      tags: [
        ["h", CHANNEL],
        ["cs-target", LEAD_TARGET_KEY],
        ["csl-command", commandId],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lifecycle-receipt/v1",
        commandId,
        status,
        session: LEAD_TARGET,
        error,
        ...(turnId ? { turnId } : {}),
      }),
    },
    secret ?? PROVIDER_SECRET,
  );
}

function indexWith(events) {
  const index = new CodingSessionTeamWakeEvidenceIndex(CONTEXT);
  index.ingest(events);
  return index;
}

function observedState(atMs = 1_000) {
  return observeCodingSessionTeamWake(
    baselineCodingSessionTeamWakes([]),
    REPORT_ID,
    atMs + CODING_SESSION_TEAM_WAKE_GRACE_MS,
  );
}

function derive(overrides = {}) {
  return deriveCodingSessionTeamWakeDeliveryPlan({
    candidates: [CANDIDATE],
    leadTarget: LEAD_TARGET,
    leadTargetKey: LEAD_TARGET_KEY,
    founderPubkey: FOUNDER,
    index: null,
    acknowledgedCommandIds: new Set(),
    acknowledgedSourceEventIds: new Set(),
    state: observedState(),
    nowMs: 1_000,
    evidenceComplete: true,
    ...overrides,
  });
}

test("D-T1: a queued provider wake owns the operation past the grace window", () => {
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "team-wake-2abd9f9a" }),
    receipt({ commandId: "team-wake-2abd9f9a", status: "turn_queued" }),
  ]);
  const plan = derive({ index, nowMs: 1_000 + 90_000 });
  const [delivery] = plan.deliveries;
  assert.equal(delivery.kind, "provider-queued");
  assert.equal(delivery.owningCommandId, "team-wake-2abd9f9a");
  assert.equal(
    delivery.detail,
    codingSessionTeamWakeDeliveryCopy["provider-queued"].detail,
  );
  assert.equal(delivery.sourceEventId, REPORT_ID);
  assert.equal(delivery.sourceActorPubkey, BUILDER_ACTOR);
  assert.equal(delivery.leadTargetKey, LEAD_TARGET_KEY);
  assert.equal(delivery.operationType, "report");
  assert.equal(delivery.reArmCount, 0);
  assert.deepEqual([...plan.suppressedSourceEventIds], [REPORT_ID]);
  assert.deepEqual([...plan.reArmEligibleSourceEventIds], []);

  index.ingest([
    receipt({
      commandId: "team-wake-2abd9f9a",
      status: "turn_started",
      turnId: "turn-9",
    }),
  ]);
  assert.equal(
    derive({ index, nowMs: 1_000 + 90_000 }).deliveries[0].kind,
    "provider-started",
  );
});

test("D-T4: forged, foreign and near-miss evidence leaves the wake in grace", () => {
  const foreign = generateSecretKey();
  const index = indexWith([
    // A command anyone may publish, receipted by a key that is not the lead's
    // provider authority: no settlement (I15).
    command({ secret: foreign, commandId: "foreign-wake" }),
    receipt({
      commandId: "foreign-wake",
      status: "turn_queued",
      secret: foreign,
    }),
    // A genuine provider command and receipt — for a pointer that differs
    // from ours by one trailing byte.
    command({
      secret: PROVIDER_SECRET,
      commandId: "one-byte-off",
      text: `${POINTER} `,
    }),
    receipt({ commandId: "one-byte-off", status: "turn_started", turnId: "t" }),
  ]);
  const [delivery] = derive({ index, nowMs: 1_000 }).deliveries;
  assert.equal(delivery.kind, "fallback-grace");
  assert.equal(
    delivery.detail,
    codingSessionTeamWakeDeliveryCopy["fallback-grace"].detail,
  );
  assert.equal(delivery.owningCommandId, null);
});

test("D-T4: a foreign-signed command with no receipt cannot settle the operation", () => {
  const foreign = generateSecretKey();
  const index = indexWith([
    command({ secret: foreign, commandId: "foreign-wake" }),
  ]);
  const plan = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(plan.deliveries[0].kind, "fallback-grace");
  assert.deepEqual([...plan.suppressedSourceEventIds], []);
});

test("F4: a stranger's UNAUTHORIZED_OPERATOR refusal never joins the command set", () => {
  const stranger = generateSecretKey();
  const index = indexWith([
    command({ secret: stranger, commandId: "stranger-cmd" }),
    receipt({
      commandId: "stranger-cmd",
      status: "turn_refused",
      error: {
        code: "UNAUTHORIZED_OPERATOR",
        message: "only the founder or a granted operator may steer this",
      },
    }),
  ]);
  const plan = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(plan.deliveries[0].kind, "fallback-grace");
  assert.deepEqual(
    plan.deliveries[0].failures,
    [],
    "a command that never held authority never failed to deliver anything",
  );
  assert.deepEqual([...plan.suppressedSourceEventIds], []);
  assert.deepEqual(
    [...plan.reArmEligibleSourceEventIds],
    [],
    "a stranger cannot spend Desktop's one re-arm before Desktop has published",
  );
});

test("F4: the first Desktop cover uses the base id; only a spent Desktop command earns :r1", () => {
  const stranger = generateSecretKey();
  const index = indexWith([
    command({ secret: stranger, commandId: "stranger-cmd" }),
    receipt({
      commandId: "stranger-cmd",
      status: "turn_refused",
      error: { code: "UNAUTHORIZED_OPERATOR", message: "not authorised" },
    }),
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  assert.deepEqual(
    [...derive({ index, nowMs: 1_000 + 90_000 }).reArmEligibleSourceEventIds],
    [],
    "Desktop has never published for this source, so its first cover is not a re-arm",
  );

  const attempted = recordCodingSessionTeamWakeAttempt({
    state: observedState(),
    candidate: CANDIDATE,
    commandId: "team-wake-v1:desktop",
    leadTarget: LEAD_TARGET,
    acknowledgedCommandIds: new Set(),
  });
  index.ingest([
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  assert.deepEqual(
    [
      ...derive({ index, state: attempted, nowMs: 1_000 + 90_000 })
        .reArmEligibleSourceEventIds,
    ],
    [REPORT_ID],
    "now a Desktop-owned command has really been spent, so the re-arm is due",
  );
});

test("D-T2: every command failing without a duplicate code arms exactly one re-arm", () => {
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "team-wake-2abd9f9a" }),
    receipt({
      commandId: "team-wake-2abd9f9a",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const first = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(first.deliveries[0].kind, "fallback-grace");
  assert.deepEqual(
    [...first.reArmEligibleSourceEventIds],
    [],
    "Desktop's own first cover always uses the base id",
  );
  assert.equal(first.deliveries[0].failures.length, 1);
  assert.equal(first.deliveries[0].failures[0].code, "QUEUE_FULL");

  const reArmed = recordCodingSessionTeamWakeReArm(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "team-wake-v1:redo:r1",
  });
  index.ingest([
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:redo:r1" }),
    receipt({
      commandId: "team-wake-v1:redo:r1",
      status: "turn_dropped",
      error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
    }),
  ]);
  const second = derive({
    index,
    state: reArmed,
    nowMs: 1_000 + 90_000,
  });
  assert.equal(second.deliveries[0].kind, "failed");
  assert.equal(second.deliveries[0].reArmCount, 1);
  assert.deepEqual([...second.reArmEligibleSourceEventIds], []);
  assert.equal(
    second.deliveries[0].detail,
    CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
    "an all-dropped operation names the §2a residual rather than a generic failure",
  );
});

test("D-T5: a DUPLICATE_OPERATION refusal of Desktop's own command is settlement", () => {
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "team-wake-2abd9f9a" }),
    receipt({ commandId: "team-wake-2abd9f9a", status: "turn_queued" }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_refused",
      error: { code: "DUPLICATE_OPERATION", message: "team-wake-2abd9f9a" },
    }),
  ]);
  const plan = derive({ index, nowMs: 1_000 + 90_000 });
  const [delivery] = plan.deliveries;
  assert.equal(delivery.kind, "provider-queued");
  assert.deepEqual(delivery.duplicateRefusedCommandIds, [
    "team-wake-v1:desktop",
  ]);
  assert.deepEqual(delivery.failures, []);
  assert.deepEqual([...plan.suppressedSourceEventIds], [REPORT_ID]);
  assert.deepEqual([...plan.reArmEligibleSourceEventIds], []);
});

test("D-T5: a duplicate refusal alone spends that attempt and nothing else", () => {
  const index = indexWith([
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_refused",
      error: {
        code: "DUPLICATE_OPERATION",
        message: "another command owns it",
      },
    }),
  ]);
  const plan = derive({ index, nowMs: 1_000 + 90_000 });
  assert.deepEqual(
    [...plan.suppressedSourceEventIds],
    [],
    "a refusal is not a receipt from the command that actually owns the operation",
  );
  assert.deepEqual(
    [...plan.resolvedSourceEventIds],
    [],
    "and it certainly does not prove a turn ran",
  );
  assert.deepEqual(
    [...plan.reArmEligibleSourceEventIds],
    [],
    "no non-duplicate failure exists yet, so nothing is due",
  );
  assert.deepEqual([...plan.spentCommandIds], ["team-wake-v1:desktop"]);
  assert.deepEqual(plan.deliveries[0].failures, []);
});

test("D-T7: incomplete evidence is unknown and is never eligible to publish", () => {
  const plan = derive({ evidenceComplete: false, nowMs: 1_000 + 90_000 });
  assert.equal(plan.deliveries[0].kind, "unknown");
  assert.equal(
    plan.deliveries[0].detail,
    codingSessionTeamWakeDeliveryCopy.unknown.detail,
  );
  assert.deepEqual([...plan.suppressedSourceEventIds], []);
  assert.deepEqual([...plan.reArmEligibleSourceEventIds], []);
  assert.equal(plan.deliveries[0].owningCommandId, null);
});

test("Desktop's own accepted publish reads as covered, not as provider delivery", () => {
  const attempted = recordCodingSessionTeamWakeAttempt({
    state: observedState(),
    candidate: CANDIDATE,
    commandId: "team-wake-v1:desktop",
    leadTarget: LEAD_TARGET,
    acknowledgedCommandIds: new Set(),
  });
  const unconfirmed = derive({
    state: attempted,
    index: indexWith([]),
    nowMs: 1_000 + 90_000,
  });
  assert.equal(unconfirmed.deliveries[0].kind, "fallback-unconfirmed");
  assert.equal(
    unconfirmed.deliveries[0].detail,
    codingSessionTeamWakeDeliveryCopy["fallback-unconfirmed"].detail,
  );

  const queued = derive({
    state: attempted,
    index: indexWith([
      command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
      receipt({ commandId: "team-wake-v1:desktop", status: "turn_queued" }),
    ]),
    nowMs: 1_000 + 90_000,
  });
  assert.equal(queued.deliveries[0].kind, "fallback-queued");
  assert.equal(queued.deliveries[0].owningCommandId, "team-wake-v1:desktop");
});

test("a publish that threw is a failure without waiting for a receipt", () => {
  const plan = derive({
    index: indexWith([]),
    nowMs: 1_000 + 90_000,
    publishFailedSourceEventIds: new Set([REPORT_ID]),
  });
  assert.equal(plan.deliveries[0].kind, "failed");
  assert.equal(
    plan.deliveries[0].detail,
    codingSessionTeamWakeDeliveryCopy.failed.detail,
  );
});

test("deriveCodingSessionTeamWakeDeliveries returns the plan's delivery rows", () => {
  const rows = deriveCodingSessionTeamWakeDeliveries({
    candidates: [CANDIDATE],
    leadTarget: LEAD_TARGET,
    leadTargetKey: LEAD_TARGET_KEY,
    founderPubkey: FOUNDER,
    index: null,
    acknowledgedCommandIds: new Set(),
    acknowledgedSourceEventIds: new Set(),
    state: observedState(),
    nowMs: 1_000,
    evidenceComplete: true,
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].kind, "fallback-grace");
  assert.equal(
    rows[0].observedAtMs,
    1_000,
    "observation is local wall clock, never the signed created_at",
  );
});

test("D-T8: seat authority is granted, created-ungranted, or unknown — never guessed", () => {
  const umbrella = {
    sessionRef: SESSION_REF,
    executions: [
      {
        executionKey: "lead",
        activeGeneration: { agentRef: "b".repeat(64), role: "lead" },
      },
      {
        executionKey: "builder",
        activeGeneration: { agentRef: BUILDER_ACTOR, role: "builder" },
      },
      {
        executionKey: "human",
        activeGeneration: { agentRef: null, role: null },
      },
    ],
  };
  const granted = deriveCodingSessionSeatAuthorities({
    channelId: CHANNEL,
    umbrella,
    authority: {
      activeSeats: [
        { actorPubkey: "b".repeat(64), role: "lead" },
        { actorPubkey: BUILDER_ACTOR, role: "verifier" },
      ],
      seatGrantRefs: {
        [codingSessionSeatGrantKey("b".repeat(64), "lead")]: "a1".repeat(32),
      },
    },
  });
  assert.equal(granted.length, 2, "only receipt-backed seats are listed");
  assert.equal(granted[0].kind, "granted");
  assert.equal(granted[0].grantEventId, "a1".repeat(32));
  assert.equal(
    granted[0].detail,
    codingSessionSeatAuthorityCopy.granted.detail,
  );
  assert.equal(granted[0].remedy, null);
  assert.equal(granted[1].kind, "created-ungranted");
  assert.equal(
    granted[1].detail,
    codingSessionSeatAuthorityCopy["created-ungranted"].detail,
  );
  assert.equal(
    granted[1].remedy,
    codingSessionSeatRepairRemedy({
      channelId: CHANNEL,
      sessionRef: SESSION_REF,
      actorPubkey: BUILDER_ACTOR,
    }),
  );

  const unknown = deriveCodingSessionSeatAuthorities({
    channelId: CHANNEL,
    umbrella,
    authority: null,
  });
  assert.deepEqual(
    unknown.map((row) => row.kind),
    ["unknown", "unknown"],
  );
  assert.equal(unknown[0].remedy, null);
});

test("F1: turn_queued is custody, not permanent resolution", () => {
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({ commandId: "provider-cmd", status: "turn_queued" }),
  ]);
  const custody = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(custody.deliveries[0].kind, "provider-queued");
  assert.deepEqual(
    [...custody.resolvedSourceEventIds],
    [],
    "queued is custody; only a start or a lead echo resolves permanently",
  );
  assert.deepEqual(
    [...custody.suppressedSourceEventIds],
    [REPORT_ID],
    "custody still suppresses the Desktop fallback",
  );
  assert.deepEqual(custody.custodiedSources, [
    {
      sourceEventId: REPORT_ID,
      leadTargetKey: LEAD_TARGET_KEY,
      commandId: "provider-cmd",
      fromProvider: true,
    },
  ]);

  index.ingest([
    receipt({
      commandId: "provider-cmd",
      status: "turn_started",
      turnId: "turn-9",
    }),
  ]);
  const started = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(started.deliveries[0].kind, "provider-started");
  assert.deepEqual([...started.resolvedSourceEventIds], [REPORT_ID]);
});

test("F1: a degraded steer is custody too, and a lead echo resolves outright", () => {
  const degraded = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_degraded",
      error: { code: "STEER_UNSUPPORTED", message: "no native steering" },
    }),
  ]);
  const custody = derive({ index: degraded, nowMs: 1_000 + 90_000 });
  assert.equal(custody.deliveries[0].kind, "provider-queued");
  assert.deepEqual([...custody.resolvedSourceEventIds], []);
  assert.deepEqual([...custody.suppressedSourceEventIds], [REPORT_ID]);

  const echoed = derive({
    index: degraded,
    nowMs: 1_000 + 90_000,
    acknowledgedSourceEventIds: new Set([REPORT_ID]),
  });
  assert.equal(echoed.deliveries[0].kind, "provider-started");
  assert.deepEqual([...echoed.resolvedSourceEventIds], [REPORT_ID]);
});

test("F1: a foreign DUPLICATE refusal never resolves, custodies, or vetoes the re-arm", () => {
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({ commandId: "provider-cmd", status: "turn_queued" }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_refused",
      error: { code: "DUPLICATE_OPERATION", message: "provider-cmd owns it" },
    }),
  ]);
  const custodied = derive({ index, nowMs: 1_000 + 90_000 });
  assert.equal(custodied.deliveries[0].kind, "provider-queued");
  assert.deepEqual([...custodied.resolvedSourceEventIds], []);
  assert.deepEqual([...custodied.suppressedSourceEventIds], [REPORT_ID]);
  assert.deepEqual(custodied.deliveries[0].duplicateRefusedCommandIds, [
    "team-wake-v1:desktop",
  ]);

  // The custodian is dropped without ever starting: the runner's fence is
  // released, so Desktop's single re-arm is due.
  index.ingest([
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
    }),
  ]);
  const released = derive({ index, nowMs: 1_000 + 90_000 });
  assert.deepEqual([...released.resolvedSourceEventIds], []);
  assert.deepEqual([...released.suppressedSourceEventIds], []);
  assert.deepEqual(
    [...released.reArmEligibleSourceEventIds],
    [REPORT_ID],
    "a duplicate refusal must never disable the re-arm forever",
  );
  assert.equal(released.deliveries[0].kind, "fallback-grace");
});

test("F1: a recorded custody row survives evidence loss and still suppresses", () => {
  const custodied = recordCodingSessionTeamWakeCustody(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "provider-cmd",
  });
  const plan = derive({
    index: indexWith([]),
    state: custodied,
    nowMs: 1_000 + 90_000,
  });
  assert.deepEqual([...plan.suppressedSourceEventIds], [REPORT_ID]);
  assert.deepEqual([...plan.reArmEligibleSourceEventIds], []);
  assert.equal(plan.deliveries[0].kind, "provider-queued");
  assert.equal(plan.deliveries[0].owningCommandId, "provider-cmd");
});

test("F1: recorded custody is released when its own command is dropped", () => {
  const custodied = recordCodingSessionTeamWakeCustody(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "provider-cmd",
  });
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const plan = derive({ index, state: custodied, nowMs: 1_000 + 90_000 });
  assert.deepEqual([...plan.releasedCustodySourceEventIds], [REPORT_ID]);
  assert.deepEqual([...plan.suppressedSourceEventIds], []);
  assert.equal(plan.deliveries[0].kind, "fallback-grace");
  assert.deepEqual(
    [...plan.reArmEligibleSourceEventIds],
    [],
    "Desktop has published nothing yet, so its cover is a base-id publish",
  );

  const spent = recordCodingSessionTeamWakeAttempt({
    state: custodied,
    candidate: CANDIDATE,
    commandId: "team-wake-v1:desktop",
    leadTarget: LEAD_TARGET,
    acknowledgedCommandIds: new Set(),
  });
  index.ingest([
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  assert.deepEqual(
    [
      ...derive({ index, state: spent, nowMs: 1_000 + 90_000 })
        .reArmEligibleSourceEventIds,
    ],
    [REPORT_ID],
    "once Desktop's own command is spent too, the one re-arm is due",
  );
});

test("F3: the §2a residual names the lead only when the lead really dropped it", () => {
  const desktopOnly = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_refused",
      error: { code: "DUPLICATE_OPERATION", message: "another owns it" },
    }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop" }),
    receipt({
      commandId: "team-wake-v1:desktop",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop:r1" }),
    receipt({
      commandId: "team-wake-v1:desktop:r1",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const spent = recordCodingSessionTeamWakeReArm(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "team-wake-v1:desktop:r1",
  });
  const plan = derive({ index: desktopOnly, state: spent, nowMs: 1_000 });
  assert.equal(plan.deliveries[0].kind, "failed");
  assert.equal(
    plan.deliveries[0].detail,
    codingSessionTeamWakeDeliveryCopy.failed.detail,
    "a duplicate refusal is not the lead dropping anything",
  );

  const providerDropped = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
    }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop:r1" }),
    receipt({
      commandId: "team-wake-v1:desktop:r1",
      status: "turn_dropped",
      error: { code: "NO_LIVE_EXECUTION", message: "the lead is gone" },
    }),
  ]);
  const residual = derive({
    index: providerDropped,
    state: spent,
    nowMs: 1_000,
  });
  assert.equal(residual.deliveries[0].kind, "failed");
  assert.equal(
    residual.deliveries[0].detail,
    CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
  );
});

test("D-6: an orphaned Desktop custody row is still Desktop's, and says so", () => {
  const orphaned = recordCodingSessionTeamWakeCustody(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "team-wake-v1:desktop",
    fromProvider: false,
  });
  const plan = derive({
    index: indexWith([]),
    state: orphaned,
    nowMs: 1_000 + 90_000,
  });
  assert.equal(plan.deliveries[0].kind, "fallback-queued");
  assert.equal(
    plan.deliveries[0].detail,
    codingSessionTeamWakeDeliveryCopy["fallback-queued"].detail,
    "Desktop covered for the provider — not 'Provider wake queued'",
  );
  assert.equal(plan.deliveries[0].owningCommandId, "team-wake-v1:desktop");

  const foreign = recordCodingSessionTeamWakeCustody(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "provider-cmd",
    fromProvider: true,
  });
  assert.equal(
    derive({ index: indexWith([]), state: foreign, nowMs: 1_000 }).deliveries[0]
      .kind,
    "provider-queued",
  );
});

test("D-5: a durable publish failure discloses, and never fences the next mount", () => {
  const threw = recordCodingSessionTeamWakePublishFailure(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
  });
  const plan = derive({
    index: indexWith([]),
    state: threw,
    nowMs: 1_000 + 90_000,
  });
  assert.equal(
    plan.deliveries[0].kind,
    "failed",
    "the row keeps telling the truth about the throw",
  );
  assert.deepEqual(
    [...plan.publishEligibleSourceEventIds],
    [REPORT_ID],
    "but a transient relay error must not lose the wake forever",
  );

  const spent = recordCodingSessionTeamWakeReArm(threw, {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
    commandId: "team-wake-v1:desktop:r1",
  });
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
    command({ secret: FOUNDER_SECRET, commandId: "team-wake-v1:desktop:r1" }),
    receipt({
      commandId: "team-wake-v1:desktop:r1",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const terminal = derive({ index, state: spent, nowMs: 1_000 + 90_000 });
  assert.equal(terminal.deliveries[0].kind, "failed");
  assert.deepEqual(
    [...terminal.publishEligibleSourceEventIds],
    [],
    "receipt-backed failure with the re-arm spent stays terminal",
  );
});

test("D-5: a publish that threw is not a Desktop command, so the retry uses the base id", () => {
  const threw = recordCodingSessionTeamWakePublishFailure(observedState(), {
    sourceEventId: REPORT_ID,
    leadTargetKey: LEAD_TARGET_KEY,
  });
  const index = indexWith([
    command({ secret: PROVIDER_SECRET, commandId: "provider-cmd" }),
    receipt({
      commandId: "provider-cmd",
      status: "turn_dropped",
      error: { code: "QUEUE_FULL", message: "mailbox is full" },
    }),
  ]);
  const plan = derive({ index, state: threw, nowMs: 1_000 + 90_000 });
  assert.deepEqual([...plan.publishEligibleSourceEventIds], [REPORT_ID]);
  assert.deepEqual(
    [...plan.reArmEligibleSourceEventIds],
    [],
    "nothing reached the relay, so Desktop has not spent a command yet",
  );
});
