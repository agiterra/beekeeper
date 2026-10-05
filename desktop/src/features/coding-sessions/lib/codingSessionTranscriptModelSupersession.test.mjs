import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionTranscriptModel,
  resolveCodingSessionTurnSettlement,
} from "./codingSessionTranscriptModel.ts";

// SV-44: when does a later turn end an earlier one that never reported a
// completion? Turns are ordered by first appearance, so "not the last turn"
// alone (the rule this replaces) ended a turn that was still publishing.

const timestamp = "2026-07-30T12:00:00.000Z";

function prompt(id, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Brian",
    text: id,
    timestamp,
    turnId,
  };
}

function say(id, turnId) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text: id,
    timestamp,
    turnId,
  };
}

function running(id, turnId) {
  return {
    id,
    type: "tool",
    renderClass: "shell",
    descriptor: { renderClass: "shell", label: "Ran command", preview: id },
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    status: "executing",
    args: { command: id },
    result: "",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: null,
    turnId,
  };
}

function turns(items, isWorking = true) {
  return deriveCodingSessionTranscriptModel(items, { isWorking }).blocks.filter(
    (block) => block.kind === "turn",
  );
}

test("SV-44: a later prompt with its own turn id does not supersede a turn that is still publishing", () => {
  const [earlier, later] = turns([
    prompt("first", "turn-a"),
    running("long-build", "turn-a"),
    // A queued prompt opens its own turn while turn-a runs...
    prompt("second", "turn-b"),
    // ...and turn-a keeps publishing after it.
    say("still-building", "turn-a"),
  ]);
  assert.equal(earlier.id, "turn-a");
  assert.equal(later.id, "turn-b");
  assert.equal(earlier.completion, null);
  assert.equal(earlier.superseded, false);
  // Its open call is not declared over: with the session running it reads
  // live, and with nothing to vouch for it, unknown — never settled.
  assert.equal(resolveCodingSessionTurnSettlement(earlier, "running"), "live");
  assert.equal(
    resolveCodingSessionTurnSettlement(earlier, "unknown"),
    "unknown",
  );
  assert.equal(later.superseded, false);
});

test("SV-44: a later turn that starts after the earlier turn's last item supersedes it", () => {
  const [earlier, later] = turns([
    prompt("first", "turn-a"),
    running("abandoned", "turn-a"),
    prompt("second", "turn-b"),
    say("answer", "turn-b"),
  ]);
  assert.equal(earlier.superseded, true);
  assert.equal(
    resolveCodingSessionTurnSettlement(earlier, "unknown"),
    "settled",
  );
  assert.equal(later.superseded, false);
});

test("SV-44: a turn that goes quiet after interleaving is superseded by a turn that starts later", () => {
  const [first, second, third] = turns([
    prompt("one", "turn-a"),
    prompt("two", "turn-b"),
    say("a-late", "turn-a"),
    running("b-open", "turn-b"),
    prompt("three", "turn-c"),
  ]);
  assert.equal(first.superseded, true);
  assert.equal(second.superseded, true);
  assert.equal(third.superseded, false);
});

test("SV-44: a steer joins the running turn and supersedes nothing", () => {
  const [only, ...rest] = turns([
    prompt("first", "turn-a"),
    running("build", "turn-a"),
    { ...prompt("steer", "turn-a"), steered: true },
  ]);
  assert.equal(rest.length, 0);
  assert.equal(only.superseded, false);
  assert.equal(only.isWorking, true);
});
