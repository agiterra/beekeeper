import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "@/shared/constants/kinds.ts";
import { CodingSessionCreateObservationStore } from "./codingSessionCreateObservations.ts";
import { buildCodingSessionGenesisEvent } from "./codingSessionGenesis.ts";
import { OPEN_CODING_SESSION_INGRESS_AUTHORITY } from "./codingSessionIngressAuthority.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "./codingSessionTrustedIngress.ts";
import {
  buildCodingSessionExecutionKey,
  groupCodingSessionCatalog,
  listCodingSessionUmbrellaParticipants,
} from "./codingSessionUmbrellaModel.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "channel-1";
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
    // No sessionRef means nothing can join it: legacy by construction.
    assert.equal(umbrella.genesisResolution, "legacy");
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
  // A ref-bearing umbrella with no observed create cannot rule a genesis
  // out — consumers (the join gate) must not treat it as ungoverned.
  assert.equal(umbrella.genesisResolution, "unresolved");
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

test("the composer participant list: a resumed single execution offers the Session lane", () => {
  // A resume adds a generation to the SAME execution. The umbrella surface
  // renders for it (prior generations are collapsed history), so the lane it
  // shows must also be addressable from the composer.
  const [umbrella] = groupCodingSessionCatalog([
    record({ sessionRef: SESSION_REF }),
    record({
      sessionRef: SESSION_REF,
      target: { ...CLAUDE_TARGET, generation: 2 },
      lastEventAt: "2026-08-12T11:00:00.000Z",
    }),
  ]);
  assert.equal(umbrella.executions.length, 1);
  assert.equal(umbrella.executions[0].priorGenerations.length, 1);
  const participants = listCodingSessionUmbrellaParticipants(umbrella);
  assert.deepEqual(
    participants.map((p) => p.kind),
    ["execution", "session"],
  );
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

test("participant labels split reasoning effort out of the raw model id", () => {
  const [umbrella] = groupCodingSessionCatalog([
    record({
      sessionRef: SESSION_REF,
      runtime: "codex",
      model: "gpt-5.6-terra[low]",
    }),
  ]);
  const [participant] = listCodingSessionUmbrellaParticipants(umbrella);
  assert.equal(participant.label, "Codex · gpt-5.6-terra · Low");
  assert.doesNotMatch(participant.label, /\[low\]/);
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

test("a viewer who runs no providers resolves the same founder as the founder does", () => {
  // Governance is a property of the session, not of the machine reading it.
  // Everything here is signed by strangers to this viewer: another member
  // founded the umbrella and another member's provider minted the execution,
  // so nothing involved appears in this machine's `allowed-bridge-pubkeys`.
  // The genesis reference must still resolve — a null founder here is what
  // produced the "ungoverned — adopt to govern." banner on a governed session,
  // and with it a composer that fell open and offered controls the provider
  // was always going to refuse.
  const founderSecret = generateSecretKey();
  const founderPubkey = getPublicKey(founderSecret);
  const providerSecret = generateSecretKey();
  const providerPubkey = getPublicKey(providerSecret);

  const genesisBuilt = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: genesisBuilt.kind,
      created_at: 1_799_999_999,
      tags: genesisBuilt.tags,
      content: genesisBuilt.content,
    },
    founderSecret,
  );
  const createBuilt = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: "csl-foreign-1",
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: providerPubkey,
    model: null,
    title: "Advance Buzz live sessions",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: createBuilt.kind,
      created_at: 1_800_000_000,
      tags: createBuilt.tags,
      content: createBuilt.content,
    },
    founderSecret,
  );
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: 1_800_000_005,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", "csl-foreign-1"],
        ["csl-key", lifecycleReceiptSemanticKey("csl-foreign-1")],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: "csl-foreign-1",
        status: "created",
        session: CLAUDE_TARGET,
        error: null,
      }),
    },
    providerSecret,
  );

  const store = new CodingSessionCreateObservationStore();
  store.ingestRelayEvents(
    [genesis, create, receipt],
    [CHANNEL_ID],
    OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  );
  const observations = store.snapshot([CHANNEL_ID]);
  assert.equal(observations.length, 1);

  const [umbrella] = groupCodingSessionCatalog(
    [record({ signerPubkey: providerPubkey, sessionRef: SESSION_REF })],
    observations,
  );
  assert.equal(umbrella.genesisRef, genesis.id);
  assert.equal(umbrella.founderPubkey, founderPubkey);
});

