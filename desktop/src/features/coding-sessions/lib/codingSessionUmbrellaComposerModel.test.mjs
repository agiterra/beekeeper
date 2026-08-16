import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionUmbrellaParticipantKey,
  defaultCodingSessionUmbrellaParticipantKey,
  resolveCodingSessionUmbrellaComposerAuthority,
} from "./codingSessionUmbrellaComposerModel.ts";
import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel.ts";

const FOUNDER = "f".repeat(64);
const TEAMMATE = "e".repeat(64);
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);

function executionParticipant(executionKey, lastEventAt) {
  return {
    kind: "execution",
    executionKey,
    label: executionKey,
    execution: {
      executionKey,
      signerPubkey: "a".repeat(64),
      activeGeneration: { lastEventAt },
      priorGenerations: [],
      operatorPubkey: null,
    },
  };
}

test("the founder may prompt executions; everyone else gets an honest disabled reason", () => {
  assert.deepEqual(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey: FOUNDER, genesisRef: GENESIS_REF },
      currentUserPubkey: FOUNDER,
    }),
    {
      canPromptExecutions: true,
      reason: null,
      isUngovernedSession: false,
    },
  );
  const gated = resolveCodingSessionUmbrellaComposerAuthority({
    umbrella: { founderPubkey: FOUNDER, genesisRef: GENESIS_REF },
    currentUserPubkey: TEAMMATE,
  });
  assert.equal(gated.canPromptExecutions, false);
  assert.match(gated.reason, /founder/);
  assert.match(gated.reason, /lane stays open/);
});

test("legacy null-founder sessions stay usable and are marked ungoverned", () => {
  const authority = resolveCodingSessionUmbrellaComposerAuthority({
    umbrella: { founderPubkey: null, genesisRef: null },
    currentUserPubkey: TEAMMATE,
  });
  assert.equal(authority.canPromptExecutions, true);
  assert.equal(authority.isUngovernedSession, true);
});

test("genesis-bearing sessions fail closed while founder or identity is unresolved", () => {
  assert.equal(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey: null, genesisRef: GENESIS_REF },
      currentUserPubkey: TEAMMATE,
    }).canPromptExecutions,
    false,
  );
  assert.equal(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey: FOUNDER, genesisRef: GENESIS_REF },
      currentUserPubkey: null,
    }).canPromptExecutions,
    false,
  );
});

function catalogRecord(sessionId, signerPubkey) {
  return {
    generationId: `generation-${sessionId}`,
    label: "generation 1",
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt: "2026-08-12T10:00:00.000Z",
    status: "running",
    transcript: [],
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "instance",
      sessionId,
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-primary",
    runtime: "claude",
    model: "claude-opus-5",
    capabilities: null,
  };
}

test("gating tracks the umbrella the catalog actually grouped from observed creates", () => {
  // The end-to-end shape production now uses: receipt-joined create
  // observations flow into grouping, and the composer reads the founder they
  // resolved. Without them the same legacy catalog stays permissive and is
  // visibly ungoverned.
  const founded = catalogRecord("claude-session", "a".repeat(64));
  const attached = catalogRecord("codex-session", "b".repeat(64));
  const creates = [
    {
      sessionRef: SESSION_REF,
      signerPubkey: FOUNDER,
      createdAt: 1_800_000_000,
      eventId: "event-a",
      target: founded.commandTarget,
    },
    {
      sessionRef: SESSION_REF,
      signerPubkey: TEAMMATE,
      createdAt: 1_800_000_100,
      eventId: "event-b",
      target: attached.commandTarget,
    },
  ];
  const [umbrella] = groupCodingSessionCatalog([founded, attached], creates);
  assert.equal(umbrella.founderPubkey, FOUNDER);
  assert.equal(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella,
      currentUserPubkey: FOUNDER,
    }).canPromptExecutions,
    true,
  );
  assert.equal(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella,
      currentUserPubkey: TEAMMATE,
    }).canPromptExecutions,
    false,
  );

  const [unobserved] = groupCodingSessionCatalog([founded, attached]);
  assert.equal(
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: unobserved,
      currentUserPubkey: TEAMMATE,
    }).canPromptExecutions,
    true,
  );
});

test("participant keys distinguish executions from the session lane", () => {
  assert.equal(
    codingSessionUmbrellaParticipantKey(executionParticipant("exec-1", "")),
    "execution:exec-1",
  );
  assert.equal(
    codingSessionUmbrellaParticipantKey({
      kind: "session",
      sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      label: "Session",
    }),
    "session",
  );
});

test("the default participant is the most recently active execution, never the lane", () => {
  const participants = [
    executionParticipant("exec-old", "2026-08-12T09:00:00.000Z"),
    executionParticipant("exec-live", "2026-08-12T11:00:00.000Z"),
    {
      kind: "session",
      sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      label: "Session",
    },
  ];
  assert.equal(
    defaultCodingSessionUmbrellaParticipantKey(participants),
    "execution:exec-live",
  );
  assert.equal(defaultCodingSessionUmbrellaParticipantKey([]), null);
});
