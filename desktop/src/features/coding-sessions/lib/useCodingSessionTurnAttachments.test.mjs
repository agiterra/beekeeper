import assert from "node:assert/strict";
import test from "node:test";

import {
  PASTED_TEXT_MAX_INLINE_BYTES,
  PASTED_TEXT_MAX_INLINE_LINES,
  attachmentLabel,
  attachmentOrdinal,
  attachmentToken,
  expandAttachmentTokens,
  formatAttachmentSize,
  pastedTextLineCount,
  renumberDraftAfterRemoval,
  shouldAttachPastedText,
} from "./useCodingSessionTurnAttachments.ts";

const sha = (n) => String(n).repeat(64).slice(0, 64);

const image = (n) => ({
  id: n,
  kind: "image",
  filename: `shot-${n}.png`,
  previewUrl: `blob:${n}`,
  sha256: sha(n),
  mime: "image/png",
  size: 10,
  url: `http://relay/media/${sha(n)}.png`,
});

const pasted = (n) => ({
  id: n,
  kind: "text",
  lineCount: 40,
  sha256: sha(n),
  mime: "text/plain",
  size: 4096,
  url: `http://relay/media/${sha(n)}.txt`,
});

test("tokens are one-based and name their kind", () => {
  assert.equal(attachmentToken("image", 1), "[Image #1]");
  assert.equal(attachmentToken("image", 3), "[Image #3]");
  assert.equal(attachmentToken("text", 1), "[Pasted text #1]");
});

test("each kind is numbered within itself, not across the strip", () => {
  // An image then a paste: the paste is the person's first, so it must read
  // "#1". One shared counter would have called it "#2", a number matching
  // nothing they can see.
  const staged = [image(1), pasted(2), image(3)];
  assert.equal(attachmentOrdinal(staged, 0), 1);
  assert.equal(attachmentOrdinal(staged, 1), 1);
  assert.equal(attachmentOrdinal(staged, 2), 2);
});

test("a paste's name follows its current position, not its staging order", () => {
  const staged = [pasted(1), pasted(2)];
  assert.equal(attachmentLabel(staged, 0), "pasted-text-1.txt");
  assert.equal(attachmentLabel(staged, 1), "pasted-text-2.txt");
  // The first one removed: what was #2 is now #1, and its filename has to
  // follow, or the transcript link and the token disagree.
  assert.equal(attachmentLabel([pasted(2)], 0), "pasted-text-1.txt");
  assert.equal(attachmentLabel([image(7)], 0), "shot-7.png");
});

test("tokens expand to the markdown the transcript renders", () => {
  const staged = [image(1), pasted(2)];
  assert.equal(
    expandAttachmentTokens(
      "I see this: [Image #1] and the log is [Pasted text #1]",
      staged,
    ),
    `I see this: ![image](${staged[0].url}) and the log is [pasted-text-1.txt](${staged[1].url})`,
  );
});

test("a token with no attachment behind it is left as prose", () => {
  // Someone writing "[Image #7]" in a sentence is writing a sentence.
  assert.equal(
    expandAttachmentTokens("see [Image #7] in the docs", [image(1)]),
    "see [Image #7] in the docs",
  );
  assert.equal(
    expandAttachmentTokens("as in [Pasted text #2]", [pasted(1)]),
    "as in [Pasted text #2]",
  );
});

test("an unsettled attachment does not expand to a broken reference", () => {
  // Send is gated on uploads settling, so this should be unreachable — but a
  // half-written `![image]()` would render as a broken image in the history.
  const pending = { ...image(1), sha256: undefined, url: undefined };
  assert.equal(
    expandAttachmentTokens("here: [Image #1]", [pending]),
    "here: [Image #1]",
  );
});

test("removing an image renumbers the ones after it", () => {
  // Three staged, the middle one removed: the reader must not be left with
  // "[Image #1] … [Image #3]" beside a strip showing two thumbnails.
  const staged = [image(1), image(2), image(3)];
  assert.equal(
    renumberDraftAfterRemoval(
      "first [Image #1] second [Image #2] third [Image #3]",
      staged,
      1,
    ),
    "first [Image #1] second third [Image #2]",
  );
});

