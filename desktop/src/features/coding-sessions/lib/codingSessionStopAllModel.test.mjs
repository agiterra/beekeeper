import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionStopAll,
  codingSessionStopAllButtonLabel,
  codingSessionStopAllDescription,
  codingSessionStopAllSentence,
  codingSessionStopAllTitle,
} from "./codingSessionStopAllModel.ts";

const FOUNDER = "e5".repeat(32);
const STRANGER = "aa".repeat(32);
const PROVIDER = "f0".repeat(32);

function target(sessionId, generation = 1) {
  return {
    driver: "claude-agent-acp",
    instanceId: "inst-1",
    sessionId,
    generation,
  };
}

function execution(overrides = {}) {
  return {
    label: "Keystone (lead)",
    status: { kind: "working" },
    // W1's own answer for this seat — the map the roster chips read.
    live: true,
    target: target("sess-lead"),
    providerAuthorityPubkey: PROVIDER,
    ...overrides,
  };
}

/**
 * Item 87(e), found live 2026-08-28: a team was launched, the lead hired two
 * more seats, and the person who started it had no control anywhere that
 * stopped them.
 */
test("the founder gets one stop per live execution", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution(),
      execution({
        label: "Builder (builder)",
        target: target("sess-builder"),
        status: { kind: "idle" },
      }),
      execution({
        label: "Banksy (designer)",
        target: target("sess-designer"),
        status: { kind: "unknown" },
      }),
    ],
  });
  assert.equal(model.kind, "available");
  assert.equal(model.seatCount, 3);
  assert.deepEqual(
    model.request.stops.map((stop) => stop.target.sessionId),
    ["sess-lead", "sess-builder", "sess-designer"],
  );
  assert.equal(model.request.channelId, "chan-1");
  assert.equal(
    model.request.stops.every(
      (stop) => stop.providerAuthorityPubkey === PROVIDER,
    ),
    true,
  );
});

test("an ended execution is not counted and not stopped", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution(),
      execution({
        label: "Runner (runner)",
        target: target("sess-runner"),
        status: { kind: "ended" },
      }),
    ],
  });
  assert.equal(model.kind, "available");
  assert.equal(model.seatCount, 1);
  assert.deepEqual(
    model.request.stops.map((stop) => stop.target.sessionId),
    ["sess-lead"],
  );
});

/**
 * A confirm that counts a seat it cannot address would say "3 seats" and stop
 * two — the same class of lie as the roster that listed four and created one.
 */
test("an execution with no target or no authority is not counted", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution(),
      execution({ label: "Pending", target: null }),
      execution({
        label: "Untrusted",
        target: target("sess-x"),
        providerAuthorityPubkey: null,
      }),
    ],
  });
  assert.equal(model.seatCount, 1);
  assert.equal(model.request.stops.length, 1);
});

test("a viewer who did not found the session gets no control", () => {
  for (const viewer of [STRANGER, null]) {
    const model = buildCodingSessionStopAll({
      channelId: "chan-1",
      founderPubkey: FOUNDER,
      currentUserPubkey: viewer,
      executions: [execution()],
    });
    assert.equal(model.kind, "hidden");
    assert.match(model.reason, /founded this session/);
  }
});

test("an unresolved founder is not an authority anybody may borrow", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: null,
    currentUserPubkey: FOUNDER,
    executions: [execution()],
  });
  assert.equal(model.kind, "hidden");
  assert.match(model.reason, /founder has not resolved/);
});

test("case never decides authority", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER.toUpperCase(),
    currentUserPubkey: FOUNDER,
    executions: [execution()],
  });
  assert.equal(model.kind, "available");
});

test("nothing live means no control at all", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [execution({ status: { kind: "ended" } })],
  });
  assert.equal(model.kind, "hidden");
  assert.match(model.reason, /nothing to stop/);
});

/**
 * The confirm must not promise a resume this app does not offer: Reconnect is
 * rendered for `disconnected` only, never for a stopped execution.
 */
test("the confirm counts the seats and does not promise a resume", () => {
  // L4.3: the confirm counts seats it can stop, which is not the same set as
  // the seats W1 calls live. It no longer borrows the seat word for the count.
  assert.equal(codingSessionStopAllTitle(1), "Stop 1 seat?");
  assert.equal(codingSessionStopAllTitle(3), "Stop 3 seats?");
  const description = codingSessionStopAllDescription(3);
  assert.match(description, /session stays open/);
  assert.match(description, /cannot be resumed/);
  assert.doesNotMatch(description, /can be resumed/);
});

/**
 * L4.3, seen live 2026-09-01: `Stop all (2)` over one working seat and one
 * idle one, with an aria label reading `Stop 2 live seats`. The count was
 * right and the word was the lie — an idle seat is stoppable, not live.
 */
