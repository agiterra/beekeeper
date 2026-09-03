import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionRouteTransactions,
  codingSessionRouteAttentionSigns,
  CODING_SESSION_ROUTE_FOUNDER_ROAD,
  deriveCodingSessionRoute,
  formatCodingSessionRouteDuration,
  ROUTE_BAND_MIN_PX,
  ROUTE_PX_PER_MINUTE,
  ROUTE_SIGN_SLOT_PX,
  ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT,
  ROUTE_SIGNS_PER_ROAD_LIMIT,
  ROUTE_STRETCH_PX,
  codingSessionRouteFits,
} from "./codingSessionRouteModel.ts";
import { buildCodingSessionMissionTransactionRows } from "./codingSessionMissionTransactionRows.ts";

// The TeamRolesV1 run (`review-2026-09-01/LIVE-RUN-TeamRolesV1.md`), on a fixed
// day so nothing here depends on the machine's clock. Every moment below is one
// the run actually published: Keystone hired Bob at 9:39 PM, assigned lane A at
// 9:46, recorded `mission.blocked` as a note at 9:57, Bob's report landed 9:58
// with its wake queued 4 m 20 s, Keystone hired the runner at 10:02, ruled at
// 10:14, Bob acknowledged at 10:16, and the lead completed the mission at
// 2:35 AM — over a `mission.blocked` the vocabulary gave it no way to clear.
const DAY = 1_788_400_000; // an arbitrary fixed midnight, in unix seconds
const at = (hours, minutes, seconds = 0) =>
  DAY + hours * 3_600 + minutes * 60 + seconds;
const HIRE_BOB = at(21, 39);
const ASSIGNMENT = at(21, 46);
const BLOCKED = at(21, 57);
const REPORT = at(21, 58);
const HIRE_RUNNER = at(22, 2);
const VERDICT = at(22, 14);
const ACK = at(22, 16);
const COMPLETED = at(26, 35); // 2:35 AM the next day

const FOUNDER = "f0".repeat(32);
const LEAD_ACTOR = "1a".repeat(32);
const BUILDER_ACTOR = "2b".repeat(32);
const RUNNER_ACTOR = "3c".repeat(32);
const STRANGER = "9e".repeat(32);
const LEAD_TARGET = "target:lead";

const ASSIGNMENT_ID = "a1".repeat(32);
const REPORT_ID = "b2".repeat(32);
const VERDICT_ID = "c3".repeat(32);
const ACK_ID = "d4".repeat(32);
const BLOCKED_ID = "e5".repeat(32);
const COMPLETED_ID = "f6".repeat(32);

function participants() {
  return [
    {
      executionKey: "exec-lead",
      label: "Keystone · Lead",
      actorPubkey: LEAD_ACTOR,
      role: "lead",
      targetKey: LEAD_TARGET,
      word: "live",
      live: true,
      firstSignedAt: at(21, 34),
      releasedAt: null,
    },
    {
      executionKey: "exec-builder",
      label: "Bob · Builder",
      actorPubkey: BUILDER_ACTOR,
      role: "builder",
      targetKey: "target:builder",
      word: "idle",
      live: false,
      firstSignedAt: HIRE_BOB,
      releasedAt: null,
    },
    {
      executionKey: "exec-runner",
      label: "Gordan · Runner",
      actorPubkey: RUNNER_ACTOR,
      role: "runner",
      targetKey: "target:runner",
      word: "idle",
      live: false,
      firstSignedAt: HIRE_RUNNER,
      releasedAt: null,
    },
  ];
}

function hires() {
  return [
    {
      executionKey: "exec-builder",
      hiredByPubkey: LEAD_ACTOR,
      at: HIRE_BOB,
      sourceEventId: "11".repeat(32),
    },
    {
      executionKey: "exec-runner",
      hiredByPubkey: LEAD_ACTOR,
      at: HIRE_RUNNER,
      sourceEventId: "22".repeat(32),
    },
  ];
}

/** One signed 44244, in the shape Lane D's projection hands the stream. */
function transactionInput(overrides) {
  return {
    sourceEventId: "00".repeat(32),
    type: "assignment",
    authorPubkey: LEAD_ACTOR,
    createdAt: ASSIGNMENT,
    counterpartyPubkey: BUILDER_ACTOR,
    parentEventId: null,
    summary: "Lane A — both sides, red-first, gates",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
    ...overrides,
  };
}

