/**
 * Ledger 178(b): a refusal's turn must never read as a person's words. It is
 * always published — `bee sessions hire` reads this same event to answer its
 * own poll — so the fix marks the text and tags the wire event, rather than
 * suppressing the publish.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_HOST_ANSWER_TAG_NAME,
  CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE,
} from "./codingSessionCommand.ts";
import {
  publishRefusal,
  refuseCodingSessionHireForSeatingFailure,
  refuseCodingSessionHireWithCode,
} from "./codingSessionHireDisclosure.ts";
import {
  CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE,
  CODING_SESSION_HOST_NOTICE_MARKER,
  markCodingSessionHostNoticeText,
  markCodingSessionHostTurnText,
} from "./codingSessionHireHostNotice.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const LEAD = "1".repeat(64);
const TARGET = {
  driver: "claude",
  instanceId: "claude-primary",
  sessionId: "session-1",
  generation: 1,
};

function request(overrides = {}) {
  return {
    eventId: "e".repeat(64),
    channelId: CHANNEL_ID,
    commandId: "csl-hire-1",
    requesterPubkey: LEAD,
    createdAt: 1_700_000_000,
    action: {
      type: "session.hire",
      sessionRef: SESSION_REF,
      genesisRef: "a".repeat(64),
      role: "builder",
      providerInstanceRef: null,
      model: null,
      brief: "Take the badge lane.",
    },
    ...overrides,
  };
}

/** A minimal input/deps pair recording every publish, newest last. */
function harness({ hasTarget = true } = {}) {
  const published = [];
  const input = {
    agents: [],
    targetForActor: (channelId, actorPubkey) =>
      hasTarget && channelId === CHANNEL_ID && actorPubkey === LEAD
        ? TARGET
        : null,
  };
  const deps = {
    newTurnCommandId: () => "csc-refusal-1",
    now: () => request().createdAt,
    publisher: {
      publishEvent: async (event) => {
        published.push(event);
        return { ...event, id: `event-${published.length}` };
      },
    },
    signer: async (input) => ({
      id: "unsigned",
      kind: input.kind,
      pubkey: "p",
      created_at: 0,
      tags: input.tags,
      content: input.content,
      sig: "s",
    }),
    disposeSeatWorktree: async () => "the worktree was removed",
  };
  return {
    input,
    deps,
    published,
    turns: () => published.filter((event) => event.kind === 44220),
    turnTexts: () =>
      published
        .filter((event) => event.kind === 44220)
        .map((event) => JSON.parse(event.content).action.text),
    notices: () =>
      published
        .filter((event) => event.kind === 9)
        .map((event) => event.content),
  };
}

test("markCodingSessionHostNoticeText: prefixes once, never doubles up", () => {
  const marked = markCodingSessionHostNoticeText("Ada asked to hire a builder");
  assert.ok(marked.startsWith(`[${CODING_SESSION_HOST_NOTICE_MARKER}]`));
  assert.equal(markCodingSessionHostNoticeText(marked), marked);
});

test("markCodingSessionHostTurnText: the hire-refused prefix `bee sessions hire` parses structurally is untouched", () => {
  const text =
    "hire refused: HIRE_NO_ROUTE — nothing offered clears that class";
  const marked = markCodingSessionHostTurnText(text);
  assert.ok(marked.startsWith("hire refused: "));
  assert.ok(marked.includes(CODING_SESSION_HOST_NOTICE_MARKER));
  // Idempotent, so a caller that already marked a text is never marked twice.
  assert.equal(markCodingSessionHostTurnText(marked), marked);
});

test("(a) publishRefusal always opens a turn on the requester, marked, and tagged as a host answer", async () => {
  const host = harness();
  await publishRefusal(
    request(),
    {
      kind: "refused",
      code: "HIRE_NO_ROUTE",
      reason: "no registry",
      text: "hire refused: HIRE_NO_ROUTE — no registry",
    },
    host.input,
    host.deps,
  );
  const [turn] = host.turns();
  assert.ok(
    turn,
    "the requester must always be told — it is the CLI's own answer channel",
  );
  const payload = JSON.parse(turn.content);
  // `bee sessions hire` parses this text structurally: it must still start
  // with the exact `hire refused: ` prefix.
  assert.ok(payload.action.text.startsWith("hire refused: HIRE_NO_ROUTE — "));
  assert.ok(payload.action.text.includes(CODING_SESSION_HOST_NOTICE_MARKER));
  // And the wire itself says this is a host answer, so a provider's turn
  // intake can recognize it without parsing the text at all.
  assert.deepEqual(
    turn.tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    [CODING_SESSION_HOST_ANSWER_TAG_NAME, "hire"],
  );

  const [noticeText] = host.notices();
  assert.ok(noticeText.startsWith(`[${CODING_SESSION_HOST_NOTICE_MARKER}]`));
});