test("the model splits stoppable seats from the seats W1 calls live", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution(),
      execution({
        label: "Builder (builder)",
        target: target("sess-builder"),
        status: { kind: "idle" },
        live: false,
      }),
    ],
  });

  assert.equal(model.kind, "available");
  assert.equal(model.seatCount, 2);
  assert.equal(model.liveCount, 1);
  assert.equal(model.buttonLabel, "Stop all (2 seats)");
  assert.equal(
    model.sentence,
    "Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.",
  );
});

test("an ended execution is in neither the seat count nor the live count", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution(),
      execution({
        label: "Ghost (runner)",
        target: target("sess-ghost"),
        status: { kind: "ended" },
        // A provider that signed `stopped` can still be the last thing W1
        // answered `working` for; the ended status decides, not the map.
        live: true,
      }),
    ],
  });

  assert.equal(model.seatCount, 1);
  assert.equal(model.liveCount, 1);
  assert.equal(model.buttonLabel, "Stop all (1 seat)");
  assert.equal(model.sentence, "Stop 1 seat — live.");
});

test("no live seat says none live rather than borrowing the word", () => {
  const model = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [
      execution({ live: false }),
      execution({
        label: "Builder (builder)",
        target: target("sess-builder"),
        status: { kind: "idle" },
        live: false,
      }),
    ],
  });

  assert.equal(model.liveCount, 0);
  assert.equal(
    model.sentence,
    "Stop 2 seats — none live. A stopped seat cannot be resumed.",
  );
});

test("the stop-all copy is one function per string, exact", () => {
  assert.equal(codingSessionStopAllButtonLabel(1), "Stop all (1 seat)");
  assert.equal(codingSessionStopAllButtonLabel(2), "Stop all (2 seats)");
  assert.equal(
    codingSessionStopAllSentence({ seatCount: 2, liveCount: 1 }),
    "Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.",
  );
  assert.equal(
    codingSessionStopAllSentence({ seatCount: 2, liveCount: 0 }),
    "Stop 2 seats — none live. A stopped seat cannot be resumed.",
  );
  assert.equal(
    codingSessionStopAllSentence({ seatCount: 2, liveCount: 2 }),
    "Stop 2 seats — 2 live. A stopped seat cannot be resumed.",
  );
  assert.equal(
    codingSessionStopAllSentence({ seatCount: 1, liveCount: 1 }),
    "Stop 1 seat — live.",
  );
  assert.equal(
    codingSessionStopAllSentence({ seatCount: 1, liveCount: 0 }),
    "Stop 1 seat — idle.",
  );
});

/**
 * F6 — L4.3's copy is a Mission ruling, and this dialog is mounted from the
 * workspace, which is both lenses.
 *
 * The masked `outerHTML` baseline is structurally blind to it: the confirm is
 * closed in the dump. So the rule is asserted here instead, on both branches.
 */
test("F6: the confirm's title is Mission's only; Conversation keeps its own", () => {
  assert.equal(codingSessionStopAllTitle(2, true), "Stop 2 seats?");
  assert.equal(codingSessionStopAllTitle(1, true), "Stop 1 seat?");
  assert.equal(codingSessionStopAllTitle(2, false), "Stop 2 live seats?");
  assert.equal(codingSessionStopAllTitle(1, false), "Stop 1 live seat?");
  // The string builder's own default is L4's wording — it is L4's string.
  // `buildCodingSessionStopAll` defaults the *other* way, to Conversation, so
  // a caller that has not adopted the flag renders exactly what it rendered
  // before this lane. The two defaults face opposite directions on purpose.
  assert.equal(codingSessionStopAllTitle(2), "Stop 2 seats?");

  const conversation = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    executions: [execution(), execution({ target: target("sess-builder") })],
  });
  assert.equal(conversation.kind, "available");
  assert.equal(conversation.confirmTitle, "Stop 2 live seats?");
  assert.equal(conversation.request.confirm.title, "Stop 2 live seats?");

  const mission = buildCodingSessionStopAll({
    channelId: "chan-1",
    founderPubkey: FOUNDER,
    currentUserPubkey: FOUNDER,
    mission: true,
    executions: [execution(), execution({ target: target("sess-builder") })],
  });
  assert.equal(mission.kind, "available");
  assert.equal(mission.confirmTitle, "Stop 2 seats?");
  assert.equal(mission.request.confirm.title, "Stop 2 seats?");

  // The same split on the hidden reason, which is prose about liveness too.
  const noneStoppable = {
    executions: [execution({ status: { kind: "ended" } })],
  };
  assert.equal(
    buildCodingSessionStopAll({
      channelId: "chan-1",
      founderPubkey: FOUNDER,
      currentUserPubkey: FOUNDER,
      ...noneStoppable,
    }).reason,
    "No seat in this session is live, so there is nothing to stop.",
  );
  assert.match(
    buildCodingSessionStopAll({
      channelId: "chan-1",
      founderPubkey: FOUNDER,
      currentUserPubkey: FOUNDER,
      mission: true,
      ...noneStoppable,
    }).reason,
    /^Every seat in this session has ended/,
  );
});