function missionInputs() {
  return [
    transactionInput({
      sourceEventId: ASSIGNMENT_ID,
      type: "assignment",
      createdAt: ASSIGNMENT,
    }),
    transactionInput({
      sourceEventId: BLOCKED_ID,
      type: "mission.blocked",
      createdAt: BLOCKED,
      counterpartyPubkey: null,
      summary: "Lane B holds until lane A's gate report lands.",
      requiredAction: "Rule on lane A before lane B starts.",
    }),
    transactionInput({
      sourceEventId: REPORT_ID,
      type: "report",
      authorPubkey: BUILDER_ACTOR,
      counterpartyPubkey: LEAD_ACTOR,
      createdAt: REPORT,
      parentEventId: ASSIGNMENT_ID,
      summary: "Lane A — both sides, gates run",
    }),
    transactionInput({
      sourceEventId: VERDICT_ID,
      type: "disposition",
      createdAt: VERDICT,
      parentEventId: REPORT_ID,
      decision: "approve-with-notes",
      summary: "The signed report is approved.",
    }),
    transactionInput({
      sourceEventId: ACK_ID,
      type: "acknowledgement",
      authorPubkey: BUILDER_ACTOR,
      counterpartyPubkey: LEAD_ACTOR,
      createdAt: ACK,
      parentEventId: VERDICT_ID,
      summary: "Notes taken.",
    }),
    transactionInput({
      sourceEventId: COMPLETED_ID,
      type: "mission.completed",
      createdAt: COMPLETED,
      counterpartyPubkey: null,
      summary: "TeamRolesV1 is complete.",
    }),
  ];
}

const ACTORS = new Map([
  [LEAD_ACTOR, { label: "Keystone · Lead", executionKey: "exec-lead" }],
  [BUILDER_ACTOR, { label: "Bob · Builder", executionKey: "exec-builder" }],
  [RUNNER_ACTOR, { label: "Gordan · Runner", executionKey: "exec-runner" }],
]);

function routeTransactions(inputs) {
  const rows = buildCodingSessionMissionTransactionRows({
    transactions: inputs,
    resolveActor: (pubkey) =>
      ACTORS.get(pubkey) ?? { label: null, executionKey: null },
    founderPubkey: FOUNDER,
    deliveries: [],
    density: "live",
  });
  return buildCodingSessionRouteTransactions(rows, inputs);
}

function queuedDelivery(observedAt) {
  return {
    sourceEventId: REPORT_ID,
    operationType: "report",
    sourceActorPubkey: BUILDER_ACTOR,
    leadTargetKey: LEAD_TARGET,
    kind: "provider-queued",
    owningCommandId: "cmd-1",
    duplicateRefusedCommandIds: [],
    failures: [],
    reArmCount: 0,
    observedAtMs: observedAt * 1_000,
    detail: "Provider wake queued",
  };
}

const UNGRANTED_RUNNER = {
  executionKey: "exec-runner",
  actorPubkey: RUNNER_ACTOR,
  role: "runner",
  kind: "created-ungranted",
  grantEventId: null,
  detail: "Seat created, not granted",
  remedy: "bee sessions seat-repair --channel c --session-ref s --actor a",
};

/** The run at 10:02:20 PM: the report's wake has been queued 4 m 20 s. */
function routeAtQueuedWake() {
  const inputs = missionInputs().filter(
    (input) => input.createdAt <= HIRE_RUNNER,
  );
  return deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(inputs),
    deliveries: [queuedDelivery(REPORT)],
    seatAuthorities: [UNGRANTED_RUNNER],
    hires: hires(),
    nowMs: (REPORT + 260) * 1_000,
  });
}

/** The whole run, at 2:36 AM — one minute after `mission.completed`. */
function routeAtCompletion() {
  return deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    deliveries: [{ ...queuedDelivery(REPORT), kind: "provider-started" }],
    seatAuthorities: [UNGRANTED_RUNNER],
    hires: hires(),
    nowMs: (COMPLETED + 60) * 1_000,
  });
}