// Observed live 2026-08-24, 11:04Z: Testing1 held five executions, Brian
// stopped the newest one, and the header read ENDED while four others —
// including one answering turns — were still open.
function statusRecord({
  sessionId,
  status,
  lastEventAt,
  signer = "a".repeat(64),
}) {
  return {
    generationId: `gen-${sessionId}`,
    label: `session ${sessionId}`,
    title: "Testing1",
    providerAuthorityPubkey: signer,
    metadataAuthorityPubkey: signer,
    lastEventAt,
    status,
    statusAt: Date.parse(lastEventAt),
    transcript: [],
    conflictCount: 0,
    commandTarget: {
      driver: "codex-acp",
      instanceId: "instance-1",
      sessionId,
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: "3f0a5c9e-2b71-4d88-9a6f-5c1e0b7d4a23",
    provider: "codex-acp",
    runtime: "codex-acp",
    model: "default",
    capabilities: null,
  };
}

test("stopping the newest execution does not end an umbrella that still lives", () => {
  const [umbrella] = groupCodingSessionCatalog([
    statusRecord({
      sessionId: "11111111-1111-1111-1111-111111111111",
      status: "idle",
      lastEventAt: "2026-08-24T10:59:00.000Z",
    }),
    statusRecord({
      sessionId: "22222222-2222-2222-2222-222222222222",
      status: "stopped",
      lastEventAt: "2026-08-24T11:04:00.000Z",
    }),
  ]);

  assert.equal(umbrella.executions.length, 2);
  assert.equal(
    umbrella.status,
    "idle",
    "a per-execution stop is not the umbrella's end",
  );
});

test("an umbrella whose every execution stopped is ended", () => {
  const [umbrella] = groupCodingSessionCatalog([
    statusRecord({
      sessionId: "11111111-1111-1111-1111-111111111111",
      status: "stopped",
      lastEventAt: "2026-08-24T10:59:00.000Z",
    }),
    statusRecord({
      sessionId: "22222222-2222-2222-2222-222222222222",
      status: "stopped",
      lastEventAt: "2026-08-24T11:04:00.000Z",
    }),
  ]);

  assert.equal(umbrella.status, "stopped");
});

test("activity anywhere still outranks a quiet survivor", () => {
  const [umbrella] = groupCodingSessionCatalog([
    statusRecord({
      sessionId: "11111111-1111-1111-1111-111111111111",
      status: "running",
      lastEventAt: "2026-08-24T10:00:00.000Z",
    }),
    statusRecord({
      sessionId: "22222222-2222-2222-2222-222222222222",
      status: "stopped",
      lastEventAt: "2026-08-24T11:04:00.000Z",
    }),
  ]);

  assert.equal(umbrella.status, "running");
});

test("two same-signer executions get chips a person can tell apart", () => {
  const [umbrella] = groupCodingSessionCatalog([
    statusRecord({
      sessionId: "16e36197-d021-4862-a3b7-f08c5adc6659",
      status: "idle",
      lastEventAt: "2026-08-24T10:59:00.000Z",
    }),
    statusRecord({
      sessionId: "39977bdf-bb7a-4c0e-9b84-c7358f531882",
      status: "idle",
      lastEventAt: "2026-08-24T11:00:00.000Z",
    }),
  ]);

  const labels = listCodingSessionUmbrellaParticipants(umbrella)
    .filter((participant) => participant.kind === "execution")
    .map((participant) => participant.label);
  assert.equal(labels.length, 2);
  assert.notEqual(labels[0], labels[1], `identical chips: ${labels[0]}`);
  assert.ok(
    labels.some((label) => label.includes("16e36197")),
    labels.join(" / "),
  );
  assert.ok(
    labels.some((label) => label.includes("39977bdf")),
    labels.join(" / "),
  );
  // The signer is identical across the collision, so it distinguishes nothing
  // and must not be stacked in front of the id that does. Seen live as
  // `Claude · sonnet · 1958c6c4…9644 · 2655de24`, three times over.
  for (const label of labels) {
    assert.ok(
      !label.includes("\u2026"),
      `a signer that identifies nothing survived: ${label}`,
    );
  }
});
