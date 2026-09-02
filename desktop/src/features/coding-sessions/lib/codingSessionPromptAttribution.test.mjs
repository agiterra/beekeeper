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
  // key. Named when a hire record vouches for the execution AND the prompt's
  // own signer is that host's key AND it carries no 44220 command id.
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      currentUserPubkey: LOCAL,
      hireDispatch: { label: "Keystone · Lead", hostPubkey: LOCAL },
      operatorPubkey: LOCAL,
    }),
    "Keystone · Lead · via your Desktop",
  );
  // Unnamed when the vouch names no requester — still never plain "You".
  const anonymous = resolveCodingSessionPromptAuthor({
    currentUserPubkey: LOCAL,
    hireDispatch: { label: null, hostPubkey: LOCAL },
    operatorPubkey: LOCAL,
  });
  assert.equal(anonymous.label, "Your Desktop (hire host)");
  assert.equal(anonymous.kind, "hire-host");
});

test("N1: the dispatch never outranks the signer, and text is never consulted", () => {
  // The three ways this branch must NOT fire, each of them a signed fact the
  // first attempt ignored in favour of a `[From the lead] ` text prefix.
  const HOST = "a1".repeat(32);
  const OTHER = "b2".repeat(32);
  const vouch = { label: "Keystone", hostPubkey: HOST };

  // 1. A different signer. The vouch is about the execution; the signer is
  //    about this event, and the signer wins.
  assert.equal(
    resolveCodingSessionPromptAuthor({
      hireDispatch: vouch,
      operatorPubkey: OTHER,
      currentUserPubkey: OTHER,
    }).kind,
    "you",
  );
  // 2. A later turn to the same seat: a 44220 command carries a command id,
  //    the create's initial turn does not.
  assert.equal(
    resolveCodingSessionPromptAuthor({
      commandId: "csc-later-turn",
      hireDispatch: vouch,
      operatorPubkey: HOST,
    }).kind,
    "operator",
  );
  // 3. No vouch at all — nothing about the words may create one.
  assert.equal(
    resolveCodingSessionPromptAuthor({
      operatorPubkey: HOST,
    }).kind,
    "operator",
  );
  // And the resolver takes no `text` at all any more: passing one changes
  // nothing, because nothing reads it.
  assert.equal(
    resolveCodingSessionPromptAuthor({
      operatorPubkey: OTHER,
      currentUserPubkey: OTHER,
      text: "[From the lead] Take the badge lane.",
    }).kind,
    "you",
  );
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

// ── The hire host's dispatch, attributed from the wire ───────────────────────

test("a hired seat's brief is attributed to the lead that asked for it", async () => {
  const {
    codingSessionHireDispatchLabelForSeat,
    resolveCodingSessionPromptAuthorLabel,
  } = await import("./codingSessionPromptAttribution.ts");
  const host = "ff".repeat(32);
  const lead = "a1".repeat(32);
  const actor = "cd".repeat(32);
  // The join, as the hire host recorded it: this actor, in this umbrella, and
  // the key that signed the create it answered with.
  const vouch = codingSessionHireDispatchLabelForSeat(
    [
      {
        sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        seatActor: actor,
        requesterLabel: "Keystone",
        hostPubkey: host,
      },
    ],
    { actorPubkey: actor, sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10" },
  );
  assert.deepEqual(vouch, { label: "Keystone", hostPubkey: host });
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      hireDispatch: vouch,
      operatorPubkey: host,
    }),
    "Keystone · via your Desktop",
  );
  void lead;
});