test("the route lays the TeamRolesV1 run out as roads, founder first and lead second", () => {
  const route = routeAtCompletion();
  assert.deepEqual(
    route.roads.map((road) => road.label),
    ["You", "Keystone · Lead", "Bob · Builder", "Gordan · Runner"],
  );
  assert.equal(route.roads[0].key, CODING_SESSION_ROUTE_FOUNDER_ROAD);
  assert.equal(route.roads[0].founder, true);
  // R8: the founder is not a seat and carries no W1 word.
  assert.equal(route.roads[0].head.word, null);
  assert.equal(route.roads[1].head.word, "live");
  assert.equal(route.roads[1].live, true);
  // A seat's road starts at its own hire, and says so.
  assert.equal(route.roads[2].startedAt, HIRE_BOB);
  assert.equal(route.roads[2].startedAtSource, "hire");
  assert.equal(route.roads[3].startedAt, HIRE_RUNNER);
  assert.equal(route.hiddenRoadCount, 0);
});

test("a hire is a junction on the hiring road with a bridge to the new one", () => {
  const route = routeAtCompletion();
  const hireSigns = route.signs.filter((sign) => sign.kind === "hire");
  assert.deepEqual(
    hireSigns.map((sign) => sign.word),
    ["hire · Bob · Builder", "hire · Gordan · Runner"],
  );
  // The sign sits on the *hiring* road — the lead's — not the new seat's.
  assert.deepEqual(
    hireSigns.map((sign) => sign.road),
    ["exec-lead", "exec-lead"],
  );
  const hireBridges = route.bridges.filter((bridge) => bridge.kind === "hire");
  assert.deepEqual(
    hireBridges.map((bridge) => [bridge.from, bridge.to]),
    [
      ["exec-lead", "exec-builder"],
      ["exec-lead", "exec-runner"],
    ],
  );
});

test("a hire with no signed time draws no junction, and the road still exists", () => {
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    hires: hires().map((hire) => ({ ...hire, at: null })),
    nowMs: (COMPLETED + 60) * 1_000,
  });
  assert.equal(
    route.signs.filter((sign) => sign.kind === "hire").length,
    0,
    "an undated create must not be drawn at an invented moment",
  );
  assert.equal(route.roads.length, 4);
  // The road falls back to the seat's first signed moment, and labels it.
  assert.equal(route.roads[2].startedAt, HIRE_BOB);
  assert.equal(route.roads[2].startedAtSource, "first-signed");
});

test("every sign comes from a typed row, a badged delivery, a seat or a hire — never prose", () => {
  const route = routeAtQueuedWake();
  const inputs = missionInputs().filter(
    (input) => input.createdAt <= HIRE_RUNNER,
  );
  // 3 transactions + 2 dated hires + 1 badged delivery + 1 ungranted seat, and
  // nothing else can mint one: the function takes no narrative input at all.
  assert.equal(inputs.length, 3);
  assert.equal(route.signs.length, inputs.length + 2 + 1 + 1);
  const byKind = route.signs.reduce((counts, sign) => {
    counts[sign.kind] = (counts[sign.kind] ?? 0) + 1;
    return counts;
  }, {});
  assert.deepEqual(byKind, {
    assignment: 1,
    "mission.blocked": 1,
    report: 1,
    hire: 2,
    delivery: 1,
    "seat-ungranted": 1,
  });
  // `provider-started` carries no badge in the frozen copy table, so a wake
  // that started is not a sign: the gutter interrupts only for what is owed.
  const started = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: [],
    deliveries: [{ ...queuedDelivery(REPORT), kind: "provider-started" }],
    hires: [],
    nowMs: (HIRE_RUNNER + 60) * 1_000,
  });
  assert.equal(started.signs.length, 0);
});

test("a sign's word is the row's own frozen word, type for type", () => {
  const inputs = missionInputs();
  const rows = buildCodingSessionMissionTransactionRows({
    transactions: inputs,
    resolveActor: (pubkey) =>
      ACTORS.get(pubkey) ?? { label: null, executionKey: null },
    founderPubkey: FOUNDER,
    deliveries: [],
    density: "live",
  });
  const route = routeAtCompletion();
  for (const row of rows) {
    const sign = route.signs.find(
      (candidate) => candidate.sourceEventId === row.meta.sourceEventId,
    );
    assert.ok(sign, `no sign for ${row.type}`);
    // The row states its own word at the end of its accessible sentence; the
    // rail must print that word and not a second vocabulary for the gutter.
    const word = row.accessibleLabel.slice(
      row.accessibleLabel.lastIndexOf(": ") + 2,
    );
    assert.equal(sign.word, word, `word drift on ${row.type}`);
    assert.equal(sign.title, `${row.title} · ${row.meta.timeLabel}`);
    assert.equal(sign.revealKey, row.key);
  }
});

