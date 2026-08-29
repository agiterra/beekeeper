import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionStopAll,
  codingSessionStopAllDescription,
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
  assert.equal(model.liveCount, 3);
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
  assert.equal(model.liveCount, 1);
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
  assert.equal(model.liveCount, 1);
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
  assert.equal(codingSessionStopAllTitle(1), "Stop 1 live seat?");
  assert.equal(codingSessionStopAllTitle(3), "Stop 3 live seats?");
  const description = codingSessionStopAllDescription(3);
  assert.match(description, /session stays open/);
  assert.match(description, /cannot be resumed/);
  assert.doesNotMatch(description, /can be resumed/);
});
