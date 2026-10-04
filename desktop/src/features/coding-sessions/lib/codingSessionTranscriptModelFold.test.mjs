import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionTranscriptModel,
  formatFoldedFailures,
  joinCodingSessionProseText,
  joinConsecutiveCodingSessionProse,
  stabilizeCodingSessionTranscriptModel,
} from "./codingSessionTranscriptModel.ts";

const timestamp = "2026-07-30T12:00:00.000Z";

function message({ id, role, text, turnId = "turn-1", messageId }) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId,
    ...(messageId === undefined ? {} : { messageId }),
  };
}

function thought(id, text) {
  return {
    id,
    type: "thought",
    renderClass: "thought",
    title: "Thought",
    text,
    timestamp,
    turnId: "turn-1",
  };
}

function lifecycle({
  id,
  title,
  text,
  renderClass = "status",
  at = timestamp,
}) {
  return {
    id,
    type: "lifecycle",
    renderClass,
    title,
    text,
    timestamp: at,
    turnId: "turn-1",
  };
}

function tool({ id, renderClass = "shell", status = "completed", isError }) {
  const failed = isError ?? status === "failed";
  return {
    id,
    type: "tool",
    renderClass: failed ? "error" : renderClass,
    descriptor: {
      renderClass: failed ? "error" : renderClass,
      label: "Ran command",
      preview: `command-${id}`,
    },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status,
    args: { command: `command-${id}` },
    result: failed ? "exit 1" : "ok",
    isError: failed,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    turnId: "turn-1",
  };
}

const result = (text = "Done. (134000ms) ($0.21)") =>
  lifecycle({ id: "result", title: "Turn result", text });

function onlyTurn(items, isWorking = false) {
  const model = deriveCodingSessionTranscriptModel(items, { isWorking });
  const turn = model.blocks.find((block) => block.kind === "turn");
  assert.ok(turn, "fixture must produce a turn");
  return turn;
}

const keyOf = (entry) => (entry.kind === "item" ? entry.item.id : entry.id);

function visibleKeys(turn) {
  const hidden = new Set(turn.fold?.hiddenIndexes ?? []);
  return turn.entries.flatMap((entry, index) =>
    hidden.has(index) ? [] : [keyOf(entry)],
  );
}

test("a settled turn folds its work, failures before the answer included, and names them", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Fix the build" }),
    thought("think-1", "Look at the logs first."),
    message({ id: "commentary", role: "assistant", text: "Checking logs." }),
    tool({ id: "read-1", renderClass: "file-read" }),
    tool({ id: "run-1" }),
    tool({ id: "run-failed", status: "failed" }),
    tool({ id: "run-2" }),
    lifecycle({
      id: "permission",
      title: "Permission requested",
      text: "Allow Bash?",
      renderClass: "permission",
    }),
    tool({ id: "stuck", status: "executing" }),
    message({ id: "answer", role: "assistant", text: "Fixed the build." }),
    result("Fixed the build. (134000ms) ($0.21)"),
  ]);

  assert.ok(turn.fold, "a settled turn with work folds");
  assert.equal(turn.fold.durationMs, 134000);
  // SV-02 (D2): the failed and the unfinished call fold with the rest, and
  // the fold sentence names them. A failed Bash still counts as a command.
  assert.equal(turn.fold.workSummary, "Read 1 file and ran 4 commands");
  assert.equal(turn.fold.failureSummary, "1 step failed and 1 did not finish");
  assert.equal(
    turn.fold.summary,
    "Read 1 file and ran 4 commands · 1 step failed and 1 did not finish",
  );
  assert.equal(turn.fold.failedCount, 1);
  assert.equal(turn.fold.unfinishedCount, 1);
  // The fold row sits where the first hidden entry was.
  assert.equal(keyOf(turn.entries[turn.fold.anchorIndex]), "think-1");
  assert.deepEqual(visibleKeys(turn), ["prompt", "permission", "answer"]);
  // Opening the fold restores every hidden entry in its own position.
  assert.deepEqual(
    turn.fold.hiddenIndexes.map((index) => keyOf(turn.entries[index])),
    ["think-1", "commentary", "tools:read-1", "run-failed", "run-2", "stuck"],
  );
  assert.equal(turn.completion.state, "completed");
});

test("a failure after the answer stays visible; ordinary trailing work folds", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Ship it" }),
    tool({ id: "run-1" }),
    message({ id: "answer", role: "assistant", text: "Shipped." }),
    tool({ id: "after-ok" }),
    tool({ id: "after-failed", status: "failed" }),
    tool({ id: "after-stuck", status: "executing" }),
    result("Shipped. (1000ms)"),
  ]);
  assert.ok(turn.fold);
  assert.deepEqual(visibleKeys(turn), [
    "prompt",
    "answer",
    "after-failed",
    "after-stuck",
  ]);
  assert.equal(turn.fold.failedCount, 0);
  assert.equal(turn.fold.failureSummary, "");
  assert.equal(turn.fold.summary, "Ran 2 commands");
});

