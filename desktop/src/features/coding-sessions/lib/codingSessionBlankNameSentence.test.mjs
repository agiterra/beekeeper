import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CODING_SESSION_BLANK_NAME_ANOTHER_COMPUTER,
  CODING_SESSION_BLANK_NAME_SENTENCE,
  CODING_SESSION_BLANK_NAME_SENTENCE_MY_MODEL,
  CODING_SESSION_BLANK_NAME_SENTENCE_OFF,
  codingSessionBlankNameSentence,
} from "./codingSessionBlankNameSentence.ts";

const AGENT =
  "Left blank, it stays untitled unless the agent's computer names it from the first message.";
const MY_MODEL =
  "Left blank, it stays untitled; your naming model only suggests a name in this field while you write.";
const OFF =
  "Left blank, it stays untitled: session titles are Off on this computer.";

test("agent mode (the default): untitled unless the agent's computer names it", () => {
  assert.equal(
    codingSessionBlankNameSentence({
      titleMode: "agent",
      target: "this-computer",
    }),
    AGENT,
  );
  assert.equal(CODING_SESSION_BLANK_NAME_SENTENCE, AGENT);
});

test("my-model mode: untitled; the naming model only suggests in the field", () => {
  assert.equal(
    codingSessionBlankNameSentence({
      titleMode: "my-model",
      target: "this-computer",
    }),
    MY_MODEL,
  );
  assert.equal(CODING_SESSION_BLANK_NAME_SENTENCE_MY_MODEL, MY_MODEL);
});

test("off mode: untitled, and says titles are Off on this computer", () => {
  assert.equal(
    codingSessionBlankNameSentence({
      titleMode: "off",
      target: "this-computer",
    }),
    OFF,
  );
  assert.equal(CODING_SESSION_BLANK_NAME_SENTENCE_OFF, OFF);
});

test("no settings read (no host, or still reading) is the default's wording, not an error", () => {
  assert.equal(codingSessionBlankNameSentence(), AGENT);
  assert.equal(codingSessionBlankNameSentence({ titleMode: null }), AGENT);
});

test("no target chosen yet reads this computer's mode", () => {
  assert.equal(
    codingSessionBlankNameSentence({ titleMode: "off", target: "unknown" }),
    OFF,
  );
  assert.equal(
    codingSessionBlankNameSentence({ titleMode: "my-model" }),
    MY_MODEL,
  );
});

test("a session on another computer follows that computer's setting, in every local mode", () => {
  for (const titleMode of ["agent", "my-model", "off", null]) {
    assert.equal(
      codingSessionBlankNameSentence({
        titleMode,
        target: "another-computer",
      }),
      `${AGENT} ${CODING_SESSION_BLANK_NAME_ANOTHER_COMPUTER}`,
      `mode ${titleMode}`,
    );
  }
  assert.equal(
    CODING_SESSION_BLANK_NAME_ANOTHER_COMPUTER,
    "A session on another computer follows that computer's setting.",
  );
});

test("no mode promises a title, and none claims a desktop namer runs after Start", () => {
  for (const titleMode of ["agent", "my-model", "off", null]) {
    for (const target of ["this-computer", "another-computer", "unknown"]) {
      const sentence = codingSessionBlankNameSentence({ titleMode, target });
      // Hosts with auto-title off, a null titleModel, a pre-S4 host, or a
      // text-less first message produce no title: every claim leads with
      // "stays untitled".
      assert.match(sentence, /^Left blank, it stays untitled/);
      assert.doesNotMatch(sentence, /after Start/);
      assert.doesNotMatch(
        sentence,
        /^Left blank, the agent's computer names it/,
      );
    }
  }
});
