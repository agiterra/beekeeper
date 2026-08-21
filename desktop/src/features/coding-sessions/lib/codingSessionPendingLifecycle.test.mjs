import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import {
  PENDING_CODING_SESSION_LIFECYCLE_TTL_MS,
  applyPendingCodingSessionLifecycle,
  pendingCodingSessionLifecycleKey,
} from "./codingSessionPendingLifecycle.ts";

const NOW = 1_755_000_000_000;

function makeSession(overrides = {}) {
  return {
    generationId: "gen-1",
    label: "Real session",
    title: "Real session",
    providerAuthorityPubkey: "authority",
    metadataAuthorityPubkey: "authority",
    lastEventAt: new Date(NOW - 60_000).toISOString(),
    status: "idle",
    transcript: [],
    conflictCount: 0,
    commandTarget: null,
    projectRef: null,
    repoRef: null,
    sessionRef: null,
    provider: null,
    runtime: null,
    model: null,
    capabilities: null,
    ...overrides,
  };
}

function makeEntry(overrides = {}) {
  const session = makeSession(overrides.session ?? {});
  return {
    placement: "unassigned",
    projectId: null,
    placedBy: null,
    channelId: "channel-1",
    generationId: session.generationId,
    label: session.label,
    sourceChannelLabel: null,
    runtimeLabel: null,
    runtimeLabels: [],
    executionCount: 1,
    status: { kind: "idle", label: "Idle" },
    stopTargets: [],
    ...overrides,
    session,
  };
}

const noPlacement = () => ({ projectId: null, placedBy: null });
const projectPlacement = () => ({
  projectId: "project-9",
  placedBy: "project-ref",
});

function makeCreate(overrides = {}) {
  return {
    kind: "create",
    channelId: "channel-1",
    commandId: "cmd-1",
    sessionRef: "umbrella-1",
    title: "My new session",
    projectRef: "30621:owner:general",
    providerAuthorityPubkey: "authority",
    hasInitialTurn: true,
    recordedAt: NOW,
    ...overrides,
  };
}

const STOP_TARGET = {
  driver: "claude",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 1,
};

test("pending create synthesizes a placed working row", () => {
  const result = applyPendingCodingSessionLifecycle(
    [],
    [makeCreate()],
    projectPlacement,
    new Map([["channel-1", "general"]]),
    NOW,
  );
  assert.equal(result.consumedKeys.length, 0);
  assert.equal(result.entries.length, 1);
  const entry = result.entries[0];
  assert.equal(entry.pending, true);
  assert.equal(entry.projectId, "project-9");
  assert.equal(entry.placement, "project");
  assert.equal(entry.label, "My new session");
  assert.equal(entry.sourceChannelLabel, "general");
  assert.deepEqual(entry.status, { kind: "working", label: "Working" });
  assert.equal(entry.stopTargets.length, 0);
  assert.equal(entry.session.sessionRef, "umbrella-1");
});

test("pending create without an initial turn shows Idle", () => {
  const result = applyPendingCodingSessionLifecycle(
    [],
    [makeCreate({ hasInitialTurn: false, title: null })],
    noPlacement,
    new Map(),
    NOW,
  );
  const entry = result.entries[0];
  assert.deepEqual(entry.status, { kind: "idle", label: "Idle" });
  assert.equal(entry.label, "Coding session");
  assert.equal(entry.placement, "unassigned");
});

test("pending create is consumed once a real entry carries its sessionRef", () => {
  const pending = makeCreate();
  const real = makeEntry({ session: { sessionRef: "umbrella-1" } });
  const result = applyPendingCodingSessionLifecycle(
    [real],
    [pending],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.deepEqual(result.consumedKeys, [
    pendingCodingSessionLifecycleKey(pending),
  ]);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0], real);
});

