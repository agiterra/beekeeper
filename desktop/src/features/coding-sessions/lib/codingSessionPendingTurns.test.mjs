import assert from "node:assert/strict";
import test from "node:test";

import {
  formatPendingCodingSessionTurnAge,
  heldPendingCodingSessionTurns,
  markPendingCodingSessionTurnDegraded,
  markPendingCodingSessionTurnQueued,
  MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET,
  PENDING_CODING_SESSION_TURN_STALL_MS,
  PENDING_CODING_SESSION_TURN_TTL_MS,
  clearPendingCodingSessionTurns,
  forgetPendingCodingSessionTurn,
  markPendingCodingSessionTurnPublished,
  pendingCodingSessionTurnKey,
  pendingCodingSessionTurnState,
  readPendingCodingSessionTurns,
  recordPendingCodingSessionTurn,
  resetPendingCodingSessionTurns,
  noteTextSettledCodingSessionEchoes,
  readTextSettledCodingSessionEchoes,
  resolvePendingCodingSessionTurns,
} from "./codingSessionPendingTurns.ts";

const CHANNEL = "channel-1";
const TARGET = "coding-session/v1|target-a";
const OTHER_TARGET = "coding-session/v1|target-b";
const OPERATOR = "a".repeat(64);

function turn(overrides = {}) {
  return {
    channelId: CHANNEL,
    targetKey: TARGET,
    commandId: "csc-1",
    text: "run the tests",
    operatorPubkey: OPERATOR,
    recordedAt: 1_000,
    published: false,
    ...overrides,
  };
}

function promptEcho(text, operatorPubkey = OPERATOR) {
  return { type: "message", role: "user", text, operatorPubkey };
}

/** The echo a provider on the per-stage contract publishes: it names its turn. */
function namedEcho(commandId, text = "run the tests", overrides = {}) {
  return {
    id: `item-${commandId}`,
    type: "message",
    role: "user",
    text,
    operatorPubkey: OPERATOR,
    commandId,
    ...overrides,
  };
}

test.afterEach(() => resetPendingCodingSessionTurns());

test("a recorded turn is visible before the relay has answered", () => {
  const pending = [turn()];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [],
    1_000,
  );
  assert.equal(resolved.visible.length, 1);
  assert.deepEqual(resolved.consumedKeys, []);
  assert.equal(pendingCodingSessionTurnState(pending[0], 1_000), "sending");
});

test("the provider's verified prompt echo retires the row", () => {
  const pending = [turn({ published: true })];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [promptEcho("run the tests")],
    1_500,
  );
  assert.deepEqual(resolved.visible, []);
  assert.deepEqual(resolved.consumedKeys, [
    pendingCodingSessionTurnKey(pending[0]),
  ]);
});

test("one echo retires one row, so a deliberate repeat survives the first", () => {
  const pending = [
    turn({ commandId: "csc-1" }),
    turn({ commandId: "csc-2", recordedAt: 1_100 }),
  ];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [promptEcho("run the tests")],
    1_200,
  );
  assert.equal(resolved.visible.length, 1);
  assert.equal(resolved.visible[0].commandId, "csc-2");
});

test("another operator's identical words do not retire this client's row", () => {
  const pending = [turn()];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [promptEcho("run the tests", "b".repeat(64))],
    1_000,
  );
  assert.equal(resolved.visible.length, 1);
});

test("an unattributed echo still retires the row rather than stranding it", () => {
  const pending = [turn()];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [promptEcho("run the tests", null)],
    1_000,
  );
  assert.deepEqual(resolved.visible, []);
});

test("assistant text is never mistaken for a prompt echo", () => {
  const pending = [turn()];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [{ type: "message", role: "assistant", text: "run the tests" }],
    1_000,
  );
  assert.equal(resolved.visible.length, 1);
});

test("rows are scoped to their own execution", () => {
  const pending = [
    turn(),
    turn({ commandId: "csc-2", targetKey: OTHER_TARGET }),
  ];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [],
    1_000,
  );
  assert.equal(resolved.visible.length, 1);
  assert.equal(resolved.visible[0].commandId, "csc-1");
  assert.deepEqual(resolved.consumedKeys, []);
});

test("a published turn goes stalled rather than spinning forever", () => {
  const sent = turn({ published: true });
  assert.equal(pendingCodingSessionTurnState(sent, 1_000), "waiting");
  assert.equal(
    pendingCodingSessionTurnState(
      sent,
      1_000 + PENDING_CODING_SESSION_TURN_STALL_MS + 1,
    ),
    "stalled",
  );
});

test("an unanswered row is dropped once its TTL passes", () => {
  const pending = [turn({ published: true })];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [],
    1_000 + PENDING_CODING_SESSION_TURN_TTL_MS + 1,
  );
  assert.deepEqual(resolved.visible, []);
  assert.deepEqual(resolved.consumedKeys, [
    pendingCodingSessionTurnKey(pending[0]),
  ]);
});

