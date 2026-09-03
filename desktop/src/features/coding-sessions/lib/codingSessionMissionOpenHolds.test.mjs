import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_NO_OPEN_HOLDS,
  CODING_SESSION_OPEN_HOLDS_LIMIT,
  codingSessionOpenHoldStatusLine,
  codingSessionOpenHoldWaitLine,
  codingSessionOpenHoldsTruncation,
  deriveCodingSessionMissionOpenHolds,
} from "./codingSessionMissionOpenHolds.ts";

const KEYSTONE = "11".repeat(32);
const IRA = "22".repeat(32);
const GHOST = "33".repeat(32);

/** 12:33:00 local, so the rendered clock is the fixture's own wall time. */
const SIGNED_AT_MS = new Date(2026, 8, 2, 12, 33, 0).getTime();
/** 12:36:03 local — 3m 3s after the assignment was signed. */
const NOW_MS = SIGNED_AT_MS + 183_000;

function participant(overrides = {}) {
  return {
    executionKey: "exec-ira",
    label: "Ira · Verifier",
    actorPubkey: IRA,
    role: "verifier",
    targetKey: null,
    word: "idle",
    live: false,
    firstSignedAt: Math.floor(SIGNED_AT_MS / 1_000),
    releasedAt: null,
    ...overrides,
  };
}

function party(pubkey, label) {
  return { pubkey, label, monogram: label[0], accent: "accent-1" };
}

function row(overrides = {}) {
  const base = {
    key: "transaction:assign-1",
    type: "assignment",
    weight: "standard",
    tone: null,
    actor: party(KEYSTONE, "Keystone"),
    counterparty: party(IRA, "Ira · Verifier"),
    title: "Keystone assigned Ira · Verifier",
    body: "Check the fold.",
    meta: { timeLabel: "12:33 PM", sourceEventId: "assign-1" },
    accessibleLabel: "Keystone to Ira · Verifier: assignment",
    decision: null,
    requiredAction: null,
    delivery: null,
    unseated: false,
    fileCount: null,
    testCount: null,
    createdAt: Math.floor(SIGNED_AT_MS / 1_000),
    showSignedSource: false,
    parentEventId: null,
  };
  return { ...base, ...overrides };
}

test("an assignment nobody reported on is one open hold, named on both sides", () => {
  const { holds } = deriveCodingSessionMissionOpenHolds({
    participants: [participant()],
    transactions: [row()],
    founderPubkey: null,
    nowMs: NOW_MS,
  });

  assert.equal(holds.length, 1);
  assert.deepEqual(holds[0], {
    waiterLabel: "Keystone",
    holderLabel: "Ira · Verifier",
    holding: "assignment",
    sinceMs: 183_000,
    sinceAt: Math.floor(SIGNED_AT_MS / 1_000),
    sourceEventId: "assign-1",
  });
  assert.equal(
    codingSessionOpenHoldWaitLine(holds[0]),
    "Keystone waits on Ira · Verifier",
  );
  assert.equal(
    codingSessionOpenHoldStatusLine(holds[0]),
    "Assignment open 3m 3s · since 12:33 PM · no report yet",
  );
});

test("a report a verdict already answered is not an open hold", () => {
  const { holds } = deriveCodingSessionMissionOpenHolds({
    participants: [participant()],
    transactions: [
      row({
        key: "transaction:report-1",
        type: "report",
        actor: party(IRA, "Ira · Verifier"),
        counterparty: party(KEYSTONE, "Keystone"),
        meta: { timeLabel: "12:33 PM", sourceEventId: "report-1" },
      }),
      row({
        key: "transaction:verdict-1",
        type: "disposition",
        actor: party(KEYSTONE, "Keystone"),
        counterparty: party(IRA, "Ira · Verifier"),
        meta: { timeLabel: "12:35 PM", sourceEventId: "verdict-1" },
        parentEventId: "report-1",
      }),
    ],
    founderPubkey: null,
    nowMs: NOW_MS,
  });

  assert.deepEqual(holds, []);
});

test("a report awaiting a verdict reads with the verdict tail", () => {
  const { holds } = deriveCodingSessionMissionOpenHolds({
    participants: [participant({ executionKey: "exec-lead" })],
    transactions: [
      row({
        key: "transaction:report-1",
        type: "report",
        actor: party(KEYSTONE, "Keystone"),
        counterparty: party(IRA, "Ira · Verifier"),
        meta: { timeLabel: "12:33 PM", sourceEventId: "report-1" },
      }),
    ],
    founderPubkey: null,
    nowMs: NOW_MS,
  });

  assert.equal(holds.length, 1);
  assert.equal(holds[0].holding, "report");
  assert.equal(
    codingSessionOpenHoldStatusLine(holds[0]),
    "Report open 3m 3s · since 12:33 PM · no verdict yet",
  );
});