test("a requester with no live target gets no turn, only the umbrella notice", async () => {
  const host = harness({ hasTarget: false });
  await publishRefusal(
    request(),
    {
      kind: "refused",
      code: "HIRE_OFF",
      reason: "hiring is switched off",
      text: "hire refused: HIRE_OFF — hiring is switched off",
    },
    host.input,
    host.deps,
  );
  assert.deepEqual(host.turns(), []);
  assert.equal(host.notices().length, 1);
});

test("refuseCodingSessionHireWithCode: always tags and marks the turn, and returns the unmarked structural text", async () => {
  const host = harness();
  const returned = await refuseCodingSessionHireWithCode(
    request(),
    {
      code: "HIRE_CHECKOUT_NOT_RECORDED",
      reason: "set the project's repository folder",
    },
    host.input,
    host.deps,
  );
  assert.equal(
    returned,
    "hire refused: HIRE_CHECKOUT_NOT_RECORDED — set the project's repository folder",
  );
  const [turn] = host.turns();
  assert.ok(turn);
  assert.deepEqual(
    turn.tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    [CODING_SESSION_HOST_ANSWER_TAG_NAME, "hire"],
  );
  assert.ok(
    JSON.parse(turn.content).action.text.includes(
      CODING_SESSION_HOST_NOTICE_MARKER,
    ),
  );
});

