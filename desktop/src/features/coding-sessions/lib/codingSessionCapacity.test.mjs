import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_CAPACITY_MAX,
  CODING_SESSION_TURN_BUDGET_MAX,
  codingSessionCapacityChoice,
  codingSessionCapacityLabel,
  codingSessionCapacityPending,
  codingSessionCapacityValue,
  codingSessionIdleTimeoutLabel,
  codingSessionIdleTimeoutMinutes,
  codingSessionIdleTimeoutPending,
  codingSessionTurnBudgetLabel,
  codingSessionTurnBudgetPending,
  codingSessionTurnBudgetUsage,
  parseCodingSessionCapacityInput,
  parseCodingSessionIdleTimeoutInput,
  parseCodingSessionTurnBudgetInput,
} from "./codingSessionCapacity.ts";

test("the three states round-trip", () => {
  for (const [stored, kind] of [
    [null, "default"],
    [0, "unlimited"],
    [6, "limit"],
  ]) {
    const choice = codingSessionCapacityChoice(stored);
    assert.equal(choice.kind, kind);
    assert.equal(codingSessionCapacityValue(choice), stored);
  }
});

test("clearing the field never silently means unlimited", () => {
  // 0 is unlimited only when chosen deliberately.
  assert.equal(parseCodingSessionCapacityInput("", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("   ", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("nonsense", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("0", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("-3", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("7", 4), 7);
  assert.equal(
    parseCodingSessionCapacityInput("9000", 4),
    CODING_SESSION_CAPACITY_MAX,
    "a typo is bounded rather than obeyed",
  );
});

test("the label names the default's number rather than the word", () => {
  assert.equal(codingSessionCapacityLabel(null, 4), "4 sessions");
  assert.equal(codingSessionCapacityLabel(1, 4), "1 session");
  assert.equal(codingSessionCapacityLabel(0, 4), "Unlimited");
});

// A saved ceiling reaches the child only at its next start (§2 item 41's rule:
// never show a setting as in force when it is not).
test("a change that is not yet in force says so, and one that is stays quiet", () => {
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: null,
      providerRunning: true,
    }),
    "The provider running now started with 4 sessions. Your change applies the next time it starts.",
  );
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: 8,
      providerRunning: true,
    }),
    null,
  );
  // Nothing is running, so nothing is being enforced to contradict.
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: null,
      providerRunning: false,
    }),
    null,
  );
});

// Reported 2026-08-24: two of Andy's turns died as "Idle timeout — no agent
// activity for 900s". The clock is a silence budget — every line the adapter
// writes resets it — so a single long command that reports only on completion
// is what spends it. It is now the person's to set.
test("typed minutes are coerced the same way the capacity field is", () => {
  assert.equal(parseCodingSessionIdleTimeoutInput("30", 15), 30);
  // Unusable input keeps what was there rather than inventing a number.
  for (const raw of ["", "   ", "abc", "0", "-5"]) {
    assert.equal(parseCodingSessionIdleTimeoutInput(raw, 15), 15);
  }
  assert.equal(parseCodingSessionIdleTimeoutInput("99999", 15), 240);
});

test("seconds read back as the minutes a person set, never as zero", () => {
  assert.equal(codingSessionIdleTimeoutMinutes(900), 15);
  assert.equal(codingSessionIdleTimeoutMinutes(3600), 60);
  // A sub-minute budget would otherwise render as "0 minutes".
  assert.equal(codingSessionIdleTimeoutMinutes(30), 1);
});

test("the label names the default when nothing is stored", () => {
  assert.equal(codingSessionIdleTimeoutLabel(null, 900), "15 minutes");
  assert.equal(codingSessionIdleTimeoutLabel(3600, 900), "60 minutes");
  assert.equal(codingSessionIdleTimeoutLabel(60, 900), "1 minute");
});

test("a saved budget that is not in force says so, and an equal one stays quiet", () => {
  assert.equal(
    codingSessionIdleTimeoutPending({
      turnIdleTimeoutSecs: 3600,
      defaultTurnIdleTimeoutSecs: 900,
      runningTurnIdleTimeoutSecs: null,
      providerRunning: true,
    }),
    "The provider running now gives up after 15 minutes of silence. Your change applies the next time it starts.",
  );
  assert.equal(
    codingSessionIdleTimeoutPending({
      turnIdleTimeoutSecs: 900,
      defaultTurnIdleTimeoutSecs: 900,
      runningTurnIdleTimeoutSecs: null,
      providerRunning: true,
    }),
    null,
    "stored equals running: there is nothing pending to disclose",
  );
  assert.equal(
    codingSessionIdleTimeoutPending({
      turnIdleTimeoutSecs: 3600,
      defaultTurnIdleTimeoutSecs: 900,
      runningTurnIdleTimeoutSecs: null,
      providerRunning: false,
    }),
    null,
    "nothing is running, so nothing is enforcing an older number",
  );
});

// D9: the crew turn budget is the third setting in this panel and obeys the
// same three rules — clearing the field never means "no limit", the label
// names the default's number, and a saved change admits it is not in force.
test("the crew turn budget reads and parses like its siblings", () => {
  assert.equal(parseCodingSessionTurnBudgetInput("", 200), 200);
  assert.equal(parseCodingSessionTurnBudgetInput("0", 200), 200);
  assert.equal(parseCodingSessionTurnBudgetInput("-1", 200), 200);
  assert.equal(parseCodingSessionTurnBudgetInput("nonsense", 200), 200);
  assert.equal(parseCodingSessionTurnBudgetInput("40", 200), 40);
  assert.equal(
    parseCodingSessionTurnBudgetInput("99999999", 200),
    CODING_SESSION_TURN_BUDGET_MAX,
  );

  assert.equal(codingSessionTurnBudgetLabel(null, 200), "200 turns");
  assert.equal(codingSessionTurnBudgetLabel(1, 200), "1 turn");
  assert.equal(codingSessionTurnBudgetLabel(0, 200), "No limit");
});

test("a crew budget that is not yet in force says so", () => {
  assert.equal(
    codingSessionTurnBudgetPending({
      turnBudget: 50,
      defaultTurnBudget: 200,
      runningTurnBudget: null,
      providerRunning: true,
    }),
    "The provider running now allows 200 turns per crew session. Your change applies the next time it starts.",
  );
  assert.equal(
    codingSessionTurnBudgetPending({
      turnBudget: 50,
      defaultTurnBudget: 200,
      runningTurnBudget: 50,
      providerRunning: true,
    }),
    null,
  );
  assert.equal(
    codingSessionTurnBudgetPending({
      turnBudget: 50,
      defaultTurnBudget: 200,
      runningTurnBudget: null,
      providerRunning: false,
    }),
    null,
  );
});

// The founder is never refused, so a crew really can end up past its
// allowance. Showing a negative remainder, or clamping it away, would both be
// lies about what was spent.
test("crew spend reads honestly at, under, and past the limit", () => {
  assert.equal(
    codingSessionTurnBudgetUsage({ used: 12, limit: 200 }),
    "12 of 200 turns used (188 left)",
  );
  assert.equal(
    codingSessionTurnBudgetUsage({ used: 200, limit: 200 }),
    "200 of 200 turns used (none left)",
  );
  assert.equal(
    codingSessionTurnBudgetUsage({ used: 203, limit: 200 }),
    "203 of 200 turns used (3 over)",
  );
});
