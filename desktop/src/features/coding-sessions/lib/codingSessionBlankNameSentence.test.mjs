import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CODING_SESSION_BLANK_NAME_SENTENCE,
  codingSessionBlankNameSentence,
} from "./codingSessionBlankNameSentence.ts";

test("a blank Name stays untitled unless the agent's computer names it", () => {
  assert.equal(
    codingSessionBlankNameSentence(),
    "Left blank, it stays untitled unless the agent's computer names it from the first message.",
  );
  assert.equal(
    codingSessionBlankNameSentence(),
    CODING_SESSION_BLANK_NAME_SENTENCE,
  );
});

test("the sentence never promises a title the host may not produce", () => {
  const sentence = codingSessionBlankNameSentence();
  // Hosts with auto-title off, a null titleModel, a pre-S4 host, or a
  // text-less first message produce no title: the claim must be conditional.
  assert.match(sentence, /stays untitled unless/);
  assert.doesNotMatch(sentence, /^Left blank, the agent's computer names it/);
});

test("the sentence no longer claims a desktop namer runs after Start", () => {
  const sentence = codingSessionBlankNameSentence();
  assert.doesNotMatch(sentence, /after Start/);
  assert.doesNotMatch(sentence, /Settings/);
});