test("the store records, publishes, clears and resets", () => {
  recordPendingCodingSessionTurn(turn());
  markPendingCodingSessionTurnPublished(CHANNEL, "csc-1");
  assert.equal(readPendingCodingSessionTurns().length, 1);
  assert.equal(readPendingCodingSessionTurns()[0].published, true);

  forgetPendingCodingSessionTurn(CHANNEL, "csc-1");
  assert.equal(readPendingCodingSessionTurns().length, 0);

  recordPendingCodingSessionTurn(turn({ commandId: "csc-9" }));
  clearPendingCodingSessionTurns([
    pendingCodingSessionTurnKey({ channelId: CHANNEL, commandId: "csc-9" }),
  ]);
  assert.equal(readPendingCodingSessionTurns().length, 0);

  recordPendingCodingSessionTurn(turn({ commandId: "csc-10" }));
  resetPendingCodingSessionTurns();
  assert.equal(readPendingCodingSessionTurns().length, 0);
});

test("a target keeps a bounded number of pending rows", () => {
  for (
    let index = 0;
    index <= MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET;
    index += 1
  ) {
    recordPendingCodingSessionTurn(
      turn({ commandId: `csc-${index}`, recordedAt: 1_000 + index }),
    );
  }
  const stored = readPendingCodingSessionTurns();
  assert.equal(stored.length, MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET);
  // The oldest is the one dropped.
  assert.equal(stored[0].commandId, "csc-1");
});

test("another execution's rows are not evicted by a busy one", () => {
  recordPendingCodingSessionTurn(
    turn({ commandId: "other", targetKey: OTHER_TARGET }),
  );
  for (
    let index = 0;
    index <= MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET;
    index += 1
  ) {
    recordPendingCodingSessionTurn(turn({ commandId: `csc-${index}` }));
  }
  assert.equal(
    readPendingCodingSessionTurns().filter(
      (entry) => entry.targetKey === OTHER_TARGET,
    ).length,
    1,
  );
});

test("the same sentence sent twice settles by command id, not by order", () => {
  const pending = [
    turn({ commandId: "csc-1" }),
    turn({ commandId: "csc-2", recordedAt: 1_100 }),
  ];
  // The provider echoes the *second* turn first. Text matching cannot tell
  // these apart and would retire the first row, leaving the wrong message
  // duplicated on screen until the second echo arrived.
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [namedEcho("csc-2")],
    1_200,
  );
  assert.equal(resolved.visible.length, 1);
  assert.equal(resolved.visible[0].commandId, "csc-1");
  assert.deepEqual(resolved.consumedKeys, [
    pendingCodingSessionTurnKey(pending[1]),
  ]);
  assert.deepEqual(resolved.settlements, [
    {
      key: pendingCodingSessionTurnKey(pending[1]),
      commandId: "csc-2",
      by: "commandId",
      echoId: "item-csc-2",
    },
  ]);
});

test("an echo naming another command does not retire this row", () => {
  const pending = [turn({ commandId: "csc-1" })];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    // Identical words, a different command: another operator's turn, or this
    // client's own earlier one replayed. Neither is an answer to this row.
    [namedEcho("csc-other")],
    1_200,
  );
  assert.equal(resolved.visible.length, 1);
  assert.deepEqual(resolved.consumedKeys, []);
  assert.deepEqual(resolved.settlements, []);
});

test("text is the join only for an echo that names no command", () => {
  const pending = [turn({ commandId: "csc-1" })];
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [{ ...promptEcho("run the tests"), id: "item-legacy" }],
    1_200,
  );
  assert.deepEqual(resolved.visible, []);
  assert.deepEqual(resolved.settlements, [
    {
      key: pendingCodingSessionTurnKey(pending[0]),
      commandId: "csc-1",
      by: "text",
      echoId: "item-legacy",
    },
  ]);
});

test("a named echo is claimed by its own row before a guess can take it", () => {
  const pending = [
    turn({ commandId: "csc-1" }),
    turn({ commandId: "csc-2", recordedAt: 1_100 }),
  ];
  // One old-style echo and one named echo, with the named one belonging to the
  // second row. Matching in row order without the two passes would let the
  // first row swallow the named echo by text and strand the second.
  const resolved = resolvePendingCodingSessionTurns(
    pending,
    { channelId: CHANNEL, targetKey: TARGET },
    [namedEcho("csc-2"), { ...promptEcho("run the tests"), id: "item-legacy" }],
    1_200,
  );
  assert.deepEqual(resolved.visible, []);
  const settledBy = new Map(
    resolved.settlements.map((entry) => [entry.commandId, entry.by]),
  );
  assert.equal(settledBy.get("csc-2"), "commandId");
  assert.equal(settledBy.get("csc-1"), "text");
});

test("a queued turn says it is queued, not that it stalled", () => {
  recordPendingCodingSessionTurn(turn({ commandId: "csc-q" }));
  markPendingCodingSessionTurnPublished(CHANNEL, "csc-q");
  markPendingCodingSessionTurnQueued(CHANNEL, "csc-q");
  const [stored] = readPendingCodingSessionTurns();
  assert.equal(stored.queuedByProvider, true);
  assert.equal(pendingCodingSessionTurnState(stored, 1_000), "queued");
  assert.equal(
    pendingCodingSessionTurnState(
      stored,
      1_000 + PENDING_CODING_SESSION_TURN_STALL_MS + 1,
    ),
    "queued",
  );
});

