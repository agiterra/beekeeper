import assert from "node:assert/strict";
import test from "node:test";

import {
  expandImageTokens,
  imageToken,
  renumberDraftAfterRemoval,
} from "./useCodingSessionImageAttachments.ts";

const attachment = (n) => ({
  id: n,
  filename: `shot-${n}.png`,
  previewUrl: `blob:${n}`,
  sha256: String(n).repeat(64).slice(0, 64),
  mime: "image/png",
  size: 10,
  url: `http://relay/media/${String(n).repeat(64).slice(0, 64)}.png`,
});

test("tokens are one-based, matching what the thumbnail strip shows", () => {
  assert.equal(imageToken(0), "[Image #1]");
  assert.equal(imageToken(2), "[Image #3]");
});

test("tokens expand to the markdown the transcript renders", () => {
  const attachments = [attachment(1), attachment(2)];
  const expanded = expandImageTokens(
    "When I do X, I see this: [Image #1] But I want: [Image #2]",
    attachments,
  );
  assert.equal(
    expanded,
    `When I do X, I see this: ![image](${attachments[0].url}) But I want: ![image](${attachments[1].url})`,
  );
});

test("a token with no attachment behind it is left as prose", () => {
  // Someone writing "[Image #7]" in a sentence is writing a sentence.
  assert.equal(
    expandImageTokens("see [Image #7] in the docs", [attachment(1)]),
    "see [Image #7] in the docs",
  );
});

test("an unsettled attachment does not expand to a broken reference", () => {
  // Send is gated on uploads settling, so this should be unreachable — but a
  // half-written `![image]()` would render as a broken image in the history.
  const pending = { ...attachment(1), sha256: undefined, url: undefined };
  assert.equal(
    expandImageTokens("here: [Image #1]", [pending]),
    "here: [Image #1]",
  );
});

test("removing an image renumbers the ones after it", () => {
  // Three staged, the middle one removed: the reader must not be left with
  // "[Image #1] … [Image #3]" beside a strip showing two thumbnails.
  const draft = "first [Image #1] second [Image #2] third [Image #3]";
  assert.equal(
    renumberDraftAfterRemoval(draft, 1, 3),
    "first [Image #1] second third [Image #2]",
  );
});

test("removing the first image shifts every later token down one", () => {
  assert.equal(
    renumberDraftAfterRemoval("a [Image #1] b [Image #2] c [Image #3]", 0, 3),
    "a b [Image #1] c [Image #2]",
  );
});

test("removing the last image disturbs no other token", () => {
  assert.equal(
    renumberDraftAfterRemoval("a [Image #1] b [Image #2]", 1, 2),
    "a [Image #1] b",
  );
});

test("renumbering never collides two images onto one token", () => {
  // The ascending pass is what guarantees this: #2→#1 only after #1 is gone.
  const renumbered = renumberDraftAfterRemoval(
    "[Image #1][Image #2][Image #3][Image #4]",
    0,
    4,
  );
  assert.equal(renumbered, "[Image #1][Image #2][Image #3]");
  const seen = renumbered.match(/\[Image #\d+\]/g) ?? [];
  assert.equal(new Set(seen).size, seen.length, "each token appears once");
});

test("a draft with no tokens survives a removal untouched", () => {
  assert.equal(renumberDraftAfterRemoval("just words", 0, 1), "just words");
});
