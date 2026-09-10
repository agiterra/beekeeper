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

// §2 item 38 — a resumed session read "Coding sessions (2)" and listed two
// rows that opened the same umbrella. Generations are how many times a session
// was resumed; they are a property of one row, not two.
const RESUME_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "instance-1",
  sessionId: "11111111-1111-1111-1111-111111111111",
};
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function generation({
  generation: generationNumber,
  lastEventAt,
  sessionRef = null,
  sessionId = RESUME_TARGET.sessionId,
  status = "idle",
}) {
  return {
    generationId: `gen-${sessionId.slice(0, 4)}-${generationNumber}`,
    label: `generation ${generationNumber}`,
    title: "Fix the reconnect bug",
    providerAuthorityPubkey: "a".repeat(64),
    metadataAuthorityPubkey: "a".repeat(64),
    lastEventAt,
    status,
    statusAt: null,
    transcript: [],
    conflictCount: 0,
    commandTarget: {
      ...RESUME_TARGET,
      sessionId,
      generation: generationNumber,
    },
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: null,
    runtime: null,
    model: null,
    capabilities: null,
  };
}

test("a resumed session is one row that says how many generations it has", () => {
  const entries = resolveChannelCodingSessionIngress({
    activeChannelId: "channel-a",
    catalog: catalog({
      entries: [
        generation({ generation: 1, lastEventAt: "2026-08-23T10:00:00.000Z" }),
        generation({ generation: 2, lastEventAt: "2026-08-23T11:00:00.000Z" }),
      ],
    }),
  });

  assert.equal(entries.length, 1, "one durable session, one row");
  assert.equal(entries[0].generationCount, 2);
  assert.equal(entries[0].executionCount, 1);
  assert.equal(
    entries[0].session.label,
    "generation 2",
    "the row opens the most recently active generation",
  );
});

test("two providers under one session ref are still one row", () => {
  const entries = resolveChannelCodingSessionIngress({
    activeChannelId: "channel-a",
    catalog: catalog({
      entries: [
        generation({
          generation: 1,
          lastEventAt: "2026-08-23T10:00:00.000Z",
          sessionRef: SESSION_REF,
        }),
        generation({
          generation: 1,
          lastEventAt: "2026-08-23T12:00:00.000Z",
          sessionRef: SESSION_REF,
          sessionId: "22222222-2222-2222-2222-222222222222",
        }),
      ],
    }),
  });

  assert.equal(entries.length, 1);
  assert.equal(entries[0].executionCount, 2);
  assert.equal(entries[0].generationCount, 2);
});

test("two unrelated sessions stay two rows", () => {
  const entries = resolveChannelCodingSessionIngress({
    activeChannelId: "channel-a",
    catalog: catalog({
      entries: [
        generation({ generation: 1, lastEventAt: "2026-08-23T10:00:00.000Z" }),
        generation({
          generation: 1,
          lastEventAt: "2026-08-23T11:00:00.000Z",
          sessionId: "33333333-3333-3333-3333-333333333333",
        }),
      ],
    }),
  });

  assert.equal(entries.length, 2);
});

// ---------------------------------------------------------------------------
// Founded umbrellas: listed behind the same gate, never counted as openable.
// ---------------------------------------------------------------------------

const FOUNDED_GENESIS = {
  channelId: "channel-a",
  sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  genesisRef: "c".repeat(64),
  founderPubkey: "f".repeat(64),
  foundedAt: 1_800_000_000,
};

test("a founded umbrella is listed by the founded resolver and absent from the openable rows", async () => {
  const { resolveChannelFoundedCodingSessions } = await import(
    "./channelCodingSessionIngress.ts"
  );
  const snapshot = catalog({ geneses: [FOUNDED_GENESIS], creates: [] });
  const founded = resolveChannelFoundedCodingSessions({
    activeChannelId: "channel-a",
    catalog: snapshot,
  });
  assert.deepEqual(founded, [FOUNDED_GENESIS]);
  const openable = resolveChannelCodingSessionIngress({
    activeChannelId: "channel-a",
    catalog: snapshot,
  });
  assert.equal(openable.length, 2, "the two provider generations only");
  assert.equal(
    openable.some(
      ({ session }) => session.sessionRef === FOUNDED_GENESIS.sessionRef,
    ),
    false,
  );
});

test("founded rows pass the same gate: channel transition, null channel, and authority error yield none", async () => {
  const { resolveChannelFoundedCodingSessions } = await import(
    "./channelCodingSessionIngress.ts"
  );
  assert.deepEqual(
    resolveChannelFoundedCodingSessions({
      activeChannelId: "channel-b",
      catalog: catalog({ geneses: [FOUNDED_GENESIS] }),
    }),
    [],
  );
  assert.deepEqual(
    resolveChannelFoundedCodingSessions({
      activeChannelId: null,
      catalog: catalog({ geneses: [FOUNDED_GENESIS] }),
    }),
    [],
  );
  assert.deepEqual(
    resolveChannelFoundedCodingSessions({
      activeChannelId: "channel-a",
      catalog: catalog({
        geneses: [FOUNDED_GENESIS],
        authorityErrorMessage: "refused",
      }),
    }),
    [],
  );
  // A snapshot that collected no geneses lists none — not a guess.
  assert.deepEqual(
    resolveChannelFoundedCodingSessions({
      activeChannelId: "channel-a",
      catalog: catalog(),
    }),
    [],
  );
});
