import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveCodingSessionWorkspaceStatus,
  resolveCodingSessionWorkspace,
} from "./codingSessionWorkspaceModel.ts";

const session = {
  generationId: "generation-1",
  label: "Generation 1",
  title: "Coding session",
  lastEventAt: "2026-07-30T12:00:00.000Z",
  status: "unknown",
  transcript: [],
  conflictCount: 0,
  commandTarget: {
    driver: "driver",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  },
};

function catalog(overrides = {}) {
  return {
    channelId: "channel-1",
    entries: [],
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    ...overrides,
  };
}

test("workspace resolves only the exact catalog generation", () => {
  assert.deepEqual(
    resolveCodingSessionWorkspace({
      catalog: catalog({ entries: [session] }),
      generationId: "generation-1",
    }),
    { kind: "ready", session },
  );
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ entries: [session] }),
      generationId: "generation-2",
    }).kind,
    "missing",
  );
});

test("missing exact generation distinguishes loading and invalid authority", () => {
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ isLoading: true }),
      generationId: "generation-1",
    }).kind,
    "loading",
  );
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({
        authorityErrorMessage: "No trusted bridge configured.",
      }),
      generationId: "generation-1",
    }).kind,
    "untrusted",
  );
});

test("unrelated rejected evidence never changes a missing generation to untrusted", () => {
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({
        rejectedAuthorCount: 2,
        invalidSignatureCount: 1,
      }),
      generationId: "generation-missing",
    }).kind,
    "missing",
  );
});

test("workspace status derives from provider-neutral transcript lifecycle", () => {
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([
      {
        id: "status-1",
        type: "lifecycle",
        renderClass: "status",
        title: "Status",
        text: "thinking",
        timestamp: "2026-07-30T12:00:00.000Z",
      },
    ]),
    { kind: "working", label: "Working" },
  );
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([
      {
        id: "result-1",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Done",
        timestamp: "2026-07-30T12:00:00.000Z",
      },
    ]),
    { kind: "idle", label: "Idle" },
  );
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([]), {
    kind: "unknown",
    label: "Status unknown",
  });
});