test("a turn that never answered keeps its failures on screen", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Fix it" }),
    tool({ id: "run-1" }),
    tool({ id: "run-failed", status: "failed" }),
    lifecycle({
      id: "stop",
      title: "Interrupted",
      text: "",
      at: "2026-07-30T12:00:05.000Z",
    }),
  ]);
  assert.ok(turn.fold);
  assert.deepEqual(visibleKeys(turn), ["prompt", "run-failed"]);
  assert.equal(turn.fold.failureSummary, "");
});

test("a blank closing message is not an answer: the failure before it stays loud", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Fix it" }),
    tool({ id: "run-1" }),
    tool({ id: "run-failed", status: "failed" }),
    message({ id: "blank", role: "assistant", text: "  \n\t " }),
    lifecycle({
      id: "stop",
      title: "Interrupted",
      text: "",
      at: "2026-07-30T12:00:05.000Z",
    }),
  ]);
  assert.ok(turn.fold);
  // SV-02 (D2): the turn never answered, so the failure is kept on screen
  // rather than folded and counted as a quiet step.
  assert.ok(visibleKeys(turn).includes("run-failed"));
  assert.ok(!visibleKeys(turn).includes("run-1"));
  assert.equal(turn.fold.failedCount, 0);
  assert.equal(turn.fold.failureSummary, "");
});

test("a fold that hides only failures still names them", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Try" }),
    tool({ id: "f-1", status: "failed" }),
    tool({ id: "f-2", status: "failed" }),
    message({ id: "answer", role: "assistant", text: "Worked around it." }),
    result("Worked around it. (1000ms)"),
  ]);
  assert.equal(turn.fold.summary, "Ran 2 commands · 2 steps failed");
  assert.deepEqual(visibleKeys(turn), ["prompt", "answer"]);
});

test("formatFoldedFailures names failed and unfinished steps", () => {
  assert.equal(formatFoldedFailures(0, 0), "");
  assert.equal(formatFoldedFailures(1, 0), "1 step failed");
  assert.equal(formatFoldedFailures(3, 0), "3 steps failed");
  assert.equal(formatFoldedFailures(0, 1), "1 step did not finish");
  assert.equal(formatFoldedFailures(0, 2), "2 steps did not finish");
  assert.equal(
    formatFoldedFailures(2, 1),
    "2 steps failed and 1 did not finish",
  );
});

test("a result body that repeats nothing stays visible beside the agent's own answer", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Go" }),
    tool({ id: "run-1" }),
    message({ id: "answer", role: "assistant", text: "Fixed it." }),
    result("Closing note from the provider. (1000ms)"),
  ]);
  assert.deepEqual(visibleKeys(turn), [
    "prompt",
    "answer",
    "result:assistant-result",
  ]);
});

test("nothing folds while the turn is live", () => {
  const items = [
    message({ id: "prompt", role: "user", text: "Fix the build" }),
    tool({ id: "run-1" }),
    tool({ id: "run-2" }),
    message({ id: "partial", role: "assistant", text: "Halfway." }),
  ];
  const live = onlyTurn(items, true);
  assert.equal(live.isWorking, true);
  assert.equal(live.fold, null);
  assert.deepEqual(visibleKeys(live), ["prompt", "tools:run-1", "partial"]);
});

test("a stopped turn folds with a duration measured from its signed rows", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Fix the build" }),
    tool({ id: "run-1" }),
    tool({ id: "run-2" }),
    lifecycle({
      id: "stop",
      title: "Interrupted",
      text: "",
      at: "2026-07-30T12:00:42.000Z",
    }),
  ]);
  assert.equal(turn.completion.state, "interrupted");
  assert.ok(turn.fold);
  assert.equal(turn.fold.durationMs, 42000);
  assert.deepEqual(visibleKeys(turn), ["prompt"]);
});

test("a turn that only thought does not fold behind a Worked row", () => {
  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Hi" }),
    thought("think-1", "Greet back."),
    message({ id: "answer", role: "assistant", text: "Hello." }),
    result("Hello. (900ms)"),
  ]);
  assert.equal(turn.fold, null);
});

