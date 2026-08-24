import assert from "node:assert/strict";
import test from "node:test";

import {
  buildEndCodingSessionStops,
  endCodingSessionDialogDescription,
  publishEndCodingSessionRequest,
} from "./endCodingSessionModel.ts";

const TARGET_A = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_B = {
  driver: "codex-acp",
  instanceId: "fedcba9876543210",
  sessionId: "99999999-8888-7777-6666-555555555555",
  generation: 2,
};

function entry(overrides = {}) {
  return {
    placement: "unassigned",
    projectId: null,
    placedBy: null,
    channelId: "channel-1",
    generationId: "gen-1",
    label: "Testing 2",
    sourceChannelLabel: null,
    runtimeLabel: null,
    runtimeLabels: [],
    executionCount: 1,
    status: { kind: "idle", label: "Idle" },
    stopTargets: [
      { target: TARGET_A, providerAuthorityPubkey: "a".repeat(64) },
    ],
    session: {},
    ...overrides,
  };
}

test("a live row maps to one stop per execution under the row's label", () => {
  const request = buildEndCodingSessionStops(
    entry({
      stopTargets: [
        { target: TARGET_A, providerAuthorityPubkey: "a".repeat(64) },
        { target: TARGET_B, providerAuthorityPubkey: "b".repeat(64) },
      ],
    }),
  );
  assert.equal(request.label, "Testing 2");
  assert.equal(request.channelId, "channel-1");
  assert.equal(request.stops.length, 2);
});

test("an ended row, or one with nothing to stop, produces no request", () => {
  assert.equal(
    buildEndCodingSessionStops(
      entry({ status: { kind: "ended", label: "Ended" } }),
    ),
    null,
  );
  assert.equal(buildEndCodingSessionStops(entry({ stopTargets: [] })), null);
});

test("the publish fans out with fresh command ids and survives a sibling failure", async () => {
  const published = [];
  const result = await publishEndCodingSessionRequest(
    {
      label: "Testing 2",
      channelId: "channel-1",
      stops: [
        { target: TARGET_A, providerAuthorityPubkey: "a".repeat(64) },
        { target: TARGET_B, providerAuthorityPubkey: "b".repeat(64) },
      ],
    },
    async (input) => {
      published.push(input);
      if (input.target === TARGET_B) {
        throw new Error("provider rejected the stop");
      }
    },
  );
  assert.equal(published.length, 2, "a failure must not abort the sibling");
  assert.notEqual(
    published[0].commandId,
    published[1].commandId,
    "each stop is its own durable command",
  );
  assert.equal(result.ok, false);
  assert.match(result.errorMessage, /provider rejected the stop/);

  const clean = await publishEndCodingSessionRequest(
    {
      label: "Testing 2",
      channelId: "channel-1",
      stops: [{ target: TARGET_A, providerAuthorityPubkey: "a".repeat(64) }],
    },
    async () => {},
  );
  assert.deepEqual(clean, { ok: true, errorMessage: null });
});

// §2 item 42 — the dialog told people the session stays open but never that
// the execution is gone for good, so they waited for a Resume button that does
// not exist. And a stop aimed at a provider that quit sat unacknowledged for
// two hours before draining.
test("the confirm says a stopped execution cannot be resumed", () => {
  const copy = endCodingSessionDialogDescription({
    label: "Testing1",
    channelId: "channel-1",
    stops: [],
  });
  assert.match(copy, /cannot be resumed/);
  assert.match(copy, /add a provider to the session/);
  assert.match(copy, /durable session and its transcript stay open/);
  assert.doesNotMatch(copy, /No provider is answering/);
});

test("a stop with nothing listening is described as a request, not an effect", () => {
  const copy = endCodingSessionDialogDescription({
    label: "Testing1",
    channelId: "channel-1",
    stops: [],
    providerUnanswered: true,
  });
  assert.match(copy, /No provider is answering for it right now/);
  assert.match(copy, /stays on the relay and runs whenever one returns/);
});

test("no request, no copy", () => {
  assert.equal(endCodingSessionDialogDescription(null), "");
});
