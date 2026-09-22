import assert from "node:assert/strict";
import test from "node:test";

import { hasMention } from "./hasMention.ts";
import { MENTION_DISAMBIGUATOR } from "./mentionDisambiguator.ts";

// Agent names are unique per project rather than per computer (ledger 246), so
// one channel can hold two agents called `Builder`. The composer files the
// second under `Builder·<key head>` so the text names exactly one of them.
//
// The separator is the whole trick, and getting it wrong fails silently: the
// draft would emit a `p` tag for both agents and nobody would see an error.

const SECOND = `Builder${MENTION_DISAMBIGUATOR}bbbbbbbb`;

test("a disambiguated mention is findable in the draft text", () => {
  assert.equal(hasMention(`hey @${SECOND} please look`, SECOND), true);
});

test("the bare name is NOT matchable inside a disambiguated one", () => {
  assert.equal(
    hasMention(`hey @${SECOND} please look`, "Builder"),
    false,
    "otherwise mentioning the second Builder would tag the first one too",
  );
});

test("a space-and-bracket separator would have done exactly that", () => {
  assert.equal(
    hasMention("hey @Builder (bbbbbbbb) please look", "Builder"),
    true,
    "pinned so nobody 'tidies' the separator back into brackets",
  );
});

test("the bare name still matches when it stands alone", () => {
  assert.equal(hasMention("hey @Builder please look", "Builder"), true);
});
