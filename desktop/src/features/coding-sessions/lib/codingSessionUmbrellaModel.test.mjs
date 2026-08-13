import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionExecutionKey,
  groupCodingSessionCatalog,
  listCodingSessionUmbrellaParticipants,
} from "./codingSessionUmbrellaModel.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);
const FOUNDER = "f".repeat(64);
const TEAMMATE = "e".repeat(64);

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};
const CODEX_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-instance",
  sessionId: "22222222-2222-2222-2222-222222222222",
  generation: 1,
};

function record({
  target = CLAUDE_TARGET,
  signerPubkey = CLAUDE_SIGNER,
  sessionRef = null,
  status = "running",
  lastEventAt = "2026-08-12T10:00:00.000Z",
  title = "Advance Buzz live sessions",
  conflictCount = 0,
  runtime = "claude",
  model = "claude-opus-5",
  transcript = [],
} = {}) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 4)}-${target.driver}-${target.sessionId}-${target.generation}`,
    label: `${target.driver} · generation ${target.generation}`,
    title,
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status,
    transcript,
    conflictCount,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: `${runtime}-primary`,
    runtime,
    model,
    capabilities: null,
  };
}

test("records without a sessionRef form implicit umbrellas of one, rendered as today", () => {
  const claude = record();
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    runtime: "codex",
    model: "gpt-5.3-codex",
    lastEventAt: "2026-08-12T09:00:00.000Z",
  });
  const umbrellas = groupCodingSessionCatalog([claude, codex]);

  assert.equal(umbrellas.length, 2);
  for (const umbrella of umbrellas) {
    assert.equal(umbrella.sessionRef, null);
    assert.equal(umbrella.executions.length, 1);
    assert.match(umbrella.umbrellaKey, /^implicit:/);
    assert.equal(umbrella.founderPubkey, null);
    assert.equal(umbrella.foreignAttachmentCount, 0);
  }
  const [first] = umbrellas;
  // Umbrella-of-one surfaces are exactly the record's own facts.
  assert.equal(first.title, claude.title);
  assert.equal(first.status, claude.status);
  assert.equal(first.lastEventAt, claude.lastEventAt);
  assert.equal(first.executions[0].activeGeneration, claude);
  assert.deepEqual(first.executions[0].priorGenerations, []);
  // Ordered by activity descending, like the flat catalog.
  assert.equal(umbrellas[0].executions[0].activeGeneration, claude);
  assert.equal(umbrellas[1].executions[0].activeGeneration, codex);
});

test("executions sharing a sessionRef group into one umbrella, streams never merged", () => {
  const claude = record({ sessionRef: SESSION_REF });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    sessionRef: SESSION_REF,
    runtime: "codex",
    model: "gpt-5.3-codex",
    lastEventAt: "2026-08-12T11:00:00.000Z",
  });
  const umbrellas = groupCodingSessionCatalog([claude, codex]);

  assert.equal(umbrellas.length, 1);
  const [umbrella] = umbrellas;
  assert.equal(umbrella.umbrellaKey, SESSION_REF);
  assert.equal(umbrella.sessionRef, SESSION_REF);
  assert.equal(umbrella.executions.length, 2);
  // Each execution keeps its own signer-attributed record; nothing merges.
  const signers = umbrella.executions.map((e) => e.signerPubkey).sort();
  assert.deepEqual(signers, [CLAUDE_SIGNER, CODEX_SIGNER]);
  assert.equal(umbrella.lastEventAt, "2026-08-12T11:00:00.000Z");
});

test("generations of one execution collapse: highest active, earlier prior, ref carried forward", () => {
  const generationOne = record({
    sessionRef: SESSION_REF,
    lastEventAt: "2026-08-12T08:00:00.000Z",
    conflictCount: 1,
  });
  const generationTwo = record({
    target: { ...CLAUDE_TARGET, generation: 2 },
    // The echo can lag a bump: generation 2's metadata has not arrived.
    sessionRef: null,
    lastEventAt: "2026-08-12T10:00:00.000Z",
    conflictCount: 2,
  });
  const umbrellas = groupCodingSessionCatalog([generationTwo, generationOne]);

  assert.equal(umbrellas.length, 1);
  const [umbrella] = umbrellas;
  assert.equal(umbrella.sessionRef, SESSION_REF);
  assert.equal(umbrella.executions.length, 1);
  const [execution] = umbrella.executions;
  assert.equal(execution.activeGeneration.commandTarget.generation, 2);
  assert.deepEqual(
    execution.priorGenerations.map((r) => r.commandTarget.generation),
    [1],
  );
  assert.equal(umbrella.conflictCount, 3);
});

test("umbrella status derives running > waiting_for_input > latest execution's status", () => {
  const base = { sessionRef: SESSION_REF };
  const running = record({ ...base, status: "running" });
  const waiting = record({
    ...base,
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    status: "waiting_for_input",
  });
  const completedNewer = record({
    ...base,
    target: {
      ...CODEX_TARGET,
      sessionId: "33333333-3333-3333-3333-333333333333",
    },
    signerPubkey: CODEX_SIGNER,
    status: "completed",
    lastEventAt: "2026-08-12T12:00:00.000Z",
  });

  assert.equal(
    groupCodingSessionCatalog([running, waiting, completedNewer])[0].status,
    "running",
  );
  assert.equal(
    groupCodingSessionCatalog([waiting, completedNewer])[0].status,
    "waiting_for_input",
  );
  assert.equal(
    groupCodingSessionCatalog([
      completedNewer,
      record({ ...base, status: "idle" }),
    ])[0].status,
    "completed",
  );
});

test("founder is the earliest create's signer (tie-break lowest event id); foreign attachments are counted", () => {
  const claude = record({ sessionRef: SESSION_REF });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    sessionRef: SESSION_REF,
    runtime: "codex",
  });
  const creates = [
    {
      sessionRef: SESSION_REF,
      signerPubkey: TEAMMATE,
      createdAt: 1_800_000_100,
      eventId: "event-b",
      target: CODEX_TARGET,
    },
    {
      sessionRef: SESSION_REF,
      signerPubkey: FOUNDER,
      createdAt: 1_800_000_000,
      eventId: "event-a",
      target: CLAUDE_TARGET,
    },
  ];
  const [umbrella] = groupCodingSessionCatalog([claude, codex], creates);

  assert.equal(umbrella.founderPubkey, FOUNDER);
  assert.equal(umbrella.foreignAttachmentCount, 1);
  const operators = new Map(
    umbrella.executions.map((e) => [e.signerPubkey, e.operatorPubkey]),
  );
  assert.equal(operators.get(CLAUDE_SIGNER), FOUNDER);
  assert.equal(operators.get(CODEX_SIGNER), TEAMMATE);

  // Same createdAt: the lowest event id founds the umbrella.
  const tied = [
    { ...creates[0], createdAt: 1_800_000_000 },
    { ...creates[1], createdAt: 1_800_000_000 },
  ];
  assert.equal(
    groupCodingSessionCatalog([claude, codex], tied)[0].founderPubkey,
    FOUNDER,
  );
});

test("without create observations, authority stays unknown rather than guessed", () => {
  const claude = record({ sessionRef: SESSION_REF });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    sessionRef: SESSION_REF,
  });
  const [umbrella] = groupCodingSessionCatalog([claude, codex]);
  assert.equal(umbrella.founderPubkey, null);
  assert.equal(umbrella.foreignAttachmentCount, 0);
  for (const execution of umbrella.executions) {
    assert.equal(execution.operatorPubkey, null);
  }
});

test("a create observation resolves the implicit umbrella's founder and operator", () => {
  const creates = [
    {
      sessionRef: null,
      signerPubkey: FOUNDER,
      createdAt: 1_800_000_000,
      eventId: "event-a",
      target: CLAUDE_TARGET,
    },
  ];
  const [umbrella] = groupCodingSessionCatalog([record()], creates);
  assert.equal(umbrella.founderPubkey, FOUNDER);
  assert.equal(umbrella.executions[0].operatorPubkey, FOUNDER);
  assert.equal(umbrella.foreignAttachmentCount, 0);
});

test("the composer participant list: N=1 offers only the execution, N>1 adds the Session lane", () => {
  const single = groupCodingSessionCatalog([
    record({ sessionRef: SESSION_REF }),
  ]);
  const singleParticipants = listCodingSessionUmbrellaParticipants(single[0]);
  assert.equal(singleParticipants.length, 1);
  assert.equal(singleParticipants[0].kind, "execution");
  assert.equal(singleParticipants[0].label, "Claude · claude-opus-5");

  const [umbrella] = groupCodingSessionCatalog([
    record({ sessionRef: SESSION_REF }),
    record({
      target: CODEX_TARGET,
      signerPubkey: CODEX_SIGNER,
      sessionRef: SESSION_REF,
      runtime: "codex",
      model: "gpt-5.3-codex",
    }),
  ]);
  const participants = listCodingSessionUmbrellaParticipants(umbrella);
  assert.equal(participants.length, 3);
  assert.deepEqual(
    participants.map((p) => p.kind),
    ["execution", "execution", "session"],
  );
  assert.deepEqual(
    participants
      .slice(0, 2)
      .map((p) => p.label)
      .sort(),
    ["Claude · claude-opus-5", "Codex · gpt-5.3-codex"],
  );
  assert.equal(participants[2].sessionRef, SESSION_REF);
  assert.equal(participants[2].label, "Session");
});

test("colliding participant labels disambiguate by signer prefix", () => {
  const [umbrella] = groupCodingSessionCatalog([
    record({ sessionRef: SESSION_REF }),
    record({
      target: {
        ...CLAUDE_TARGET,
        sessionId: "44444444-4444-4444-4444-444444444444",
      },
      signerPubkey: CODEX_SIGNER,
      sessionRef: SESSION_REF,
    }),
  ]);
  const labels = listCodingSessionUmbrellaParticipants(umbrella)
    .filter((p) => p.kind === "execution")
    .map((p) => p.label);
  assert.equal(new Set(labels).size, labels.length);
  assert.equal(
    labels.every((label) => label.startsWith("Claude · claude-opus-5")),
    true,
  );
});

test("the execution key is the target minus generation, per signer", () => {
  const keyGen1 = buildCodingSessionExecutionKey(CLAUDE_SIGNER, CLAUDE_TARGET);
  const keyGen2 = buildCodingSessionExecutionKey(CLAUDE_SIGNER, {
    ...CLAUDE_TARGET,
    generation: 7,
  });
  const otherSigner = buildCodingSessionExecutionKey(
    CODEX_SIGNER,
    CLAUDE_TARGET,
  );
  assert.equal(keyGen1, keyGen2);
  assert.notEqual(keyGen1, otherSigner);
});