test("a queued wake is its own measured stretch, whatever its length", () => {
  const route = routeAtQueuedWake();
  const queued = route.stretches.filter((stretch) => stretch.kind === "queued");
  assert.equal(queued.length, 1);
  assert.equal(
    queued[0].road,
    "exec-lead",
    "the stretch is on the lead's road",
  );
  assert.equal(queued[0].durationMs, 260_000);
  assert.equal(queued[0].label, "4m 20s");
  // One stretch, one duration, even though the runner's hire landed four
  // minutes into the wait and split the span into two segments.
  assert.ok(queued[0].heightPx >= ROUTE_STRETCH_PX);
  assert.equal(queued[0].fromAt, REPORT);
  assert.equal(queued[0].toAt, REPORT + 260);
});

test("a silence longer than five minutes is a dashed stretch carrying its duration", () => {
  const route = routeAtCompletion();
  const silences = route.stretches.filter(
    (stretch) => stretch.kind === "silence",
  );
  assert.ok(silences.length >= 1);
  for (const silence of silences) {
    assert.ok(
      silence.toAt - silence.fromAt > 300,
      "a gap of five minutes or less must stay scaled distance",
    );
    assert.equal(
      silence.label,
      formatCodingSessionRouteDuration((silence.toAt - silence.fromAt) * 1_000),
    );
    assert.equal(silence.heightPx, ROUTE_STRETCH_PX);
  }
  // The 4 h 19 m the lead spent between Bob's acknowledgement and completing.
  assert.ok(silences.some((silence) => silence.label === "4h 19m"));
});

test("a gap of five minutes or less is scaled at 12 px per minute", () => {
  const first = at(10, 0);
  const second = at(10, 4);
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: [],
    transactions: routeTransactions([
      transactionInput({ sourceEventId: "aa".repeat(32), createdAt: first }),
      transactionInput({ sourceEventId: "bb".repeat(32), createdAt: second }),
    ]),
    nowMs: second * 1_000,
  });
  const [older, newer] = route.signs;
  assert.equal(newer.offsetPx - older.offsetPx, 4 * ROUTE_PX_PER_MINUTE);
  assert.equal(route.stretches.length, 0);
});

test("simultaneous signs stack in 20 px slots instead of overlapping", () => {
  const moment = at(10, 0);
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: [],
    transactions: routeTransactions([
      transactionInput({ sourceEventId: "aa".repeat(32), createdAt: moment }),
      transactionInput({ sourceEventId: "bb".repeat(32), createdAt: moment }),
      transactionInput({ sourceEventId: "cc".repeat(32), createdAt: moment }),
    ]),
    nowMs: (moment + 30) * 1_000,
  });
  assert.deepEqual(
    route.signs.map((sign) => sign.offsetPx),
    [0, ROUTE_SIGN_SLOT_PX, ROUTE_SIGN_SLOT_PX * 2],
  );
  // And the moment after them starts below the last slot.
  assert.ok(route.nowOffsetPx >= ROUTE_SIGN_SLOT_PX * 3);
});

test("more than 200 signs on one road collapse into a counted marker", () => {
  const extra = ROUTE_SIGNS_PER_ROAD_LIMIT + 14;
  const inputs = Array.from({ length: extra }, (_unused, index) =>
    transactionInput({
      sourceEventId: index.toString(16).padStart(64, "0"),
      createdAt: at(1, 0) + index,
      counterpartyPubkey: null,
    }),
  );
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(inputs),
    hires: [],
    nowMs: (at(1, 0) + extra) * 1_000,
  });
  const onLead = route.signs.filter((sign) => sign.road === "exec-lead");
  assert.equal(onLead.length, ROUTE_SIGNS_PER_ROAD_LIMIT);
  assert.equal(route.hiddenSignCount, 14);
  const lead = route.roads.find((road) => road.key === "exec-lead");
  assert.equal(lead.hiddenSignCount, 14);
  // The signs kept are the newest, not the first 200 the list happened to hold.
  assert.equal(onLead[onLead.length - 1].at, at(1, 0) + extra - 1);
});

