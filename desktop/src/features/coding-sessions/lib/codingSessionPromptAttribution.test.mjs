import assert from "node:assert/strict";
import test from "node:test";

import {
  collectForeignOperatorPubkeys,
  normalizeOperatorPubkey,
  resolveCodingSessionPromptAuthorLabel,
} from "./codingSessionPromptAttribution.ts";

const LOCAL = "a".repeat(64);
const FOREIGN = `b${"c".repeat(63)}`;

test("only canonical 64-hex survives normalization", () => {
  assert.equal(normalizeOperatorPubkey(LOCAL), LOCAL);
  assert.equal(normalizeOperatorPubkey(` ${"A".repeat(64)} `), "a".repeat(64));
  for (const bad of [
    undefined,
    null,
    7,
    {},
    "",
    "nope",
    "a".repeat(63),
    "a".repeat(65),
    `z${"a".repeat(63)}`,
  ]) {
    assert.equal(
      normalizeOperatorPubkey(bad),
      null,
      `${String(bad)} must not normalize`,
    );
  }
});

test("the viewer's own prompt stays You, in either case form", () => {
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      operatorPubkey: LOCAL.toUpperCase(),
    }),
    "You",
  );
});

test("a prompt with no attribution stays You", () => {
  // Every transcript published before the provider stamped the operator lands
  // here. There is no fact to render, so the historical label is kept rather
  // than a name being invented.
  for (const operatorPubkey of [undefined, null, "garbage"]) {
    assert.equal(
      resolveCodingSessionPromptAuthorLabel({
        currentUserPubkey: LOCAL,
        operatorPubkey,
      }),
      "You",
    );
  }
});

test("another operator resolves to their name, then handle, then truncated hex", () => {
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      operatorPubkey: FOREIGN,
      profiles: { [FOREIGN]: { displayName: "Dana", nip05Handle: null } },
    }),
    "Dana",
  );
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      operatorPubkey: FOREIGN,
      profiles: { [FOREIGN]: { displayName: null, nip05Handle: "dana@buzz" } },
    }),
    "dana@buzz",
  );
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      operatorPubkey: FOREIGN,
    }),
    "bccccccc…cccc",
  );
});

test("an unknown local identity names the operator instead of guessing You", () => {
  // "You" would be a claim about who is reading; the operator's own label is
  // true for every viewer.
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: null,
      operatorPubkey: FOREIGN,
      profiles: { [FOREIGN]: { displayName: "Dana", nip05Handle: null } },
    }),
    "Dana",
  );
});

test("collecting operators skips the viewer, junk, and duplicates", () => {
  assert.deepEqual(
    collectForeignOperatorPubkeys(
      [
        { operatorPubkey: LOCAL },
        { operatorPubkey: FOREIGN },
        { operatorPubkey: FOREIGN.toUpperCase() },
        { operatorPubkey: "nope" },
        { type: "tool" },
        null,
        "not an item",
      ],
      LOCAL,
    ),
    [FOREIGN],
  );
  assert.deepEqual(collectForeignOperatorPubkeys([], LOCAL), []);
});