test("text-only settlements are remembered so the message can disclose them", () => {
  assert.equal(readTextSettledCodingSessionEchoes().size, 0);
  noteTextSettledCodingSessionEchoes(["item-legacy"]);
  assert.equal(readTextSettledCodingSessionEchoes().has("item-legacy"), true);
  resetPendingCodingSessionTurns();
  assert.equal(readTextSettledCodingSessionEchoes().size, 0);
});

test("a turn the provider holds outlives the unanswered-row TTL", () => {
  // The TTL exists to clear a row nobody ever picked up. A signed
  // `turn_queued` is the provider saying it did pick it up, so dropping the
  // row at three minutes would delete a message that is genuinely still
  // coming — the exact silence this whole store exists to remove.
  const held = turn({
    commandId: "csc-held",
    published: true,
    queuedByProvider: true,
  });
  const resolved = resolvePendingCodingSessionTurns(
    [held],
    { channelId: CHANNEL, targetKey: TARGET },
    [],
    1_000 + PENDING_CODING_SESSION_TURN_TTL_MS + 1,
  );
  assert.equal(resolved.visible.length, 1);
  assert.deepEqual(resolved.consumedKeys, []);

  const unheld = turn({ commandId: "csc-unheld", published: true });
  assert.deepEqual(
    resolvePendingCodingSessionTurns(
      [unheld],
      { channelId: CHANNEL, targetKey: TARGET },
      [],
      1_000 + PENDING_CODING_SESSION_TURN_TTL_MS + 1,
    ).visible,
    [],
  );
});

test("a degraded steer is held at the boundary, and says which", () => {
  recordPendingCodingSessionTurn(turn({ commandId: "csc-d" }));
  markPendingCodingSessionTurnDegraded(CHANNEL, "csc-d");
  const [stored] = readPendingCodingSessionTurns();
  assert.equal(stored.published, true);
  assert.equal(stored.degradedByProvider, true);
  assert.equal(pendingCodingSessionTurnState(stored, 1_000), "degraded");
  // Degradation outranks the queue receipt that follows it: the person asked
  // to steer and did not get to.
  markPendingCodingSessionTurnQueued(CHANNEL, "csc-d");
  const [requeued] = readPendingCodingSessionTurns();
  assert.equal(pendingCodingSessionTurnState(requeued, 1_000), "degraded");
  assert.equal(
    resolvePendingCodingSessionTurns(
      readPendingCodingSessionTurns(),
      { channelId: CHANNEL, targetKey: TARGET },
      [],
      1_000 + PENDING_CODING_SESSION_TURN_TTL_MS + 1,
    ).visible.length,
    1,
  );
});

test("the age a held row shows counts seconds before it counts minutes", () => {
  assert.equal(formatPendingCodingSessionTurnAge(0), "0s");
  assert.equal(formatPendingCodingSessionTurnAge(11_000), "11s");
  assert.equal(formatPendingCodingSessionTurnAge(59_999), "59s");
  assert.equal(formatPendingCodingSessionTurnAge(60_000), "1m");
  assert.equal(formatPendingCodingSessionTurnAge(3_599_000), "59m");
  assert.equal(formatPendingCodingSessionTurnAge(3_600_000), "1h");
  assert.equal(formatPendingCodingSessionTurnAge(-1), "0s");
});

test("the rows a remounting composer must adopt are the held ones, with the draft", () => {
  // The other half of "a held row outlives the TTL": the watch that is now the
  // only thing able to retire it lives in a component, so a composer coming
  // back has to be able to find these again. Held only — an unheld row still
  // expires on its own — and scoped to one execution.
  recordPendingCodingSessionTurn(
    turn({ commandId: "csc-held", text: "run it", draft: "@builder run it" }),
  );
  markPendingCodingSessionTurnQueued(CHANNEL, "csc-held");
  recordPendingCodingSessionTurn(turn({ commandId: "csc-degraded" }));
  markPendingCodingSessionTurnDegraded(CHANNEL, "csc-degraded");
  recordPendingCodingSessionTurn(turn({ commandId: "csc-unheld" }));
  markPendingCodingSessionTurnPublished(CHANNEL, "csc-unheld");
  recordPendingCodingSessionTurn(
    turn({ commandId: "csc-elsewhere", targetKey: OTHER_TARGET }),
  );
  markPendingCodingSessionTurnQueued(CHANNEL, "csc-elsewhere");

  const held = heldPendingCodingSessionTurns(CHANNEL, TARGET);
  assert.deepEqual(
    held.map((entry) => entry.commandId),
    ["csc-held", "csc-degraded"],
  );
  // The person's own words, not the wire text: an umbrella composer strips a
  // routing handle before publishing, and it is the draft that goes back in
  // the editor.
  assert.equal(held[0].draft, "@builder run it");
  assert.deepEqual(heldPendingCodingSessionTurns("other-channel", TARGET), []);
});