test("N1: the seat join needs the session too, not the actor alone", async () => {
  const { codingSessionHireDispatchLabelForSeat } = await import(
    "./codingSessionPromptAttribution.ts"
  );
  const actor = "cd".repeat(32);
  const outcomes = [
    {
      sessionRef: "11111111-1111-4111-8111-111111111111",
      seatActor: actor,
      requesterLabel: "Keystone",
      hostPubkey: "ff".repeat(32),
    },
    {
      sessionRef: "22222222-2222-4222-8222-222222222222",
      seatActor: actor,
      requesterLabel: "Parallax",
      hostPubkey: "ff".repeat(32),
    },
  ];
  // One identity hired into two umbrellas by two different leads. Joining on
  // the actor alone showed the newer requester's name on both.
  assert.equal(
    codingSessionHireDispatchLabelForSeat(outcomes, {
      actorPubkey: actor,
      sessionRef: "11111111-1111-4111-8111-111111111111",
    }).label,
    "Keystone",
  );
  assert.equal(
    codingSessionHireDispatchLabelForSeat(outcomes, {
      actorPubkey: actor,
      sessionRef: "22222222-2222-4222-8222-222222222222",
    }).label,
    "Parallax",
  );
  // A session this host answered no hire for gets no vouch at all.
  assert.equal(
    codingSessionHireDispatchLabelForSeat(outcomes, {
      actorPubkey: actor,
      sessionRef: "33333333-3333-4333-8333-333333333333",
    }),
    null,
  );
  // And no host key means no vouch: without it nothing could require the
  // signer to be this computer's own operator.
  assert.equal(
    codingSessionHireDispatchLabelForSeat(
      [{ sessionRef: "s", seatActor: actor, requesterLabel: "X" }],
      { actorPubkey: actor, sessionRef: "s" },
    ),
    null,
  );
});

test("a hire naming somebody other than its signer is disclosed, not believed", async () => {
  const { codingSessionHireRequesterStanding } = await import(
    "./codingSessionHireWire.ts"
  );
  const { codingSessionHireRequesterLabel } = await import(
    "./codingSessionHireSeat.ts"
  );
  const { resolveCodingSessionPromptAuthorLabel } = await import(
    "./codingSessionPromptAttribution.ts"
  );
  const signer = "a1".repeat(32);
  const claimed = "b2".repeat(32);
  const host = "ff".repeat(32);
  const standing = codingSessionHireRequesterStanding({
    requesterPubkey: signer,
    action: { requestedBy: claimed },
  });
  assert.equal(standing.kind, "disputed");
  const label = codingSessionHireRequesterLabel({
    standing,
    nameFor: (pubkey) => (pubkey === claimed ? "Keystone" : null),
  });
  // Both keys are on screen, and the word "unverified" is on the label — a
  // forged attribution printed as a name is the failure this closes.
  assert.match(label, /a1a1a1a1…a1a1/);
  assert.match(label, /Keystone/);
  assert.match(label, /unverified attribution/);
  assert.match(
    resolveCodingSessionPromptAuthorLabel({
      hireDispatch: { label, hostPubkey: host },
      operatorPubkey: host,
    }),
    /\(unverified attribution\) · via your Desktop$/,
  );
});

test("a hire that claimed no requester is unknown, never mismatched", async () => {
  const { codingSessionHireRequesterStanding } = await import(
    "./codingSessionHireWire.ts"
  );
  const { codingSessionHireRequesterLabel } = await import(
    "./codingSessionHireSeat.ts"
  );
  const signer = "a1".repeat(32);
  const standing = codingSessionHireRequesterStanding({
    requesterPubkey: signer,
    action: {},
  });
  assert.equal(standing.kind, "unclaimed");
  assert.equal(
    codingSessionHireRequesterLabel({ standing, nameFor: () => null }),
    "A seat (a1a1a1a1…a1a1)",
  );
});

test("a create naming no hire falls back rather than guessing a lead", async () => {
  const {
    CODING_SESSION_HIRE_HOST_AUTHOR_LABEL,
    resolveCodingSessionPromptAuthorLabel,
  } = await import("./codingSessionPromptAttribution.ts");
  // Batch 1 item 10's fallback survives for exactly the case it was written
  // for: a vouch that names no requester. With no vouch at all there is no
  // hire-host branch to take, and the byline is the signer's.
  assert.equal(
    resolveCodingSessionPromptAuthorLabel({
      hireDispatch: { label: null, hostPubkey: "ff".repeat(32) },
      operatorPubkey: "ff".repeat(32),
    }),
    CODING_SESSION_HIRE_HOST_AUTHOR_LABEL,
  );
});