test("a sign nobody can attribute goes off-road, never into the founder's lane", () => {
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions([
      transactionInput({
        sourceEventId: "ee".repeat(32),
        authorPubkey: STRANGER,
        counterpartyPubkey: null,
        createdAt: ASSIGNMENT,
      }),
    ]),
    hires: [],
    nowMs: (ACK + 60) * 1_000,
  });
  assert.equal(route.signs.length, 1);
  assert.equal(route.signs[0].road, null);
  assert.notEqual(route.signs[0].road, CODING_SESSION_ROUTE_FOUNDER_ROAD);
});

test("a road head says what its participant is holding, and stops when it is answered", () => {
  const open = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(
      missionInputs().filter((input) => input.createdAt <= REPORT),
    ),
    seatAuthorities: [UNGRANTED_RUNNER],
    hires: hires(),
    nowMs: (REPORT + 420) * 1_000,
  });
  const bobOpen = open.roads.find((road) => road.key === "exec-builder");
  assert.equal(bobOpen.head.holding, "report");
  assert.equal(bobOpen.head.sinceAt, REPORT);
  assert.equal(bobOpen.head.sinceMs, 420_000);

  const ruled = routeAtCompletion();
  const bobRuled = ruled.roads.find((road) => road.key === "exec-builder");
  assert.equal(bobRuled.head.holding, null, "a ruled report is not still held");
  assert.equal(bobRuled.head.sinceMs, null);

  const runner = ruled.roads.find((road) => road.key === "exec-runner");
  assert.equal(runner.head.seatAuthorityDetail, "Seat created, not granted");
  assert.match(runner.head.seatAuthorityRemedy, /^bee sessions seat-repair /);
});

test("the terminal conflict shows as two signs, the completion last", () => {
  const route = routeAtCompletion();
  const terminals = route.signs.filter((sign) =>
    sign.kind.startsWith("mission."),
  );
  assert.deepEqual(
    terminals.map((sign) => sign.kind),
    ["mission.blocked", "mission.completed"],
  );
  // The blocked terminal is a full-weight blocker and the completion is not.
  assert.equal(terminals[0].weight, "attention");
  assert.equal(terminals[0].tone, "critical");
  assert.equal(terminals[0].requiresDecision, true);
  assert.equal(terminals[1].weight, "standard");
  // The completion is the last thing anyone *did*. The ungranted-seat sign
  // sorts after it because a standing condition marks the road at Now, which
  // is later than every signed act.
  const acts = route.signs.filter((sign) => sign.kind !== "seat-ungranted");
  assert.equal(acts[acts.length - 1].kind, "mission.completed");
});

test("the You-are-here band follows the visible rows and names the distance to Now", () => {
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    hires: hires(),
    visibleAt: [ASSIGNMENT, REPORT],
    nowMs: (REPORT + 420) * 1_000,
  });
  assert.equal(route.here.fromAt, ASSIGNMENT);
  assert.equal(route.here.toAt, REPORT);
  assert.equal(route.here.toNowMs, 420_000);
  assert.equal(route.here.toNowLabel, "7m to Now");
  assert.ok(route.here.heightPx >= ROUTE_BAND_MIN_PX);
});

test("a band over one row is still 24 px, and no visible row is no band", () => {
  const single = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    hires: hires(),
    visibleAt: [REPORT],
    nowMs: (REPORT + 30) * 1_000,
  });
  assert.equal(single.here.heightPx, ROUTE_BAND_MIN_PX);
  // Under a second there is no distance to print, and `0s` is never printed.
  assert.equal(formatCodingSessionRouteDuration(400), null);
  const none = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    hires: hires(),
    nowMs: (REPORT + 30) * 1_000,
  });
  assert.equal(none.here, null);
});

test("durations read Nm Ss under an hour and Nh Nm above it", () => {
  assert.equal(formatCodingSessionRouteDuration(260_000), "4m 20s");
  assert.equal(formatCodingSessionRouteDuration(420_000), "7m");
  assert.equal(formatCodingSessionRouteDuration(45_000), "45s");
  assert.equal(formatCodingSessionRouteDuration(16_620_000), "4h 37m");
  assert.equal(formatCodingSessionRouteDuration(7_200_000), "2h");
  assert.equal(formatCodingSessionRouteDuration(0), null);
  assert.equal(formatCodingSessionRouteDuration(Number.NaN), null);
});

