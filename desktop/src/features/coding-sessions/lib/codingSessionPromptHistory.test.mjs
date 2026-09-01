import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionPromptHistory,
  IDLE_PROMPT_RECALL,
  MAX_PROMPT_HISTORY,
  stepPromptRecall,
} from "./codingSessionPromptHistory.ts";

const ME = "a".repeat(64);
const SOMEONE_ELSE = "b".repeat(64);

function userMessage(text, operatorPubkey = ME) {
  return {
    id: `${text}-${operatorPubkey}`,
    type: "message",
    renderClass: "message",
    role: "user",
    title: "You",
    text,
    timestamp: "2026-09-01T00:00:00Z",
    operatorPubkey,
  };
}

function assistantMessage(text) {
  return {
    id: `assistant-${text}`,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Agent",
    text,
    timestamp: "2026-09-01T00:00:00Z",
  };
}

function history(overrides = {}) {
  return buildCodingSessionPromptHistory({
    transcript: [],
    pending: [],
    currentPubkey: ME,
    ...overrides,
  });
}

// ── Building the list ────────────────────────────────────────────────────────

test("prompts come back newest first", () => {
  assert.deepEqual(
    history({
      transcript: [
        userMessage("first"),
        assistantMessage("working on it"),
        userMessage("second"),
      ],
    }),
    ["second", "first"],
  );
});

test("only user turns are prompts", () => {
  assert.deepEqual(history({ transcript: [assistantMessage("hello")] }), []);
});

// A coding session is multi-operator: a founder and a granted operator steer
// the same execution. Recalling someone else's instruction and pressing Enter
// would put words in their mouth.
test("another operator's turns are not yours to recall", () => {
  assert.deepEqual(
    history({
      transcript: [userMessage("mine"), userMessage("theirs", SOMEONE_ELSE)],
    }),
    ["mine"],
  );
});

test("unattributed turns are kept — they predate attribution", () => {
  assert.deepEqual(history({ transcript: [userMessage("old", null)] }), [
    "old",
  ]);
});

test("with no signed-in identity, only unattributed turns come back", () => {
  assert.deepEqual(
    history({
      currentPubkey: null,
      transcript: [userMessage("old", null), userMessage("mine")],
    }),
    ["old"],
  );
});

test("attribution matching ignores case", () => {
  assert.deepEqual(
    history({ transcript: [userMessage("mine", ME.toUpperCase())] }),
    ["mine"],
  );
});

// The umbrella composer strips a routing @handle before publishing, so the
// wire text is not what the person typed.
test("a pending turn recalls the draft, not the wire text", () => {
  assert.deepEqual(
    history({
      pending: [
        {
          draft: "@codex fix the test",
          text: "fix the test",
          operatorPubkey: ME,
        },
      ],
    }),
    ["@codex fix the test"],
  );
});

test("a pending turn with no separate draft falls back to its text", () => {
  assert.deepEqual(
    history({ pending: [{ text: "fix the test", operatorPubkey: ME }] }),
    ["fix the test"],
  );
});

test("pending turns are newer than anything settled", () => {
  assert.deepEqual(
    history({
      transcript: [userMessage("settled")],
      pending: [{ text: "in flight", operatorPubkey: ME }],
    }),
    ["in flight", "settled"],
  );
});

test("consecutive repeats collapse; a repeat with a gap does not", () => {
  assert.deepEqual(
    history({
      transcript: [
        userMessage("run tests"),
        userMessage("run tests"),
        userMessage("look again"),
        userMessage("run tests"),
      ],
    }),
    ["run tests", "look again", "run tests"],
  );
});

test("blank prompts never enter the history", () => {
  assert.deepEqual(
    history({ transcript: [userMessage("   "), userMessage("real")] }),
    ["real"],
  );
});

test("the history is bounded", () => {
  const transcript = Array.from({ length: MAX_PROMPT_HISTORY + 20 }, (_, i) =>
    userMessage(`prompt ${i}`),
  );
  assert.equal(history({ transcript }).length, MAX_PROMPT_HISTORY);
});

// ── Walking it ───────────────────────────────────────────────────────────────

const LIST = ["newest", "middle", "oldest"];

test("the first ⌘↑ stashes the draft and lands on the newest prompt", () => {
  const step = stepPromptRecall(
    "older",
    IDLE_PROMPT_RECALL,
    LIST,
    "half typed",
  );
  assert.equal(step.text, "newest");
  assert.deepEqual(step.state, { cursor: 0, stashedDraft: "half typed" });
});

test("⌘↑ walks back and stops at the oldest rather than wrapping", () => {
  const state = { cursor: 2, stashedDraft: "draft" };
  const step = stepPromptRecall("older", state, LIST, "oldest");
  assert.equal(step.text, null);
  assert.deepEqual(step.state, state);
});

test("⌘↓ walks forward through the list", () => {
  const step = stepPromptRecall(
    "newer",
    { cursor: 2, stashedDraft: "draft" },
    LIST,
    "oldest",
  );
  assert.equal(step.text, "middle");
  assert.deepEqual(step.state, { cursor: 1, stashedDraft: "draft" });
});

test("one ⌘↓ past the newest hands back the interrupted draft", () => {
  const step = stepPromptRecall(
    "newer",
    { cursor: 0, stashedDraft: "half typed" },
    LIST,
    "newest",
  );
  assert.equal(step.text, "half typed");
  assert.deepEqual(step.state, IDLE_PROMPT_RECALL);
});

test("⌘↓ before any recall does nothing", () => {
  const step = stepPromptRecall("newer", IDLE_PROMPT_RECALL, LIST, "typing");
  assert.equal(step.text, null);
  assert.deepEqual(step.state, IDLE_PROMPT_RECALL);
});

test("an empty history leaves the draft alone", () => {
  const step = stepPromptRecall("older", IDLE_PROMPT_RECALL, [], "typing");
  assert.equal(step.text, null);
  assert.deepEqual(step.state, IDLE_PROMPT_RECALL);
});
