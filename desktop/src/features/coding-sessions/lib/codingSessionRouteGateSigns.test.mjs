import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionRouteAttentionSigns,
  CODING_SESSION_ROUTE_FOUNDER_ROAD,
  deriveCodingSessionRoute,
  ROUTE_SIGNS_PER_ROAD_LIMIT,
} from "./codingSessionRouteModel.ts";

// L5.6. `codingSessionRouteModel.ts:11-16` allows a sign only for a row the
// stream itself renders — prose never becomes a sign — and a 44246 gate row is
// a signed event two surfaces render, so it qualifies. Checkpoints, findings
// and phase timings get none: they would flood the gutter.

const DAY = 1_788_400_000;
const at = (hours, minutes) => DAY + hours * 3_600 + minutes * 60;

const FOUNDER = "f0".repeat(32);
const BUILDER_ACTOR = "2b".repeat(32);
const PROVIDER = "9e".repeat(32);

function participants() {
  return [
    {
      executionKey: "exec-builder",
      label: "Bob · Builder",
      actorPubkey: BUILDER_ACTOR,
      role: "builder",
      targetKey: "target:builder",
      word: "live",
      live: true,
      firstSignedAt: at(21, 30),
      releasedAt: null,
    },
  ];
}

function gateRow(overrides = {}) {
  return {
    key: `${BUILDER_ACTOR}:declared:cargo test`,
    authorPubkey: BUILDER_ACTOR,
    gate: "cargo test",
    outcome: "passed",
    at: at(21, 45),
    sourceEventId: "a1".repeat(32),
    ...overrides,
  };
}

function route(gateRows) {
  return deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: participants(),
    transactions: [],
    gateRows,
    nowMs: at(22, 0) * 1_000,
  });
}

test("a passed and a failed row give two signs in the fold's own words", () => {
  const signs = route([
    gateRow(),
    gateRow({
      key: `${BUILDER_ACTOR}:declared:cargo clippy`,
      gate: "cargo clippy",
      outcome: "failed",
      at: at(21, 50),
      sourceEventId: "b2".repeat(32),
    }),
  ]).signs.filter((sign) => sign.kind === "gate");

  assert.equal(signs.length, 2);
  // The row's own outcome word, never a second vocabulary for the same fact.
  assert.deepEqual(
    signs.map((sign) => sign.word),
    ["passed", "failed"],
  );
  assert.deepEqual(
    signs.map((sign) => sign.road),
    ["exec-builder", "exec-builder"],
  );
  const failed = signs.find((sign) => sign.word === "failed");
  assert.equal(failed.weight, "attention");
  assert.equal(failed.tone, "critical");
  assert.match(failed.title, /cargo clippy · failed · /);
  assert.equal(failed.sourceEventId, "b2".repeat(32));
  // Nothing renders these in the stream, so there is no row to reveal — and
  // `null` is what the delivery and seat-authority signs already say.
  assert.equal(failed.revealKey, null);

  const passed = signs.find((sign) => sign.word === "passed");
  assert.equal(passed.weight, "standard");
  assert.equal(passed.tone, null);
});

test("a checkpoint yields no sign — nothing but a gate row reaches the rail", () => {
  // The route takes gate rows and nothing else from kind 44246: there is no
  // input here a checkpoint, a finding or a phase timing could arrive through.
  const signs = route([]).signs;
  assert.deepEqual(
    signs.filter((sign) => sign.kind === "gate"),
    [],
  );
});

test("a row with no signed time draws no sign, rather than an invented one", () => {
  const signs = route([gateRow({ at: null })]).signs.filter(
    (sign) => sign.kind === "gate",
  );
  assert.deepEqual(signs, []);
});

test("a row signed by nobody on the map is off-road, never the founder's", () => {
  // Folding an unattributable sign into the founder's lane would attribute a
  // stranger's signed act to the person reading — the worst lie this map could
  // tell (R2's own comment).
  const [sign] = route([
    gateRow({ authorPubkey: PROVIDER, key: `${PROVIDER}:observed:cargo test` }),
  ]).signs.filter((entry) => entry.kind === "gate");
  assert.equal(sign.road, null);
  assert.notEqual(sign.road, CODING_SESSION_ROUTE_FOUNDER_ROAD);
});

test("a full road truncates visibly, gate rows included", () => {
  const rows = Array.from(
    { length: ROUTE_SIGNS_PER_ROAD_LIMIT + 4 },
    (_unused, index) =>
      gateRow({
        key: `${BUILDER_ACTOR}:declared:gate-${index}`,
        gate: `gate-${index}`,
        at: at(21, 30) + index,
        sourceEventId: index.toString(16).padStart(64, "0"),
      }),
  );
  const map = route(rows);
  const road = map.roads.find((entry) => entry.key === "exec-builder");
  assert.equal(road.hiddenSignCount, 4);
  assert.equal(map.hiddenSignCount, 4);
  assert.equal(
    map.signs.filter((sign) => sign.road === "exec-builder").length,
    ROUTE_SIGNS_PER_ROAD_LIMIT,
  );
});

test("a failed gate survives the fold to the scrubber; a passed one does not", () => {
  const map = route([
    gateRow(),
    gateRow({
      key: `${BUILDER_ACTOR}:declared:cargo clippy`,
      gate: "cargo clippy",
      outcome: "failed",
      at: at(21, 50),
      sourceEventId: "b2".repeat(32),
    }),
  ]);
  const attention = codingSessionRouteAttentionSigns(map);
  assert.deepEqual(
    attention.map((sign) => sign.word),
    ["failed"],
  );
});

test("a not-run row is never a pass, on the rail either", () => {
  const [sign] = route([gateRow({ outcome: "not-run" })]).signs.filter(
    (entry) => entry.kind === "gate",
  );
  assert.equal(sign.word, "not-run");
  assert.equal(sign.weight, "standard");
  assert.equal(
    codingSessionRouteAttentionSigns(route([gateRow({ outcome: "not-run" })]))
      .length,
    0,
  );
});
