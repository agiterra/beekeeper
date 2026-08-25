import assert from "node:assert/strict";
import test from "node:test";

import {
  MIN_CODING_SESSION_NAME_SUGGEST_CHARS,
  codingSessionNameSuggestStatus,
  shouldAdoptSuggestion,
  shouldRequestCodingSessionName,
} from "./codingSessionNameSuggestion.ts";

const LONG_ENOUGH = "a".repeat(MIN_CODING_SESSION_NAME_SUGGEST_CHARS);

test("a namer that is off never sends anything", () => {
  assert.equal(
    shouldRequestCodingSessionName({
      enabled: false,
      inFlight: false,
      lastRequestedText: null,
      text: LONG_ENOUGH,
    }),
    false,
  );
});

test("a short first message is not worth naming", () => {
  assert.equal(
    shouldRequestCodingSessionName({
      enabled: true,
      inFlight: false,
      lastRequestedText: null,
      text: "fix it",
    }),
    false,
  );
});

test("text already named is never sent twice", () => {
  assert.equal(
    shouldRequestCodingSessionName({
      enabled: true,
      inFlight: false,
      lastRequestedText: LONG_ENOUGH,
      text: `  ${LONG_ENOUGH}  `,
    }),
    false,
  );
});

test("changed text is sent once the previous request settles", () => {
  const input = {
    enabled: true,
    lastRequestedText: LONG_ENOUGH,
    text: `${LONG_ENOUGH} and more`,
  };
  assert.equal(
    shouldRequestCodingSessionName({ ...input, inFlight: true }),
    false,
  );
  assert.equal(
    shouldRequestCodingSessionName({ ...input, inFlight: false }),
    true,
  );
});

test("a newer suggestion replaces the value it filled in itself", () => {
  assert.equal(
    shouldAdoptSuggestion({
      current: "Old name",
      lastAutoFilled: "Old name",
      suggestion: "New name",
    }),
    true,
  );
});

test("a hand-typed name is never overwritten", () => {
  assert.equal(
    shouldAdoptSuggestion({
      current: "Mine",
      lastAutoFilled: "Old name",
      suggestion: "New name",
    }),
    false,
  );
});

test("an empty field always takes the suggestion", () => {
  assert.equal(
    shouldAdoptSuggestion({
      current: "  ",
      lastAutoFilled: null,
      suggestion: "New name",
    }),
    true,
  );
});

test("an empty or unchanged suggestion is not adopted", () => {
  assert.equal(
    shouldAdoptSuggestion({
      current: "",
      lastAutoFilled: null,
      suggestion: "",
    }),
    false,
  );
  assert.equal(
    shouldAdoptSuggestion({
      current: "Same",
      lastAutoFilled: "Same",
      suggestion: "Same",
    }),
    false,
  );
});

test("an unconfigured namer says nothing at all", () => {
  const status = codingSessionNameSuggestStatus({
    enabled: false,
    error: "boom",
    isGenerating: true,
  });
  assert.equal(status.state, "off");
  assert.equal(status.message, null);
});

test("a configured namer that failed says why", () => {
  const status = codingSessionNameSuggestStatus({
    enabled: true,
    error: "The Anthropic API answered 401",
    isGenerating: false,
  });
  assert.equal(status.state, "failed");
  assert.match(status.message, /401/);
});

test("naming in progress outranks a previous failure", () => {
  const status = codingSessionNameSuggestStatus({
    enabled: true,
    error: "stale failure",
    isGenerating: true,
  });
  assert.equal(status.state, "generating");
  assert.doesNotMatch(status.message, /stale/);
});
