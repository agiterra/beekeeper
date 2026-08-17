import assert from "node:assert/strict";
import test from "node:test";

import {
  buildEndCodingSessionStops,
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