test("paragraph pieces join into one message without breaking a fence or a list", () => {
  const pieces = [
    "Here is the change:\n\n",
    "```ts\nconst a = 1;\n\n",
    "const b = 2;\n```\n\n",
    "Steps:\n\n- one\n- two\n",
    "- three\n\nDone.",
  ];
  const turn = onlyTurn(
    [
      message({ id: "prompt", role: "user", text: "Show me" }),
      ...pieces.map((text, index) =>
        message({ id: `piece-${index}`, role: "assistant", text }),
      ),
      result(`${pieces.join("")} (2000ms)`),
    ],
    false,
  );

  const assistant = turn.entries.filter(
    (entry) =>
      entry.kind === "item" &&
      entry.item.type === "message" &&
      entry.item.role === "assistant",
  );
  assert.equal(assistant.length, 1, "one message, not five blocks");
  assert.equal(assistant[0].item.id, "piece-0", "keyed by its first piece");
  assert.equal(assistant[0].item.text, pieces.join(""));
  assert.equal(assistant[0].item.text.match(/^```/gm)?.length, 2);
  assert.match(assistant[0].item.text, /- one\n- two\n- three/);
  // The result body equals the joined answer, so it is not repeated.
  assert.equal(
    turn.entries.some((entry) => keyOf(entry) === "result:assistant-result"),
    false,
  );
});

test("two messages the provider names as distinct get a paragraph break", () => {
  assert.equal(
    joinCodingSessionProseText("Done.", "Next", true),
    "Done.\n\nNext",
  );
  assert.equal(
    joinCodingSessionProseText("Done.\n\n", "Next", true),
    "Done.\n\nNext",
  );
  // Slices of one message are concatenated exactly, even mid-word.
  assert.equal(joinCodingSessionProseText("Hel", "lo", false), "Hello");

  const turn = onlyTurn([
    message({ id: "prompt", role: "user", text: "Go" }),
    message({ id: "a", role: "assistant", text: "Done.", messageId: "m1" }),
    message({ id: "b", role: "assistant", text: "Next", messageId: "m2" }),
  ]);
  assert.equal(turn.entries[1].item.text, "Done.\n\nNext");
});

test("prose separated by a tool stays two messages", () => {
  const turn = onlyTurn(
    [
      message({ id: "prompt", role: "user", text: "Go" }),
      message({ id: "before", role: "assistant", text: "Looking." }),
      tool({ id: "run-1" }),
      message({ id: "after", role: "assistant", text: "Found it." }),
    ],
    true,
  );
  assert.deepEqual(turn.entries.map(keyOf), [
    "prompt",
    "before",
    "run-1",
    "after",
  ]);
});

test("a settled turn whose answer arrived in pieces keeps its block identity", () => {
  const items = [
    message({ id: "prompt", role: "user", text: "Go" }),
    message({ id: "p1", role: "assistant", text: "First paragraph.\n\n" }),
    message({ id: "p2", role: "assistant", text: "Second paragraph." }),
    result("First paragraph.\n\nSecond paragraph. (1000ms)"),
  ];
  const previous = deriveCodingSessionTranscriptModel(items, {
    isWorking: false,
  });
  const next = deriveCodingSessionTranscriptModel(
    [
      ...items,
      message({ id: "prompt-2", role: "user", text: "More", turnId: "turn-2" }),
    ],
    { isWorking: true },
  );
  const stable = stabilizeCodingSessionTranscriptModel(previous, next);
  assert.equal(
    stable.blocks[0],
    previous.blocks[0],
    "a rebuilt joined message must not make a settled turn look changed",
  );
});

test("prose from a different signer, generation, author or subagent never joins", () => {
  const piece = (id, extra) => ({
    ...message({ id, role: "assistant", text: `${id} words. ` }),
    ...extra,
  });
  const cases = {
    sessionId: [{ sessionId: "gen-a" }, { sessionId: "gen-b" }],
    authorPubkey: [
      { authorPubkey: "a".repeat(64) },
      { authorPubkey: "b".repeat(64) },
    ],
    bridgeSource: [
      { bridgeSource: { pubkey: "1".repeat(64), label: "one" } },
      { bridgeSource: { pubkey: "2".repeat(64), label: "two" } },
    ],
    parentToolId: [{}, { parentToolId: "call-1" }],
  };
  for (const [field, [left, right]] of Object.entries(cases)) {
    const joined = joinConsecutiveCodingSessionProse([
      piece("left", left),
      piece("right", right),
    ]);
    assert.deepEqual(
      joined.map((item) => [item.id, item.text]),
      [
        ["left", "left words. "],
        ["right", "right words. "],
      ],
      `${field} differs, so the pieces stay apart`,
    );
  }
  // Different turn ids never meet inside one turn, but the rule holds anyway.
  assert.equal(
    joinConsecutiveCodingSessionProse([
      piece("left", { turnId: "turn-1" }),
      piece("right", { turnId: "turn-2" }),
    ]).length,
    2,
  );
  // Identically attributed pieces still join (pre-L1 size flushes included).
  const same = {
    sessionId: "gen-a",
    bridgeSource: { pubkey: "1".repeat(64), label: "x" },
  };
  const joined = joinConsecutiveCodingSessionProse([
    piece("left", same),
    piece("right", same),
  ]);
  assert.deepEqual(
    joined.map((item) => [item.id, item.text]),
    [["left", "left words. right words. "]],
  );
});