test("removing the first image shifts every later token down one", () => {
  assert.equal(
    renumberDraftAfterRemoval(
      "a [Image #1] b [Image #2] c [Image #3]",
      [image(1), image(2), image(3)],
      0,
    ),
    "a b [Image #1] c [Image #2]",
  );
});

test("renumbering never collides two images onto one token", () => {
  // The ascending pass is what guarantees this: #2→#1 only after #1 is gone.
  const renumbered = renumberDraftAfterRemoval(
    "[Image #1][Image #2][Image #3][Image #4]",
    [image(1), image(2), image(3), image(4)],
    0,
  );
  assert.equal(renumbered, "[Image #1][Image #2][Image #3]");
  const seen = renumbered.match(/\[Image #\d+\]/g) ?? [];
  assert.equal(new Set(seen).size, seen.length, "each token appears once");
});

test("removing one kind leaves the other kind's numbering alone", () => {
  // The whole reason the two are numbered separately: pulling a screenshot out
  // must not renumber the pastes, and vice versa.
  const staged = [image(1), pasted(2), image(3), pasted(4)];
  assert.equal(
    renumberDraftAfterRemoval(
      "[Image #1] [Pasted text #1] [Image #2] [Pasted text #2]",
      staged,
      0,
    ),
    "[Pasted text #1] [Image #1] [Pasted text #2]",
  );
  assert.equal(
    renumberDraftAfterRemoval(
      "[Image #1] [Pasted text #1] [Image #2] [Pasted text #2]",
      staged,
      1,
    ),
    "[Image #1] [Image #2] [Pasted text #1]",
  );
});

test("a draft with no tokens survives a removal untouched", () => {
  assert.equal(
    renumberDraftAfterRemoval("just words", [image(1)], 0),
    "just words",
  );
});

test("a short paste stays in the draft", () => {
  assert.equal(shouldAttachPastedText(""), false);
  assert.equal(shouldAttachPastedText("one line"), false);
  assert.equal(
    shouldAttachPastedText("a\nb\nc\nd\ne"),
    false,
    "five lines is still part of the sentence",
  );
});

test("a trailing newline does not push a five-line paste over the line bound", () => {
  // The case a person would most clearly call wrong: selecting five lines
  // almost always copies the newline after the fifth.
  assert.equal(pastedTextLineCount("a\nb\nc\nd\ne\n"), 5);
  assert.equal(shouldAttachPastedText("a\nb\nc\nd\ne\n"), false);
  assert.equal(shouldAttachPastedText("a\nb\nc\nd\ne\n\n\n"), false);
  assert.equal(pastedTextLineCount(""), 0);
});

test("either bound on its own makes a paste a file", () => {
  const sixLines = "a\n".repeat(PASTED_TEXT_MAX_INLINE_LINES) + "b";
  assert.equal(pastedTextLineCount(sixLines), 6);
  assert.equal(shouldAttachPastedText(sixLines), true);

  // One long line: small in lines, large in bytes.
  const oneLongLine = "x".repeat(PASTED_TEXT_MAX_INLINE_BYTES + 1);
  assert.equal(pastedTextLineCount(oneLongLine), 1);
  assert.equal(shouldAttachPastedText(oneLongLine), true);
  assert.equal(
    shouldAttachPastedText("x".repeat(PASTED_TEXT_MAX_INLINE_BYTES)),
    false,
    "the byte bound is exclusive",
  );
});

test("the byte bound counts UTF-8 bytes, not characters", () => {
  // 301 three-byte characters is 903 bytes but only 301 "length".
  const multibyte = "é".repeat(301);
  assert.equal(multibyte.length <= PASTED_TEXT_MAX_INLINE_BYTES, true);
  assert.equal(shouldAttachPastedText(multibyte), true);
});

test("sizes read in the unit a person would use", () => {
  assert.equal(formatAttachmentSize(612), "612 B");
  assert.equal(formatAttachmentSize(1024), "1 KB");
  assert.equal(formatAttachmentSize(3300), "3 KB");
  assert.equal(formatAttachmentSize(1024 * 1024), "1.0 MB");
});
