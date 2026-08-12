import assert from "node:assert/strict";
import test from "node:test";

import { resolveChannelCodingSessionIngress } from "./channelCodingSessionIngress.ts";

const idleSession = {
  generationId: "instance:seat:2",
  label: "Session worker-a / generation 2",
  title: "Coding session",
  lastEventAt: "2026-07-30T12:00:00.000Z",
  status: "unknown",
  transcript: [
    {
      id: "result-2",
      type: "lifecycle",
      title: "Turn result",
      text: "completed",
      timestamp: "2026-07-30T12:00:00.000Z",
    },
  ],
  conflictCount: 0,
  commandTarget: null,
};

const workingSession = {
  ...idleSession,
  generationId: "instance:seat:3",
  label: "Session worker-a / generation 3",
  transcript: [
    {
      id: "status-3",
      type: "lifecycle",
      title: "Status",
      text: "streaming",
      timestamp: "2026-07-30T12:01:00.000Z",
    },
  ],
};

function catalog(overrides = {}) {
  return {
    channelId: "channel-a",
    entries: [idleSession, workingSession],
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    ...overrides,
  };
}

test("channel ingress exposes every exact trusted generation with a human status", () => {
  const entries = resolveChannelCodingSessionIngress({
    activeChannelId: "channel-a",
    catalog: catalog(),
  });

  assert.deepEqual(
    entries.map(({ session, status }) => ({
      generationId: session.generationId,
      label: session.label,
      status: status.label,
    })),
    [
      {
        generationId: "instance:seat:2",
        label: "Session worker-a / generation 2",
        status: "Idle",
      },
      {
        generationId: "instance:seat:3",
        label: "Session worker-a / generation 3",
        status: "Working",
      },
    ],
  );
});

test("channel ingress never falls back across a channel transition", () => {
  assert.deepEqual(
    resolveChannelCodingSessionIngress({
      activeChannelId: "channel-b",
      catalog: catalog(),
    }),
    [],
  );
  assert.deepEqual(
    resolveChannelCodingSessionIngress({
      activeChannelId: null,
      catalog: catalog(),
    }),
    [],
  );
});

test("channel ingress fails closed when projection authority is invalid", () => {
  assert.deepEqual(
    resolveChannelCodingSessionIngress({
      activeChannelId: "channel-a",
      catalog: catalog({
        authorityErrorMessage: "Trusted bridge configuration is invalid.",
      }),
    }),
    [],
  );
});
