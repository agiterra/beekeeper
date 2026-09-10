/**
 * The founded projection: a genesis with nothing running under it.
 *
 * What is proved here is the subtraction — which geneses are still founded
 * once creates and entries are taken away — the founder identity, the order,
 * and the five states the founded route can be in. Signing is not exercised:
 * the store already refuses anything unsigned before it reaches this model.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  founderPubkeysByGenesisRef,
  resolveFoundedCodingSessions,
  resolveFoundedCodingSessionRoute,
} from "./codingSessionFoundedModel.ts";

const CHANNEL_ID = "channel-1";
const OTHER_CHANNEL_ID = "channel-2";
const REF_A = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const REF_B = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
const FOUNDER = "f".repeat(64);
const OTHER_FOUNDER = "e".repeat(64);
const PROVIDER = "a".repeat(64);

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

function genesis({
  channelId = CHANNEL_ID,
  sessionRef = REF_A,
  genesisRef = `genesis-${sessionRef}`,
  founderPubkey = FOUNDER,
  foundedAt = 1_800_000_000,
} = {}) {
  return { channelId, sessionRef, genesisRef, founderPubkey, foundedAt };
}

function create({
  channelId = CHANNEL_ID,
  sessionRef = REF_A,
  genesisRef = `genesis-${sessionRef}`,
  target = TARGET,
} = {}) {
  return {
    channelId,
    sessionRef,
    signerPubkey: FOUNDER,
    createdAt: 1_800_000_010,
    eventId: `create-${sessionRef}`,
    genesisRef,
    genesisFounderPubkey: FOUNDER,
    target,
  };
}

function entry({
  sessionRef = REF_A,
  generationId = "gen-1",
  lastEventAt = "2026-09-08T10:00:00.000Z",
  target = TARGET,
} = {}) {
  return {
    generationId,
    label: `Session · generation ${target.generation}`,
    title: "Coding session",
    providerAuthorityPubkey: PROVIDER,
    metadataAuthorityPubkey: PROVIDER,
    lastEventAt,
    status: "running",
    statusAt: null,
    statusEventId: null,
    transcript: [],
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: "claude-primary",
    runtime: target.driver,
    model: null,
    agentRef: null,
    role: null,
    turnBudget: null,
    routing: null,
    capabilities: null,
    beeStamp: null,
    packRef: null,
  };
}

function catalog(overrides = {}) {
  return {
    channelId: CHANNEL_ID,
    entries: [],
    creates: [],
    geneses: [],
    isLoading: false,
    foundingIsLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    ...overrides,
  };
}

test("a genesis nothing claims is founded; the founder is the genesis signer", () => {
  const founded = resolveFoundedCodingSessions({
    channelId: CHANNEL_ID,
    geneses: [genesis()],
    entries: [],
    creates: [],
  });
  assert.deepEqual(founded, [
    {
      channelId: CHANNEL_ID,
      sessionRef: REF_A,
      genesisRef: `genesis-${REF_A}`,
      founderPubkey: FOUNDER,
      foundedAt: 1_800_000_000,
    },
  ]);
});

test("a receipt-joined create claiming the ref hides the founded row", () => {
  assert.deepEqual(
    resolveFoundedCodingSessions({
      channelId: CHANNEL_ID,
      geneses: [genesis()],
      entries: [],
      creates: [create()],
    }),
    [],
  );
  // A create in another channel claims nothing here, even with the same ref.
  assert.equal(
    resolveFoundedCodingSessions({
      channelId: CHANNEL_ID,
      geneses: [genesis()],
      entries: [],
      creates: [create({ channelId: OTHER_CHANNEL_ID })],
    }).length,
    1,
  );
});

test("a catalog entry echoing the ref hides the founded row on its own", () => {
  // The backstop: a create outside the observation history window while its
  // 44223 is still held.
  assert.deepEqual(
    resolveFoundedCodingSessions({
      channelId: CHANNEL_ID,
      geneses: [genesis()],
      entries: [entry()],
      creates: [],
    }),
    [],
  );
});

test("founded rows are scoped to the channel and ordered newest founding first", () => {
  const founded = resolveFoundedCodingSessions({
    channelId: CHANNEL_ID,
    geneses: [
      genesis({ sessionRef: REF_A, foundedAt: 1_800_000_000 }),
      genesis({ sessionRef: REF_B, foundedAt: 1_800_000_500 }),
      genesis({
        channelId: OTHER_CHANNEL_ID,
        sessionRef: "7d9a3e4c-12f6-4d2a-c3b5-9e4f0a8b6c32",
        foundedAt: 1_800_000_900,
      }),
    ],
    entries: [],
    creates: [],
  });
  assert.deepEqual(
    founded.map((row) => row.sessionRef),
    [REF_B, REF_A],
  );
});

test("the founded route: loading while either read is in flight and no genesis is known", () => {
  assert.deepEqual(
    resolveFoundedCodingSessionRoute({
      catalog: catalog({ isLoading: false, foundingIsLoading: true }),
      channelId: CHANNEL_ID,
      sessionRef: REF_A,
    }),
    { kind: "loading" },
  );
  assert.deepEqual(
    resolveFoundedCodingSessionRoute({
      catalog: catalog({ isLoading: true, foundingIsLoading: false }),
      channelId: CHANNEL_ID,
      sessionRef: REF_A,
    }),
    { kind: "loading" },
  );
});

test("the founded route: founded when a genesis exists and nothing claims it", () => {
  const resolution = resolveFoundedCodingSessionRoute({
    catalog: catalog({ geneses: [genesis()] }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.equal(resolution.kind, "founded");
  assert.equal(resolution.founded.founderPubkey, FOUNDER);
  assert.equal(resolution.founded.genesisRef, `genesis-${REF_A}`);
  // A genesis that is still loading elsewhere does not demote a known one.
  assert.equal(
    resolveFoundedCodingSessionRoute({
      catalog: catalog({ geneses: [genesis()], foundingIsLoading: true }),
      channelId: CHANNEL_ID,
      sessionRef: REF_A,
    }).kind,
    "founded",
  );
});

test("the founded route: starting once a create claims the ref but no generation exists", () => {
  const resolution = resolveFoundedCodingSessionRoute({
    catalog: catalog({ geneses: [genesis()], creates: [create()] }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.equal(resolution.kind, "starting");
  assert.equal(resolution.founded.sessionRef, REF_A);
});

test("the founded route: started names the newest generation of the umbrella", () => {
  const older = entry({
    generationId: "gen-1",
    lastEventAt: "2026-09-08T10:00:00.000Z",
    target: TARGET,
  });
  const newer = entry({
    generationId: "gen-2",
    lastEventAt: "2026-09-08T11:00:00.000Z",
    target: { ...TARGET, generation: 2 },
  });
  const resolution = resolveFoundedCodingSessionRoute({
    catalog: catalog({
      geneses: [genesis()],
      creates: [create()],
      entries: [older, newer],
    }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.deepEqual(resolution, { kind: "started", generationId: "gen-2" });
  // Started wins even while the observation read is still loading.
  assert.equal(
    resolveFoundedCodingSessionRoute({
      catalog: catalog({ entries: [older], foundingIsLoading: true }),
      channelId: CHANNEL_ID,
      sessionRef: REF_A,
    }).kind,
    "started",
  );
});

test("the founded route: missing when nothing is loading and no genesis matches, and on an authority error", () => {
  const missing = resolveFoundedCodingSessionRoute({
    catalog: catalog(),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.equal(missing.kind, "missing");
  assert.match(missing.description, /No genesis/);

  const wrongChannel = resolveFoundedCodingSessionRoute({
    catalog: catalog({ geneses: [genesis({ channelId: OTHER_CHANNEL_ID })] }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.equal(wrongChannel.kind, "missing");

  // Fail closed: an authority error is not a founded session, even with a
  // genesis in the snapshot.
  const refused = resolveFoundedCodingSessionRoute({
    catalog: catalog({
      geneses: [genesis()],
      authorityErrorMessage: "Pop-out refused.",
    }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.deepEqual(refused, {
    kind: "missing",
    description: "Pop-out refused.",
  });

  const errored = resolveFoundedCodingSessionRoute({
    catalog: catalog({ errorMessage: "relay unreachable" }),
    channelId: CHANNEL_ID,
    sessionRef: REF_A,
  });
  assert.deepEqual(errored, {
    kind: "missing",
    description: "relay unreachable",
  });
});

test("founders by genesis cover started umbrellas and founded geneses alike", () => {
  const founders = founderPubkeysByGenesisRef({
    entries: [entry()],
    creates: [create()],
    geneses: [
      genesis(),
      genesis({ sessionRef: REF_B, founderPubkey: OTHER_FOUNDER }),
    ],
  });
  assert.deepEqual(
    [...founders.entries()],
    [
      [`genesis-${REF_A}`, FOUNDER],
      [`genesis-${REF_B}`, OTHER_FOUNDER],
    ],
  );
  // Without the geneses the founded umbrella has no founder, and its 44230
  // could never be authorized — the regression this second source closes.
  assert.equal(
    founderPubkeysByGenesisRef({ entries: [entry()], creates: [create()] }).has(
      `genesis-${REF_B}`,
    ),
    false,
  );
});
