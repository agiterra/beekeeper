/**
 * Ledger 178(b): a refusal's turn must never read as a person's words, and
 * must never be opened at all when the requester's own `bee sessions hire`
 * could still be reading the same answer off its own poll.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  publishRefusal,
  refuseCodingSessionHireForSeatingFailure,
  refuseCodingSessionHireWithCode,
} from "./codingSessionHireDisclosure.ts";
import {
  CODING_SESSION_HIRE_ANSWER_SYNCHRONOUS_SECONDS,
  CODING_SESSION_HOST_NOTICE_MARKER,
  codingSessionHireAnswerWithinRequesterWindow,
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
function harness({ now, hasTarget = true } = {}) {
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
    now: () => now,
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
      created_at: now,
      tags: input.tags,
      content: input.content,
      sig: "s",
    }),
    disposeSeatWorktree: async () => "the worktree was removed",
  };
  return {
    input,
    deps,
    turns: () =>
      published
        .filter((event) => event.kind === 44220)
        .map((event) => JSON.parse(event.content).action.text),
    notices: () =>
      published
        .filter((event) => event.kind === 9)
        .map((event) => event.content),
  };
}

test("codingSessionHireAnswerWithinRequesterWindow: boundary at exactly the CLI's own wait", () => {
  assert.equal(CODING_SESSION_HIRE_ANSWER_SYNCHRONOUS_SECONDS, 120);
  assert.equal(
    codingSessionHireAnswerWithinRequesterWindow({
      requestCreatedAt: 1000,
      nowSeconds: 1000 + 120,
    }),
    true,
  );
  assert.equal(
    codingSessionHireAnswerWithinRequesterWindow({
      requestCreatedAt: 1000,
      nowSeconds: 1000 + 121,
    }),
    false,
  );
});

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

test("(a) publishRefusal's turn text carries the host-notice marker", async () => {
  // Outside the window, so a turn is actually opened to inspect.
  const host = harness({ now: request().createdAt + 200 });
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
  const [turnText] = host.turns();
  assert.ok(turnText.startsWith("hire refused: HIRE_NO_ROUTE — "));
  assert.ok(turnText.includes(CODING_SESSION_HOST_NOTICE_MARKER));
  const [noticeText] = host.notices();
  assert.ok(noticeText.startsWith(`[${CODING_SESSION_HOST_NOTICE_MARKER}]`));
});

test("(b) a refusal answered inside the requester's wait window publishes the notice and no turn", async () => {
  const req = request();
  // Answered one second after the request — well inside the CLI's 120s wait.
  const host = harness({ now: req.createdAt + 1 });
  await publishRefusal(
    req,
    {
      kind: "refused",
      code: "HIRE_NO_ROUTE",
      reason: "no registry",
      text: "hire refused: HIRE_NO_ROUTE — no registry",
    },
    host.input,
    host.deps,
  );
  assert.deepEqual(host.turns(), []);
  const notices = host.notices();
  assert.equal(notices.length, 1);
  assert.ok(notices[0].includes("hire refused: HIRE_NO_ROUTE — no registry"));
});

test("(c) a refusal published after the window still opens the turn, prefixed", async () => {
  const req = request();
  // Answered well past the CLI's own 120s wait — its caller has already been
  // told `unconfirmed` and has no other way to learn the outcome.
  const host = harness({ now: req.createdAt + 121 });
  await publishRefusal(
    req,
    {
      kind: "refused",
      code: "HIRE_NO_ROUTE",
      reason: "no registry",
      text: "hire refused: HIRE_NO_ROUTE — no registry",
    },
    host.input,
    host.deps,
  );
  const turns = host.turns();
  assert.equal(turns.length, 1);
  assert.ok(turns[0].startsWith("hire refused: HIRE_NO_ROUTE — "));
  assert.ok(turns[0].includes(CODING_SESSION_HOST_NOTICE_MARKER));
  assert.equal(host.notices().length, 1);
});

test("a requester with no live target never opens a turn, window notwithstanding", async () => {
  const req = request();
  const host = harness({ now: req.createdAt + 500, hasTarget: false });
  await publishRefusal(
    req,
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

test("refuseCodingSessionHireWithCode: same window rule, and returns the unmarked structural text", async () => {
  const req = request();
  const withinWindow = harness({ now: req.createdAt + 5 });
  const returned = await refuseCodingSessionHireWithCode(
    req,
    {
      code: "HIRE_CHECKOUT_NOT_RECORDED",
      reason: "set the project's repository folder",
    },
    withinWindow.input,
    withinWindow.deps,
  );
  assert.equal(
    returned,
    "hire refused: HIRE_CHECKOUT_NOT_RECORDED — set the project's repository folder",
  );
  assert.deepEqual(withinWindow.turns(), []);

  const pastWindow = harness({ now: req.createdAt + 300 });
  await refuseCodingSessionHireWithCode(
    req,
    {
      code: "HIRE_CHECKOUT_NOT_RECORDED",
      reason: "set the project's repository folder",
    },
    pastWindow.input,
    pastWindow.deps,
  );
  const turns = pastWindow.turns();
  assert.equal(turns.length, 1);
  assert.ok(turns[0].includes(CODING_SESSION_HOST_NOTICE_MARKER));
});

test("refuseCodingSessionHireForSeatingFailure: disposes the worktree and still marks the answer", async () => {
  const req = request();
  const host = harness({ now: req.createdAt + 400 });
  await refuseCodingSessionHireForSeatingFailure(
    req,
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
  assert.ok(turns[0].startsWith("hire refused: HIRE_SEAT_STAGING_FAILED — "));
  assert.ok(turns[0].includes("git clone failed: ENOENT"));
  assert.ok(turns[0].includes(CODING_SESSION_HOST_NOTICE_MARKER));
});