test("an assignee that resolves to no seat holds with a null holder", () => {
  const { holds } = deriveCodingSessionMissionOpenHolds({
    participants: [participant()],
    transactions: [
      row({
        counterparty: party(GHOST, "unknown actor"),
      }),
    ],
    founderPubkey: null,
    nowMs: NOW_MS,
  });

  assert.equal(holds.length, 1);
  assert.equal(holds[0].holderLabel, null);
  assert.equal(holds[0].waiterLabel, "Keystone");
  assert.equal(
    codingSessionOpenHoldWaitLine(holds[0]),
    "Keystone waits on holder not resolved",
  );
});

test("a hold nothing dates prints no duration and never 0s", () => {
  const detail = codingSessionOpenHoldStatusLine({
    holding: "assignment",
    sinceMs: null,
    sinceAt: null,
  });

  assert.equal(detail, "Assignment open · no report yet");
  assert.ok(!detail.includes("0s"));
});

test("a dated hold with no clock keeps the duration and drops the since clause", () => {
  assert.equal(
    codingSessionOpenHoldStatusLine({
      holding: "assignment",
      sinceMs: 183_000,
      sinceAt: null,
    }),
    "Assignment open 3m 3s · no report yet",
  );
});

test("a road head holding nothing gets no status line at all", () => {
  assert.equal(
    codingSessionOpenHoldStatusLine({
      holding: null,
      sinceMs: null,
      sinceAt: null,
    }),
    null,
  );
});

test("a hold whose waiter nothing resolves still names the holder", () => {
  assert.equal(
    codingSessionOpenHoldWaitLine({
      waiterLabel: null,
      holderLabel: "Ira · Verifier",
      holding: "assignment",
      sinceMs: null,
      sinceAt: null,
      sourceEventId: null,
    }),
    "Waiting on Ira · Verifier",
  );
});

test("the empty state is one sentence, not an absence", () => {
  assert.equal(CODING_SESSION_NO_OPEN_HOLDS, "No open assignments.");
  assert.deepEqual(
    deriveCodingSessionMissionOpenHolds({
      participants: [],
      transactions: [],
      founderPubkey: null,
      nowMs: NOW_MS,
    }),
    { holds: [], omitted: 0 },
  );
});

test("F2: a report held on the founder names them, and the seat is the waiter", () => {
  // The commonest hold in any mission: a seat filed a report, the founder owes
  // the verdict. The founder holds no seat, and resolving the counterparty
  // against the seat roster alone printed `holder not resolved` over the
  // person actually reading the screen (REVIEW-L4 F2).
  const { holds } = deriveCodingSessionMissionOpenHolds({
    participants: [participant({ actorPubkey: IRA, label: "Bob · Builder" })],
    transactions: [
      row({
        key: "transaction:report-1",
        type: "report",
        actor: party(IRA, "Bob · Builder"),
        counterparty: party(KEYSTONE, "Founder"),
        parentEventId: "assign-1",
        meta: { timeLabel: "12:33 PM", sourceEventId: "report-1" },
        createdAt: Math.floor(SIGNED_AT_MS / 1_000),
      }),
    ],
    founderPubkey: KEYSTONE,
    founderLabel: "you",
    nowMs: SIGNED_AT_MS + 192_000,
  });
  assert.equal(holds.length, 1);
  assert.equal(holds[0].waiterLabel, "Bob · Builder");
  assert.equal(holds[0].holderLabel, "you");
  assert.equal(
    codingSessionOpenHoldWaitLine(holds[0]),
    "Bob · Builder waits on you",
  );
  assert.match(
    codingSessionOpenHoldStatusLine(holds[0]),
    /^Report open 3m 12s · since .* · no verdict yet$/,
  );
});

test("F9/I10: the open-hold list is capped and says how many it dropped", () => {
  const transactions = Array.from({ length: 62 }, (_, index) =>
    row({
      key: `transaction:assign-${index}`,
      meta: { timeLabel: "12:33 PM", sourceEventId: `assign-${index}` },
      createdAt: Math.floor(SIGNED_AT_MS / 1_000) + index,
    }),
  );
  const result = deriveCodingSessionMissionOpenHolds({
    participants: [participant()],
    transactions,
    founderPubkey: null,
    nowMs: NOW_MS,
  });
  assert.equal(result.holds.length, CODING_SESSION_OPEN_HOLDS_LIMIT);
  assert.equal(result.omitted, 12);
  // Oldest first, so the cap drops the newest — the ones a person watching has
  // had the least time to worry about.
  assert.equal(result.holds[0].sourceEventId, "assign-0");
  assert.equal(
    codingSessionOpenHoldsTruncation(result.omitted),
    "Showing the 50 oldest open handoffs · 12 more not shown.",
  );
  assert.equal(codingSessionOpenHoldsTruncation(0), null);
});
