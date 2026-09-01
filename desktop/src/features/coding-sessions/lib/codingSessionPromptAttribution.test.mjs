import assert from "node:assert/strict";
import test from "node:test";

import {
  collectForeignOperatorPubkeys,
  isCodingSessionTeamWakeCommandId,
  normalizeOperatorPubkey,
  resolveCodingSessionPromptAuthor,
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

test("item 10c: a prompt with no attribution says so instead of guessing You", () => {
  // This used to read "You". A person's own typed turn always carries their
  // stamp, so "You" over an unstamped prompt was a guess — and one that put
  // the reader's name on words they may never have written.
  for (const operatorPubkey of [undefined, null, "garbage"]) {
    const author = resolveCodingSessionPromptAuthor({
      currentUserPubkey: LOCAL,
      operatorPubkey,
    });
    assert.equal(author.label, "Operator not recorded");
    assert.equal(author.kind, "unrecorded");
    assert.equal(author.executionKey, null);
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

// ---------------------------------------------------------------------------
// Item 10 (batch 2026-09-01): "You" is reserved for a prompt the reader typed.
//
// Brian saw session messages that looked like they came from him. Three
// different causes, all landing on the same wrong word.
// ---------------------------------------------------------------------------

const SEAT = `d${"e".repeat(63)}`;

const resolveSeat = (pubkey) =>
  pubkey === SEAT
    ? { label: "Keystone · Lead", executionKey: "exec-keystone" }
    : null;

test("item 10a: a seat's turn is that seat, never You and never a bare key", () => {
  // The observed case: one seat sending a turn to another. The prompt is
  // stamped with the SEAT's actor key, so it is not the viewer's — but with no
  // seat resolver it rendered as a truncated hash, and if the viewer happened
  // to hold that key it would have read "You".
  const author = resolveCodingSessionPromptAuthor({
    currentUserPubkey: LOCAL,
    operatorPubkey: SEAT,
    resolveSeat,
  });
  assert.equal(author.label, "Keystone · Lead");
  assert.equal(author.kind, "seat");
  assert.equal(author.executionKey, "exec-keystone");

  // Even when the viewer's own key IS the seat's actor key, the seat wins:
  // the words came from the seat, not from the person reading.
  const selfSeated = resolveCodingSessionPromptAuthor({
    currentUserPubkey: SEAT,
    operatorPubkey: SEAT,
    resolveSeat,
  });
  assert.equal(selfSeated.label, "Keystone · Lead");
  assert.equal(selfSeated.kind, "seat");

  // A known seat with no resolved name still is not "You" — it falls back to
  // the truncated key rather than to the reader.
  const unnamed = resolveCodingSessionPromptAuthor({
    currentUserPubkey: SEAT,
    operatorPubkey: SEAT,
    resolveSeat: () => ({ label: null, executionKey: "exec-x" }),
  });
  assert.notEqual(unnamed.label, "You");
  assert.equal(unnamed.kind, "seat");
});

test("item 10b: automatic founder-signed commands are not You", () => {
  // Desktop signs its fallback wake with the founder's key, so operatorPubkey
  // === currentUserPubkey and the old rule said "You" about a message the
  // founder never wrote. The command id is the fact that gives it away.
  for (const commandId of [
    `team-wake-v1:${"9".repeat(64)}:${"a".repeat(24)}`,
    `team-wake-v1:${"9".repeat(64)}:${"a".repeat(24)}:r1`,
    "team-wake-2abd9f9a",
  ]) {
    assert.ok(isCodingSessionTeamWakeCommandId(commandId));
    const author = resolveCodingSessionPromptAuthor({
      commandId,
      currentUserPubkey: LOCAL,
      operatorPubkey: LOCAL,
    });
    assert.equal(author.label, "Beekeeper · team wake");
    assert.equal(author.kind, "team-wake");
  }
  // Prose commands are untouched.
  assert.equal(isCodingSessionTeamWakeCommandId("csl-53c9515e"), false);
  assert.equal(isCodingSessionTeamWakeCommandId(undefined), false);

  // The hire host dispatches a brief a LEAD wrote, signed with the founder's
  // key. Named when the signed hire evidence names the hiring seat.
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      hiringSeatLabel: "Keystone · Lead",
      isHireHostDispatch: true,
      operatorPubkey: LOCAL,
    }),
    "Keystone · Lead · via your Desktop",
  );
  // Unnamed when it does not — still never plain "You".
  const anonymous = resolveCodingSessionPromptAuthor({
    currentUserPubkey: LOCAL,
    isHireHostDispatch: true,
    operatorPubkey: LOCAL,
  });
  assert.equal(anonymous.label, "Your Desktop (hire host)");
  assert.equal(anonymous.kind, "hire-host");
});

test("item 10: You survives exactly where it is true", () => {
  const typed = resolveCodingSessionPromptAuthor({
    currentUserPubkey: LOCAL,
    operatorPubkey: LOCAL,
    resolveSeat,
  });
  assert.equal(typed.label, "You");
  assert.equal(typed.kind, "you");

  // A prose command id does not demote a turn the reader really typed.
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      commandId: "csl-53c9515e",
      currentUserPubkey: LOCAL,
      operatorPubkey: LOCAL,
    }),
    "You",
  );

  // Another human operator is still named, not "You".
  const other = resolveCodingSessionPromptAuthor({
    currentUserPubkey: LOCAL,
    operatorPubkey: FOREIGN,
    profiles: { [FOREIGN]: { displayName: "Dana", nip05Handle: null } },
    resolveSeat,
  });
  assert.equal(other.label, "Dana");
  assert.equal(other.kind, "operator");
});
