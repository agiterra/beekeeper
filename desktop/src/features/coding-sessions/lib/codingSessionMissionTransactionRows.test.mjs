import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionMissionTransactionRows } from "./codingSessionMissionTransactionRows.ts";

const FOUNDER = "f".repeat(64);
const BUILDER = "b".repeat(64);
const LEAD = "1".repeat(64);

const NAMES = new Map([
  [FOUNDER, { label: "Keystone", executionKey: "lead" }],
  [BUILDER, { label: "Bob", executionKey: "builder" }],
]);

function resolveActor(pubkey) {
  return NAMES.get(pubkey) ?? { label: null, executionKey: null };
}

function transaction(overrides) {
  return {
    sourceEventId: "e".repeat(64),
    type: "report",
    authorPubkey: BUILDER,
    createdAt: 1_800_000_000,
    counterpartyPubkey: FOUNDER,
    parentEventId: null,
    summary: "Dispatch extraction",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
    ...overrides,
  };
}

function build(transactions, extra = {}) {
  return buildCodingSessionMissionTransactionRows({
    transactions,
    resolveActor,
    founderPubkey: FOUNDER,
    density: "live",
    ...extra,
  });
}

test("U-T1: every transaction type renders its frozen title", () => {
  const rows = build([
    transaction({
      type: "assignment",
      authorPubkey: FOUNDER,
      counterpartyPubkey: BUILDER,
      sourceEventId: "a1",
    }),
    transaction({ type: "report", sourceEventId: "a2" }),
    transaction({ type: "refutation", sourceEventId: "a3" }),
    transaction({
      type: "disposition",
      authorPubkey: FOUNDER,
      counterpartyPubkey: BUILDER,
      decision: "approved",
      sourceEventId: "a4",
    }),
    transaction({ type: "acknowledgement", sourceEventId: "a5" }),
    transaction({
      type: "mission.completed",
      authorPubkey: FOUNDER,
      counterpartyPubkey: null,
      sourceEventId: "a6",
    }),
    transaction({
      type: "mission.blocked",
      authorPubkey: FOUNDER,
      counterpartyPubkey: null,
      sourceEventId: "a7",
    }),
    // B1c's three verbs. Before they were listed, the title chain's final
    // `else` gave each of them a verdict's words.
    transaction({
      type: "note",
      counterpartyPubkey: null,
      sourceEventId: "a8",
    }),
    transaction({ type: "decision.request", sourceEventId: "a9" }),
    transaction({
      type: "decision.answer",
      authorPubkey: FOUNDER,
      counterpartyPubkey: BUILDER,
      sourceEventId: "a10",
    }),
  ]);
  assert.deepEqual(
    rows.map((row) => row.title),
    [
      "Keystone → Bob · Assignment",
      "Bob → Keystone · Report",
      "Bob → Keystone · Refutation",
      "Keystone → Bob · Verdict: approved",
      "Bob → Keystone · Acknowledgement",
      "Mission completed · Keystone",
      "Mission blocked · Keystone",
      "Bob · Note",
      "Bob → Keystone · Ruling asked",
      "Keystone → Bob · Ruling given",
    ],
  );
  // Every row stays standard weight: an open ruling is a fold fact, and this
  // builder cannot see whether the answer has landed.
  assert.deepEqual(
    rows.slice(-3).map((row) => [row.weight, row.tone]),
    [
      ["standard", null],
      ["standard", null],
      ["standard", null],
    ],
  );
  assert.deepEqual(
    rows.slice(-3).map((row) => row.accessibleLabel),
    [
      "Bob: note",
      "Bob to Keystone: ruling asked",
      "Keystone to Bob: ruling given",
    ],
  );
});

test("U-T1: the accessible sentence names both parties and the type", () => {
  const [assignment, terminal] = build([
    transaction({
      type: "assignment",
      authorPubkey: FOUNDER,
      counterpartyPubkey: BUILDER,
      sourceEventId: "a1",
    }),
    transaction({
      type: "mission.blocked",
      authorPubkey: FOUNDER,
      counterpartyPubkey: null,
      sourceEventId: "a2",
    }),
  ]);
  assert.equal(assignment.accessibleLabel, "Keystone to Bob: assignment");
  assert.equal(terminal.accessibleLabel, "Keystone: mission blocked");
});

test("U-T1: blocked, refutation, a required action and a failed delivery are attention", () => {
  const rows = build(
    [
      transaction({ type: "report", sourceEventId: "plain" }),
      transaction({ type: "refutation", sourceEventId: "refute" }),
      transaction({
        type: "mission.blocked",
        counterpartyPubkey: null,
        sourceEventId: "blocked",
      }),
      transaction({
        type: "disposition",
        decision: "changes-requested",
        requiredAction: "Publish the follow-up note.",
        sourceEventId: "action",
      }),
      transaction({ type: "report", sourceEventId: "dropped" }),
    ],
    {
      deliveries: [
        {
          sourceEventId: "dropped",
          operationType: "report",
          sourceActorPubkey: BUILDER,
          leadTargetKey: "lead-target",
          kind: "failed",
          owningCommandId: null,
          duplicateRefusedCommandIds: [],
          failures: [],
          reArmCount: 1,
          observedAtMs: 10,
          detail: "Wake delivery failed",
        },
      ],
    },
  );
  assert.deepEqual(
    rows.map((row) => `${row.type}:${row.weight}:${row.tone ?? "none"}`),
    [
      "report:standard:none",
      "refutation:attention:caution",
      "mission.blocked:attention:critical",
      "disposition:attention:caution",
      "report:attention:critical",
    ],
  );
  assert.equal(rows[4].delivery?.kind, "failed");
});

test("U-T1: an unresolvable author is named, never printed as a key", () => {
  const [row] = build([
    transaction({ authorPubkey: LEAD, counterpartyPubkey: null }),
  ]);
  assert.equal(row.actor.label, "unknown actor");
  assert.doesNotMatch(row.title, new RegExp(LEAD));
  assert.equal(row.actor.monogram, "U");
});

test("U-T1: the founder reads as You when no profile resolves the key", () => {
  const [row] = build(
    [
      transaction({
        authorPubkey: FOUNDER,
        counterpartyPubkey: null,
        type: "mission.completed",
      }),
    ],
    {
      resolveActor: () => ({ label: null, executionKey: null }),
    },
  );
  assert.equal(row.actor.label, "You");
  assert.equal(row.title, "Mission completed · You");
});

test("U-T1: Trace alone marks the row for signed-source disclosure", () => {
  assert.equal(
    build([transaction({})], { density: "trace" })[0].showSignedSource,
    true,
  );
  assert.equal(
    build([transaction({})], { density: "live" })[0].showSignedSource,
    false,
  );
  assert.equal(
    build([transaction({})], { density: "brief" })[0].showSignedSource,
    false,
  );
});

test("U-T1: Brief keeps every transaction row", () => {
  const transactions = ["t1", "t2", "t3"].map((id) =>
    transaction({ sourceEventId: id }),
  );
  assert.equal(build(transactions, { density: "brief" }).length, 3);
});
