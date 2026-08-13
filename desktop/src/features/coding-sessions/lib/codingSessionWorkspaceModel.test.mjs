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
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [session] }),
    generationId: "generation-1",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, session);
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ entries: [session] }),
      generationId: "generation-2",
    }).kind,
    "missing",
  );
});

test("N=1 identity: a ready single-execution resolution is an umbrella of one over the exact record", () => {
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [session] }),
    generationId: "generation-1",
  });
  assert.equal(resolution.kind, "ready");
  // The routed record is handed to the surface untouched — the single-session
  // tree renders exactly the same catalog record it rendered before Step 4.
  assert.equal(resolution.session, session);
  assert.equal(resolution.umbrella.executions.length, 1);
  assert.equal(resolution.umbrella.sessionRef, null);
  assert.match(resolution.umbrella.umbrellaKey, /^implicit:/);
  assert.equal(resolution.focusedExecution.activeGeneration, session);
});

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function umbrellaMember({
  generationId,
  signerPubkey,
  sessionId,
  generation = 1,
  lastEventAt = "2026-07-30T12:00:00.000Z",
}) {
  return {
    ...session,
    generationId,
    providerAuthorityPubkey: signerPubkey,
    sessionRef: SESSION_REF,
    lastEventAt,
    commandTarget: { ...session.commandTarget, sessionId, generation },
  };
}

test("a shared sessionRef resolves one umbrella with the routed execution focused", () => {
  const claude = umbrellaMember({
    generationId: "generation-claude",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex] }),
    generationId: "generation-codex",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, codex);
  assert.equal(resolution.umbrella.sessionRef, SESSION_REF);
  assert.equal(resolution.umbrella.executions.length, 2);
  assert.equal(resolution.focusedExecution.activeGeneration, codex);
});

test("routing to a collapsed prior generation still resolves the surrounding umbrella", () => {
  const priorGeneration = umbrellaMember({
    generationId: "generation-claude-1",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
    generation: 1,
    lastEventAt: "2026-07-30T10:00:00.000Z",
  });
  const activeGeneration = umbrellaMember({
    generationId: "generation-claude-2",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
    generation: 2,
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [priorGeneration, activeGeneration, codex] }),
    generationId: "generation-claude-1",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, priorGeneration);
  assert.equal(resolution.umbrella.executions.length, 2);
  assert.equal(resolution.focusedExecution.activeGeneration, activeGeneration);
  assert.deepEqual(
    resolution.focusedExecution.priorGenerations.map(
      (record) => record.generationId,
    ),
    ["generation-claude-1"],
  );
});

test("the catalog's create observations resolve founder, operator, and foreign attachments", () => {
  const founder = "f".repeat(64);
  const teammate = "e".repeat(64);
  const claude = umbrellaMember({
    generationId: "generation-claude",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const creates = [
    {
      sessionRef: SESSION_REF,
      signerPubkey: founder,
      createdAt: 1_800_000_000,
      eventId: "event-a",
      target: claude.commandTarget,
    },
    {
      sessionRef: SESSION_REF,
      signerPubkey: teammate,
      createdAt: 1_800_000_100,
      eventId: "event-b",
      target: codex.commandTarget,
    },
  ];

  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex], creates }),
    generationId: "generation-codex",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.umbrella.founderPubkey, founder);
  // The teammate's execution is flagged, not merged and not hidden.
  assert.equal(resolution.umbrella.foreignAttachmentCount, 1);
  assert.equal(resolution.focusedExecution.operatorPubkey, teammate);

  // Same catalog with the observations still in flight (relay gap, cold
  // start): authority is unknown, so nothing is gated and nothing is accused.
  const unobserved = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex] }),
    generationId: "generation-codex",
  });
  assert.equal(unobserved.umbrella.founderPubkey, null);
  assert.equal(unobserved.umbrella.foreignAttachmentCount, 0);
  assert.equal(unobserved.focusedExecution.operatorPubkey, null);
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