test("the rail folds only when the stream would drop under its floor", () => {
  // L4.6: the body-width and reading-reserve gates are gone. The one question
  // is the stream's own floor, because the viewer now owns the rail's width
  // and its collapse.
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 224,
      sectionWidthPx: 1_279,
    }),
    true,
    "a wide body no longer has to clear 1280 for the rail to be allowed",
  );
  // 970 px of section: expanding costs 224 - 40 = 184, leaving 786. Fits.
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 224,
      sectionWidthPx: 970,
    }),
    true,
  );
  // 603 px of section: expanding leaves 419 — one px under the 420 floor.
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 224,
      sectionWidthPx: 603,
    }),
    false,
  );
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 224,
      sectionWidthPx: 604,
    }),
    true,
    "420 px of stream is the floor, not 421",
  );
  // A rail the viewer dragged to 480 costs 440 to expand, so the same section
  // that fits a 224 px rail refuses a 480 px one.
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 480,
      sectionWidthPx: 800,
    }),
    false,
  );
  // Once shown, the section already has the rail subtracted, so the decision
  // is asked about the same layout either way and does not oscillate on the
  // exact pixel where it flips.
  assert.equal(
    codingSessionRouteFits({
      railShown: true,
      railWidthPx: 224,
      sectionWidthPx: 420,
    }),
    true,
  );
  assert.equal(
    codingSessionRouteFits({
      railShown: true,
      railWidthPx: 224,
      sectionWidthPx: 419,
    }),
    false,
  );
  // Nothing measured yet is not "it fits".
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: 224,
      sectionWidthPx: 0,
    }),
    false,
  );
});

test("the scrubber keeps only the signs a 40 px track owes the reader", () => {
  const route = routeAtQueuedWake();
  const attention = codingSessionRouteAttentionSigns(route);
  assert.deepEqual(attention.map((sign) => sign.kind).sort(), [
    "mission.blocked",
    "seat-ungranted",
  ]);
});

// ---------------------------------------------------------------------------
// Fix round 1 (REVIEW-A4)
// ---------------------------------------------------------------------------

test("F2: a queued wake longer than five minutes is bounded, and still says how long", () => {
  const start = at(3, 0);
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions([
      transactionInput({
        sourceEventId: REPORT_ID,
        type: "report",
        authorPubkey: BUILDER_ACTOR,
        counterpartyPubkey: LEAD_ACTOR,
        createdAt: start,
      }),
    ]),
    deliveries: [queuedDelivery(start)],
    hires: [],
    // 1 h 30 m still queued — the §2a residual's shape, and the input that is
    // unbounded by construction because it runs to Now.
    nowMs: (start + 5_400) * 1_000,
  });
  const queued = route.stretches.find((stretch) => stretch.kind === "queued");
  assert.equal(queued.label, "1h 30m", "the measurement is the fact");
  assert.equal(queued.durationMs, 5_400_000);
  assert.equal(
    queued.heightPx,
    ROUTE_STRETCH_PX,
    "a long wake is drawn at the silence height, not scaled",
  );
  assert.ok(
    route.heightPx < 200,
    `the whole rail must stay readable; got ${route.heightPx}px`,
  );
  // A short wake is still its own measured stretch at the same 48 px floor.
  const short = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: [],
    deliveries: [queuedDelivery(start)],
    hires: [],
    nowMs: (start + 60) * 1_000,
  });
  const shortQueued = short.stretches.find(
    (stretch) => stretch.kind === "queued",
  );
  assert.equal(shortQueued.label, "1m");
  assert.equal(shortQueued.heightPx, ROUTE_STRETCH_PX);
});