test("refuseCodingSessionHireForSeatingFailure: disposes the worktree and still marks and tags the answer", async () => {
  const host = harness();
  await refuseCodingSessionHireForSeatingFailure(
    request(),
    {
      failure: "git clone failed: ENOENT",
      sessionRef: SESSION_REF,
      seatLabel: "builder-1",
      worktreePath: "/tmp/trees/builder-1",
    },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  assert.equal(turns.length, 1);
  const text = JSON.parse(turns[0].content).action.text;
  assert.ok(text.startsWith("hire refused: HIRE_SEAT_STAGING_FAILED — "));
  assert.ok(text.includes("git clone failed: ENOENT"));
  assert.ok(text.includes(CODING_SESSION_HOST_NOTICE_MARKER));
  assert.deepEqual(
    turns[0].tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    [CODING_SESSION_HOST_ANSWER_TAG_NAME, "hire"],
  );
});

/**
 * Ledger 192: a desktop must work against a relay one release behind. Lane
 * 181 tagged the requester-addressed turn `buzz-host-answer: hire`, but a
 * relay whose ingest allowlist predates that lane refuses the tag itself
 * with `invalid: unsupported coding-session command tag`, so the requester
 * never sees an answer at all — reopening ledger 169. `harnessWithScript`
 * lets a publisher answer differently by call, so these tests can pick that
 * exact rejection apart from any other reason a publish can fail.
 */
function harnessWithScript(script) {
  const published = [];
  const input = {
    agents: [],
    targetForActor: (channelId, actorPubkey) =>
      channelId === CHANNEL_ID && actorPubkey === LEAD ? TARGET : null,
  };
  let call = 0;
  const deps = {
    newTurnCommandId: () => `csc-refusal-${published.length + 1}`,
    now: () => request().createdAt,
    publisher: {
      publishEvent: async (event) => {
        published.push(event);
        const outcome = script[Math.min(call, script.length - 1)];
        call += 1;
        if (outcome instanceof Error) throw outcome;
        return { ...event, id: `event-${published.length}` };
      },
    },
    signer: async (input) => ({
      id: "unsigned",
      kind: input.kind,
      pubkey: "p",
      created_at: 0,
      tags: input.tags,
      content: input.content,
      sig: "s",
    }),
    disposeSeatWorktree: async () => "the worktree was removed",
  };
  return {
    input,
    deps,
    turns: () => published.filter((event) => event.kind === 44220),
    notices: () =>
      published
        .filter((event) => event.kind === 9)
        .map((event) => event.content),
  };
}

const UNSUPPORTED_TAG_REJECTION = new Error(
  `invalid: ${CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE}`,
);

test("(192a) a tagged host-answer turn accepted by the relay is published exactly once", async () => {
  const host = harnessWithScript([null]);
  await refuseCodingSessionHireWithCode(
    request(),
    { code: "HIRE_OFF", reason: "hiring is switched off" },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  assert.equal(turns.length, 1);
  assert.deepEqual(
    turns[0].tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    [CODING_SESSION_HOST_ANSWER_TAG_NAME, "hire"],
  );
  assert.equal(host.notices().length, 1);
  assert.ok(
    !host.notices()[0].includes(CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE),
    "an accepted tagged turn discloses no downgrade",
  );
});

test("(192b) a relay whose allowlist predates the tag gets one untagged retry, and the umbrella notice discloses the downgrade", async () => {
  const host = harnessWithScript([UNSUPPORTED_TAG_REJECTION, null]);
  await refuseCodingSessionHireWithCode(
    request(),
    { code: "HIRE_OFF", reason: "hiring is switched off" },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  // Exactly two publish attempts reached the wire: the refused tagged one,
  // then one untagged retry — never a third.
  assert.equal(turns.length, 2);
  assert.deepEqual(
    turns[0].tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    [CODING_SESSION_HOST_ANSWER_TAG_NAME, "hire"],
  );
  assert.equal(
    turns[1].tags.find((tag) => tag[0] === CODING_SESSION_HOST_ANSWER_TAG_NAME),
    undefined,
    "the retry must not carry the tag the relay just refused",
  );
  const [firstCommandId, secondCommandId] = turns.map(
    (turn) => JSON.parse(turn.content).commandId,
  );
  assert.notEqual(
    firstCommandId,
    secondCommandId,
    "the retry is a fresh command, never the rejected one resent",
  );
  // Same requester-visible text on both, marker included.
  const [firstText, secondText] = turns.map(
    (turn) => JSON.parse(turn.content).action.text,
  );
  assert.equal(firstText, secondText);
  assert.ok(firstText.includes(CODING_SESSION_HOST_NOTICE_MARKER));

  assert.equal(host.notices().length, 1);
  assert.ok(
    host.notices()[0].includes(CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE),
    "the umbrella must disclose that the answer went out untagged",
  );
});

test("(192c) a rejection for any other reason is not retried, and the umbrella notice still publishes", async (t) => {
  const errors = t.mock.method(console, "error", () => {});
  const host = harnessWithScript([new Error("restricted: not a member")]);
  await refuseCodingSessionHireWithCode(
    request(),
    { code: "HIRE_OFF", reason: "hiring is switched off" },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  assert.equal(turns.length, 1, "no untagged retry for an unrelated refusal");
  assert.equal(host.notices().length, 1, "the refusal is never dropped");
  assert.ok(
    !host.notices()[0].includes(CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE),
  );
  assert.equal(errors.mock.callCount(), 1);
  assert.ok(
    String(errors.mock.calls[0].arguments[1]?.message ?? "").includes(
      "restricted: not a member",
    ),
  );
});

test("(192d) when the untagged retry also fails, both messages are logged and the umbrella notice still publishes", async (t) => {
  const errors = t.mock.method(console, "error", () => {});
  const host = harnessWithScript([
    UNSUPPORTED_TAG_REJECTION,
    new Error("error: relay unreachable"),
  ]);
  await refuseCodingSessionHireWithCode(
    request(),
    { code: "HIRE_OFF", reason: "hiring is switched off" },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  assert.equal(turns.length, 2, "the retry happens exactly once, never more");
  // Neither publish landed, so there is nothing to disclose as a downgrade —
  // but the umbrella notice for the refusal itself is never dropped.
  assert.equal(host.notices().length, 1);
  assert.ok(
    !host.notices()[0].includes(CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE),
  );
  assert.equal(errors.mock.callCount(), 1);
  const [, taggedError, untaggedError] = errors.mock.calls[0].arguments;
  assert.ok(
    String(taggedError?.message).includes(
      "unsupported coding-session command tag",
    ),
  );
  assert.ok(String(untaggedError?.message).includes("relay unreachable"));
});
