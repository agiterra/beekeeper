import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

import {
  clearNewCodingSessionPromptHistory,
  MAX_NEW_CODING_SESSION_PROMPT_HISTORY,
  MAX_NEW_CODING_SESSION_PROMPT_HISTORY_BYTES,
  newCodingSessionPromptHistoryKey,
  readNewCodingSessionPromptHistory,
  rememberNewCodingSessionPrompt,
} from "./newCodingSessionPromptHistory.ts";

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
    values,
  };
}

test("launched prompts come back newest first, per exact scope", () => {
  const storage = memoryStorage();
  rememberNewCodingSessionPrompt("project-a", "Inspect the repo.", storage);
  rememberNewCodingSessionPrompt("project-a", "Run the tests.", storage);
  rememberNewCodingSessionPrompt("project-b", "Something else.", storage);

  assert.deepEqual(readNewCodingSessionPromptHistory("project-a", storage), [
    "Run the tests.",
    "Inspect the repo.",
  ]);
  assert.deepEqual(readNewCodingSessionPromptHistory("project-b", storage), [
    "Something else.",
  ]);
  assert.deepEqual(readNewCodingSessionPromptHistory("project-c", storage), []);
});

test("a prompt launched twice moves to the front instead of occupying two rungs", () => {
  const storage = memoryStorage();
  rememberNewCodingSessionPrompt("scope", "First.", storage);
  rememberNewCodingSessionPrompt("scope", "Second.", storage);
  rememberNewCodingSessionPrompt("scope", "First.", storage);

  assert.deepEqual(readNewCodingSessionPromptHistory("scope", storage), [
    "First.",
    "Second.",
  ]);
});

test("prompts are trimmed, and an empty goal is not remembered at all", () => {
  const storage = memoryStorage();
  rememberNewCodingSessionPrompt("scope", "  Padded.  ", storage);
  rememberNewCodingSessionPrompt("scope", "   ", storage);
  rememberNewCodingSessionPrompt("scope", "", storage);

  assert.deepEqual(readNewCodingSessionPromptHistory("scope", storage), [
    "Padded.",
  ]);
});

test("the list is capped, dropping the oldest", () => {
  const storage = memoryStorage();
  const total = MAX_NEW_CODING_SESSION_PROMPT_HISTORY + 5;
  for (let index = 0; index < total; index += 1) {
    rememberNewCodingSessionPrompt("scope", `prompt ${index}`, storage);
  }
  const history = readNewCodingSessionPromptHistory("scope", storage);
  assert.equal(history.length, MAX_NEW_CODING_SESSION_PROMPT_HISTORY);
  assert.equal(history[0], `prompt ${total - 1}`);
  assert.equal(
    history.at(-1),
    `prompt ${total - MAX_NEW_CODING_SESSION_PROMPT_HISTORY}`,
  );
});

test("an oversized record drops the oldest prompts rather than failing the write", () => {
  const storage = memoryStorage();
  const big = "x".repeat(
    Math.floor(MAX_NEW_CODING_SESSION_PROMPT_HISTORY_BYTES / 2),
  );
  rememberNewCodingSessionPrompt("scope", `${big}-one`, storage);
  rememberNewCodingSessionPrompt("scope", `${big}-two`, storage);
  rememberNewCodingSessionPrompt("scope", `${big}-three`, storage);

  const history = readNewCodingSessionPromptHistory("scope", storage);
  assert.equal(history[0], `${big}-three`);
  assert.ok(
    history.length < 3,
    "the oldest must be dropped to keep the record inside its budget",
  );
  assert.ok(
    new TextEncoder().encode(
      storage.getItem(newCodingSessionPromptHistoryKey("scope")),
    ).byteLength <= MAX_NEW_CODING_SESSION_PROMPT_HISTORY_BYTES,
  );
});

test("the reader fails closed on a stale scope binding or malformed data", () => {
  const storage = memoryStorage();
  storage.setItem(
    newCodingSessionPromptHistoryKey("scope-a"),
    JSON.stringify({
      schema: "buzz-new-coding-session-prompts/v1",
      scopeId: "scope-b",
      prompts: ["wrong scope"],
    }),
  );
  assert.deepEqual(readNewCodingSessionPromptHistory("scope-a", storage), []);

  storage.setItem(newCodingSessionPromptHistoryKey("scope-c"), "{not json");
  assert.deepEqual(readNewCodingSessionPromptHistory("scope-c", storage), []);

  storage.setItem(
    newCodingSessionPromptHistoryKey("scope-d"),
    JSON.stringify({
      schema: "buzz-new-coding-session-prompts/v1",
      scopeId: "scope-d",
      prompts: ["fine", 7],
    }),
  );
  assert.deepEqual(readNewCodingSessionPromptHistory("scope-d", storage), []);
});