test("F7: a delivery sign says it sits on a local clock, and an undated one is counted", () => {
  const route = routeAtQueuedWake();
  const delivery = route.signs.find((sign) => sign.kind === "delivery");
  assert.equal(delivery.timeSource, "local");
  for (const sign of route.signs) {
    if (sign.kind === "delivery") continue;
    assert.equal(sign.timeSource, "signed", `${sign.kind} is not signed time`);
  }

  const undated = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: [],
    deliveries: [{ ...queuedDelivery(REPORT), observedAtMs: null }],
    hires: [],
    nowMs: (REPORT + 600) * 1_000,
  });
  assert.equal(
    undated.signs.length,
    0,
    "nothing may be placed at an invented moment",
  );
  assert.equal(undated.undatedSignCount, 1, "and nothing may vanish either");
  const lead = undated.roads.find((road) => road.key === "exec-lead");
  assert.equal(lead.undatedSignCount, 1);
});

test("F8: a hidden sign carries no bridge", () => {
  const extra = ROUTE_SIGNS_PER_ROAD_LIMIT + 14;
  const inputs = Array.from({ length: extra }, (_unused, index) =>
    transactionInput({
      sourceEventId: index.toString(16).padStart(64, "0"),
      createdAt: at(1, 0) + index,
    }),
  );
  const route = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(inputs),
    hires: [],
    nowMs: (at(1, 0) + extra) * 1_000,
  });
  assert.equal(route.signs.length, ROUTE_SIGNS_PER_ROAD_LIMIT);
  assert.equal(route.hiddenSignCount, 14);
  assert.ok(
    route.bridges.length <= ROUTE_SIGNS_PER_ROAD_LIMIT,
    `bridges outran signs: ${route.bridges.length}`,
  );
  const signKeys = new Set(route.signs.map((sign) => sign.key));
  for (const bridge of route.bridges) {
    assert.ok(
      signKeys.has(bridge.ownerSignKey),
      `bridge ${bridge.key} belongs to a sign the rail says is not shown`,
    );
  }
});

test("F10: clicking a road's `+N earlier` lifts that road's bound, and only that road's", () => {
  const extra = ROUTE_SIGNS_PER_ROAD_LIMIT + 14;
  const inputs = Array.from({ length: extra }, (_unused, index) =>
    transactionInput({
      sourceEventId: index.toString(16).padStart(64, "0"),
      createdAt: at(1, 0) + index,
      counterpartyPubkey: null,
    }),
  );
  const base = {
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(inputs),
    hires: [],
    nowMs: (at(1, 0) + extra) * 1_000,
  };
  const folded = deriveCodingSessionRoute(base);
  assert.equal(folded.hiddenSignCount, 14);
  assert.equal(
    folded.roads.find((road) => road.key === "exec-lead").expanded,
    false,
  );
  const opened = deriveCodingSessionRoute({
    ...base,
    expandedRoads: ["exec-lead"],
  });
  assert.equal(opened.signs.length, extra);
  assert.equal(opened.hiddenSignCount, 0);
  assert.equal(
    opened.roads.find((road) => road.key === "exec-lead").expanded,
    true,
  );
  // Lifted, never removed: the expanded ceiling is a bound of its own.
  assert.ok(ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT > ROUTE_SIGNS_PER_ROAD_LIMIT);
  const huge = Array.from(
    { length: ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT + 3 },
    (_unused, index) =>
      transactionInput({
        sourceEventId: index.toString(16).padStart(64, "0"),
        createdAt: at(1, 0) + index,
        counterpartyPubkey: null,
      }),
  );
  const stillBounded = deriveCodingSessionRoute({
    ...base,
    transactions: routeTransactions(huge),
    expandedRoads: ["exec-lead"],
    nowMs: (at(1, 0) + huge.length) * 1_000,
  });
  assert.equal(stillBounded.hiddenSignCount, 3);
});

test("F1: a road says whether its start is a create or a first signed sign", () => {
  const dated = routeAtCompletion();
  const bob = dated.roads.find((road) => road.key === "exec-builder");
  assert.equal(bob.startedAtSource, "hire");
  assert.equal(bob.startedAt, HIRE_BOB);

  const undated = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: routeTransactions(missionInputs()),
    hires: hires().map((hire) => ({ ...hire, at: null })),
    nowMs: (COMPLETED + 60) * 1_000,
  });
  const bobUndated = undated.roads.find((road) => road.key === "exec-builder");
  assert.equal(bobUndated.startedAtSource, "first-signed");
  assert.notEqual(
    bobUndated.startedAtSource,
    dated.roads.find((road) => road.key === "exec-builder").startedAtSource,
    "the two starts must be distinguishable, not both drawn as a hire",
  );
});