test("a matching sessionRef in another channel does not consume the create", () => {
  const pending = makeCreate();
  const otherChannel = makeEntry({
    channelId: "channel-2",
    session: { sessionRef: "umbrella-1" },
  });
  const result = applyPendingCodingSessionLifecycle(
    [otherChannel],
    [pending],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.equal(result.consumedKeys.length, 0);
  assert.equal(result.entries.length, 2);
});

test("expired records are reported consumed and not applied", () => {
  const pending = makeCreate({
    recordedAt: NOW - PENDING_CODING_SESSION_LIFECYCLE_TTL_MS - 1,
  });
  const result = applyPendingCodingSessionLifecycle(
    [],
    [pending],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.deepEqual(result.consumedKeys, [
    pendingCodingSessionLifecycleKey(pending),
  ]);
  assert.equal(result.entries.length, 0);
});

test("pending stop settles the matching live row immediately", () => {
  const entry = makeEntry({
    stopTargets: [
      { target: STOP_TARGET, providerAuthorityPubkey: "authority" },
    ],
  });
  const result = applyPendingCodingSessionLifecycle(
    [entry],
    [
      {
        kind: "stop",
        channelId: "channel-1",
        targetKey: buildCodingSessionTargetKey(STOP_TARGET),
        providerAuthorityPubkey: "authority",
        recordedAt: NOW,
      },
    ],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.equal(result.consumedKeys.length, 0);
  assert.equal(result.entries.length, 1);
  assert.deepEqual(result.entries[0].status, { kind: "ended", label: "Ended" });
  assert.equal(result.entries[0].stopTargets.length, 0);
});

test("pending stop leaves rows in other channels or targets untouched", () => {
  const otherTarget = makeEntry({
    generationId: "gen-2",
    session: { generationId: "gen-2" },
    stopTargets: [
      {
        target: { ...STOP_TARGET, sessionId: "session-other" },
        providerAuthorityPubkey: "authority",
      },
    ],
  });
  const result = applyPendingCodingSessionLifecycle(
    [otherTarget],
    [
      {
        kind: "stop",
        channelId: "channel-1",
        targetKey: buildCodingSessionTargetKey(STOP_TARGET),
        providerAuthorityPubkey: "authority",
        recordedAt: NOW,
      },
    ],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.deepEqual(result.entries[0].status, { kind: "idle", label: "Idle" });
  assert.equal(result.entries[0].stopTargets.length, 1);
});

test("an already-ended row is not re-overridden by a pending stop", () => {
  const ended = makeEntry({
    status: { kind: "ended", label: "Ended" },
    stopTargets: [
      { target: STOP_TARGET, providerAuthorityPubkey: "authority" },
    ],
  });
  const result = applyPendingCodingSessionLifecycle(
    [ended],
    [
      {
        kind: "stop",
        channelId: "channel-1",
        targetKey: buildCodingSessionTargetKey(STOP_TARGET),
        providerAuthorityPubkey: "authority",
        recordedAt: NOW,
      },
    ],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.equal(result.entries[0], ended);
});

test("a receipt-confirmed target consumes the pending create before metadata", () => {
  const pending = makeCreate({ sessionRef: null });
  // The real row exists from transcripts only: sessionRef is still null,
  // which used to leave the spinner zombie alive until TTL.
  const real = makeEntry({
    session: { sessionRef: null, commandTarget: STOP_TARGET },
  });
  const lifecycleFor = (channelId, commandId, authority) =>
    channelId === "channel-1" &&
    commandId === "cmd-1" &&
    authority === "authority"
      ? { state: "created", target: STOP_TARGET }
      : null;
  const result = applyPendingCodingSessionLifecycle(
    [real],
    [pending],
    noPlacement,
    new Map(),
    NOW,
    lifecycleFor,
  );
  assert.deepEqual(result.consumedKeys, [
    pendingCodingSessionLifecycleKey(pending),
  ]);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0], real);
});

test("a receipt without a matching real row keeps the spinner (atomic handoff)", () => {
  const pending = makeCreate({ sessionRef: null });
  const result = applyPendingCodingSessionLifecycle(
    [],
    [pending],
    noPlacement,
    new Map(),
    NOW,
    () => ({ state: "created", target: STOP_TARGET }),
  );
  assert.equal(result.consumedKeys.length, 0);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0].pending, true);
});

test("a failed or conflicted receipt drops the spinner immediately", () => {
  for (const state of ["failed", "conflict"]) {
    const pending = makeCreate();
    const result = applyPendingCodingSessionLifecycle(
      [],
      [pending],
      noPlacement,
      new Map(),
      NOW,
      () => ({ state }),
    );
    assert.deepEqual(
      result.consumedKeys,
      [pendingCodingSessionLifecycleKey(pending)],
      state,
    );
    assert.equal(result.entries.length, 0, state);
  }
});

test("the sessionRef echo still consumes when no resolver is available", () => {
  const pending = makeCreate();
  const real = makeEntry({ session: { sessionRef: "umbrella-1" } });
  const result = applyPendingCodingSessionLifecycle(
    [real],
    [pending],
    noPlacement,
    new Map(),
    NOW,
  );
  assert.deepEqual(result.consumedKeys, [
    pendingCodingSessionLifecycleKey(pending),
  ]);
});

test("a still-pending resolution keeps the spinner row", () => {
  const pending = makeCreate({ sessionRef: null });
  const result = applyPendingCodingSessionLifecycle(
    [],
    [pending],
    noPlacement,
    new Map(),
    NOW,
    () => ({ state: "pending" }),
  );
  assert.equal(result.consumedKeys.length, 0);
  assert.equal(result.entries.length, 1);
});
