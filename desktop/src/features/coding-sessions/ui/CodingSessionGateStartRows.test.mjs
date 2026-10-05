import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionGateRunState,
  describeCodingSessionGateRun,
  describeCodingSessionGateRunTitle,
  isCodingSessionGateRunClosed,
} from "./CodingSessionGateStartRows.tsx";

const STALE_AFTER_MS = 30 * 60_000;
const STARTED = Date.UTC(2026, 9, 4, 14, 2);
const clock = (ms) =>
  `${new Date(ms).getUTCHours()}:${String(new Date(ms).getUTCMinutes()).padStart(2, "0")}`;

function start(overrides = {}) {
  return {
    eventId: "5a".repeat(32),
    authorPubkey: "11".repeat(32),
    gate: "cargo test",
    startedAtMs: STARTED,
    assignmentRef: null,
    closeEventId: null,
    endedAtMs: null,
    durationMs: null,
    ...overrides,
  };
}

test("a measured close is ended, with its span", () => {
  const run = start({
    closeEventId: "5b".repeat(32),
    endedAtMs: STARTED + 180_000,
    durationMs: 180_000,
  });
  const state = codingSessionGateRunState(run, STALE_AFTER_MS, STARTED);
  assert.equal(state, "ended");
  assert.equal(isCodingSessionGateRunClosed(state), true);
  assert.equal(
    describeCodingSessionGateRun(run, state, clock),
    "cargo test · started 14:02 · ended 14:05 (3m)",
  );
  assert.match(
    describeCodingSessionGateRunTitle(state, STALE_AFTER_MS, true),
    /the gate row above/,
  );
  assert.doesNotMatch(
    describeCodingSessionGateRunTitle(state, STALE_AFTER_MS, false),
    /gate row above/,
  );
});

test("a durationless close is the provider no longer watching, never an end", () => {
  // e.g. the turn was interrupted 40s into `cargo test`: end_turn signs a
  // close with durationMs null, and the command may still be running.
  const run = start({
    closeEventId: "5b".repeat(32),
    endedAtMs: STARTED + 60_000,
    durationMs: null,
  });
  const state = codingSessionGateRunState(run, STALE_AFTER_MS, STARTED);
  assert.equal(state, "unwatched");
  assert.equal(isCodingSessionGateRunClosed(state), true);
  const line = describeCodingSessionGateRun(run, state, clock);
  assert.equal(
    line,
    "cargo test · started 14:02 · stopped watching 14:03 · end not observed",
  );
  assert.doesNotMatch(line, /ended/);
  const title = describeCodingSessionGateRunTitle(state, STALE_AFTER_MS, false);
  assert.match(title, /stopped watching/);
  assert.match(title, /turn ended.*32-call window.*exited/);
  assert.match(title, /may still have been running/);
  assert.match(title, /No span was measured/);
  // No gate row in the block: the title points at none.
  assert.doesNotMatch(title, /gate row/);
  assert.match(
    describeCodingSessionGateRunTitle(state, STALE_AFTER_MS, true),
    /its result is a gate row above/,
  );
});

test("a close clamped to its start says the provider's clock went backwards", () => {
  const run = start({
    closeEventId: "5b".repeat(32),
    endedAtMs: STARTED,
    durationMs: null,
  });
  const state = codingSessionGateRunState(run, STALE_AFTER_MS, STARTED);
  assert.equal(state, "clock-backwards");
  assert.equal(
    describeCodingSessionGateRun(run, state, clock),
    "cargo test · started 14:02 · end not observed (the provider's clock went backwards)",
  );
  const title = describeCodingSessionGateRunTitle(state, STALE_AFTER_MS, false);
  assert.match(title, /clock read earlier at the close/);
  assert.match(title, /may still have been running/);
});

test("an open start runs until the stale threshold, then has no result", () => {
  const run = start();
  assert.equal(
    codingSessionGateRunState(run, STALE_AFTER_MS, STARTED + 60_000),
    "running",
  );
  assert.equal(
    codingSessionGateRunState(run, STALE_AFTER_MS, STARTED + STALE_AFTER_MS),
    "no-result",
  );
  assert.equal(isCodingSessionGateRunClosed("running"), false);
  assert.equal(isCodingSessionGateRunClosed("no-result"), false);
  assert.equal(
    describeCodingSessionGateRun(run, "running", clock),
    "cargo test · running since 14:02",
  );
});