test("a denied storage still hands back the prompt just launched", () => {
  const denied = {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("denied");
    },
    removeItem: () => {},
  };
  assert.deepEqual(rememberNewCodingSessionPrompt("scope", "Go.", denied), [
    "Go.",
  ]);
});

test("clearing forgets one scope and leaves the others alone", () => {
  const storage = memoryStorage();
  rememberNewCodingSessionPrompt("scope-a", "Mine.", storage);
  rememberNewCodingSessionPrompt("scope-b", "Theirs.", storage);
  clearNewCodingSessionPromptHistory("scope-a", storage);
  assert.deepEqual(readNewCodingSessionPromptHistory("scope-a", storage), []);
  assert.deepEqual(readNewCodingSessionPromptHistory("scope-b", storage), [
    "Theirs.",
  ]);
});

// ── The shortcut itself ──────────────────────────────────────────────────
//
// The storage tests above prove the list; these prove the keystroke, which
// is the half a person actually experiences.

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    Node: dom.window.Node,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle,
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  localStorage.clear();
});

after(() => dom.window.close());

async function mountRecall(scopeId) {
  const React = await import("react");
  const { render } = await import("@testing-library/react");
  const { useNewCodingSessionPromptRecall } = await import(
    "./newCodingSessionPromptHistory.ts"
  );

  function Harness() {
    const [text, setText] = React.useState("");
    const recall = useNewCodingSessionPromptRecall({ scopeId, text, setText });
    return React.createElement("textarea", {
      "data-testid": "goal",
      onChange: (event) => setText(event.target.value),
      onKeyDown: recall.onKeyDown,
      value: text,
    });
  }

  render(React.createElement(Harness));
  const { screen } = await import("@testing-library/react");
  return screen.getByTestId("goal");
}

/**
 * The primary shortcut modifier for whatever platform JSDOM claims to be —
 * ⌘ on macOS, Ctrl elsewhere. `hasPrimaryShortcutModifier` rejects the other
 * one outright, so this cannot be hard-coded without pinning the test to one
 * machine's answer.
 */
async function primaryModifier() {
  const { isMacPlatform } = await import("@/shared/lib/platform.ts");
  return isMacPlatform() ? { metaKey: true } : { ctrlKey: true };
}

let OLDER;
let NEWER;

before(async () => {
  const modifier = await primaryModifier();
  OLDER = { key: "ArrowUp", ...modifier };
  NEWER = { key: "ArrowDown", ...modifier };
});

test("⌘↑ walks back through launched prompts and ⌘↓ returns the interrupted draft", async () => {
  rememberNewCodingSessionPrompt("scope", "Older prompt.");
  rememberNewCodingSessionPrompt("scope", "Newer prompt.");

  const goal = await mountRecall("scope");
  const { fireEvent } = await import("@testing-library/react");

  fireEvent.change(goal, { target: { value: "half-typed" } });
  assert.equal(goal.value, "half-typed");

  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "Newer prompt.");
  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "Older prompt.");

  // Walking back stops at the oldest rather than wrapping.
  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "Older prompt.");

  fireEvent.keyDown(goal, NEWER);
  assert.equal(goal.value, "Newer prompt.");
  fireEvent.keyDown(goal, NEWER);
  assert.equal(
    goal.value,
    "half-typed",
    "one step past the newest hands back the draft recall interrupted",
  );
});

test("typing over a recalled prompt ends recall, so ⌘↑ starts again from the newest", async () => {
  rememberNewCodingSessionPrompt("scope", "Older prompt.");
  rememberNewCodingSessionPrompt("scope", "Newer prompt.");

  const goal = await mountRecall("scope");
  const { fireEvent } = await import("@testing-library/react");

  fireEvent.keyDown(goal, OLDER);
  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "Older prompt.");

  fireEvent.change(goal, { target: { value: "my own words" } });
  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "Newer prompt.");
});

test("an empty history leaves the field alone", async () => {
  const goal = await mountRecall("empty-scope");
  const { fireEvent } = await import("@testing-library/react");
  fireEvent.change(goal, { target: { value: "only mine" } });
  fireEvent.keyDown(goal, OLDER);
  assert.equal(goal.value, "only mine");
});

test("a bare arrow key is left to the caret", async () => {
  rememberNewCodingSessionPrompt("scope", "Launched before.");
  const goal = await mountRecall("scope");
  const { fireEvent } = await import("@testing-library/react");
  fireEvent.change(goal, { target: { value: "line one\nline two" } });
  fireEvent.keyDown(goal, { key: "ArrowUp" });
  assert.equal(goal.value, "line one\nline two");
});
