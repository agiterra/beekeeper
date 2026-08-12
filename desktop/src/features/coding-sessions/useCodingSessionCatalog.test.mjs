import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTargetKey } from "./lib/codingSessionCommand.ts";
import { BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA } from "./lib/codingSessionTranscriptPresentation.ts";
import { BUZZ_CODING_SESSION_METADATA_SCHEMA } from "./lib/codingSessionTrustedIngress.ts";
import { mergeTrustedCodingSessionIngress } from "./useCodingSessionCatalog.ts";

const CHANNEL_ID = "channel-1";
const SIGNER = "a".repeat(64);
const OTHER_SIGNER = "b".repeat(64);
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function metadataEntry({
  target = TARGET,
  signerPubkey = SIGNER,
  channelId = CHANNEL_ID,
  createdAt = 1_800_000_000,
  conflictCount = 0,
  ...overrides
} = {}) {
  return {
    channelId,
    targetKey: buildCodingSessionTargetKey(target),
    signerPubkey,
    metadata: {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      projectRef: "30621:owner:agiterra",
      repoRef: null,
      title: "Advance coding sessions",
      agentRef: null,
      provider: "claude-primary",
      runtime: "claude",
      model: "claude-opus-5",
      status: "running",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: false,
        context: false,
        diff: false,
        plan: true,
      },
      ...overrides,
    },
    eventId: `metadata-${signerPubkey}-${target.generation}`,
    createdAt,
    conflictCount,
  };
}

function transcriptEntry({
  eventSeq,
  item,
  target = TARGET,
  signerPubkey = SIGNER,
  channelId = CHANNEL_ID,
  timestamp = 1_800_000_100_000 + eventSeq,
  conflictCount = 0,
} = {}) {
  return {
    channelId,
    targetKey: buildCodingSessionTargetKey(target),
    signerPubkey,
    transcript: {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: target,
      eventSeq,
      timestamp,
      turnId: "turn-1",
      item,
    },
    eventId: `transcript-${signerPubkey}-${eventSeq}`,
    createdAt: Math.floor(timestamp / 1000),
    conflictCount,
  };
}

test("a session is discoverable from metadata alone", () => {
  const [session] = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [metadataEntry()],
    [],
  );
  assert.equal(session.title, "Advance coding sessions");
  assert.equal(session.providerAuthorityPubkey, SIGNER);
  assert.equal(session.metadataAuthorityPubkey, SIGNER);
  assert.equal(session.status, "running");
  assert.equal(session.projectRef, "30621:owner:agiterra");
  assert.equal(session.runtime, "claude");
  assert.deepEqual(session.commandTarget, TARGET);
  assert.deepEqual(session.transcript, []);
  assert.equal(session.lastEventAt, new Date(1_800_000_000_000).toISOString());
});

test("a session is discoverable from transcripts alone, with inferred status", () => {
  const [session] = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [],
    [
      transcriptEntry({
        eventSeq: 1,
        item: { kind: "user_prompt", content: "go" },
      }),
      transcriptEntry({
        eventSeq: 2,
        item: { kind: "assistant_text", text: "working" },
      }),
    ],
  );
  assert.equal(session.metadataAuthorityPubkey, null);
  assert.equal(session.status, "running");
  assert.equal(session.title, "Claude Agent Acp session");
  assert.equal(session.label, "Claude Agent Acp · generation 1");
  assert.equal(session.transcript.length, 2);
  assert.equal(session.projectRef, null);
  assert.equal(session.capabilities, null);
});

test("metadata enriches only its exact channel, target, signer, and unconflicted key", () => {
  const sessions = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [
      metadataEntry({ channelId: "channel-2", title: "Wrong channel" }),
      metadataEntry({
        target: { ...TARGET, generation: 2 },
        title: "Wrong generation",
      }),
      metadataEntry({ signerPubkey: OTHER_SIGNER, title: "Wrong signer" }),
      metadataEntry({ conflictCount: 1, title: "Conflicted" }),
    ],
    [transcriptEntry({ eventSeq: 1, item: { kind: "status" } })],
  );
  const exact = sessions.find(
    (session) =>
      session.providerAuthorityPubkey === SIGNER &&
      session.commandTarget.generation === 1,
  );
  assert.equal(exact.metadataAuthorityPubkey, null);
  assert.equal(exact.title, "Claude Agent Acp session");
  assert.ok(
    !sessions.some((session) => session.title === "Wrong channel"),
    "another channel's metadata never creates a session here",
  );
  assert.ok(
    sessions.some((session) => session.title === "Wrong generation"),
    "a different generation is its own session, not an enrichment",
  );
});

test("each trusted signer gets its own generation record", () => {
  const sessions = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [metadataEntry(), metadataEntry({ signerPubkey: OTHER_SIGNER })],
    [],
  );
  assert.equal(sessions.length, 2);
  assert.deepEqual(
    sessions.map((session) => session.providerAuthorityPubkey).sort(),
    [SIGNER, OTHER_SIGNER].sort(),
  );
  assert.equal(
    new Set(sessions.map((session) => session.generationId)).size,
    2,
  );
});

test("lastEventAt folds transcript timestamp maxima over the metadata createdAt seed", () => {
  const [session] = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [metadataEntry({ createdAt: 1_800_000_000 })],
    [
      transcriptEntry({
        eventSeq: 1,
        item: { kind: "status" },
        timestamp: 1_800_000_500_000,
      }),
      transcriptEntry({
        eventSeq: 2,
        item: { kind: "status" },
        timestamp: 1_800_000_200_000,
      }),
    ],
  );
  assert.equal(session.lastEventAt, new Date(1_800_000_500_000).toISOString());
});

test("no metadata and no transcripts seed epoch zero", () => {
  const [session] = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [metadataEntry({ conflictCount: 1 })],
    [],
  );
  assert.equal(session.lastEventAt, new Date(0).toISOString());
  assert.equal(session.conflictCount, 0, "the conflicted entry never enriched");
});

test("status infers from the highest unconflicted eventSeq", () => {
  const cases = [
    [{ kind: "interrupted" }, "interrupted"],
    [{ kind: "result", subtype: "cancelled" }, "interrupted"],
    [{ kind: "result", subtype: "error" }, "failed"],
    [{ kind: "result", subtype: "success", isError: false }, "completed"],
    [{ kind: "result", isError: true }, "failed"],
    [{ kind: "assistant_text", text: "mid-turn" }, "running"],
    [{ notAKind: true }, "unknown"],
  ];
  for (const [item, expected] of cases) {
    const [session] = mergeTrustedCodingSessionIngress(
      CHANNEL_ID,
      [],
      [
        transcriptEntry({
          eventSeq: 1,
          item: { kind: "user_prompt", content: "go" },
        }),
        transcriptEntry({ eventSeq: 9, item }),
      ],
    );
    assert.equal(session.status, expected, JSON.stringify(item));
  }
});

test("a conflicted transcript is counted but never rendered", () => {
  const [session] = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [],
    [
      transcriptEntry({
        eventSeq: 1,
        item: { kind: "assistant_text", text: "clean" },
      }),
      transcriptEntry({
        eventSeq: 2,
        item: { kind: "assistant_text", text: "conflicted" },
        conflictCount: 1,
      }),
    ],
  );
  assert.equal(session.conflictCount, 1);
  assert.deepEqual(
    session.transcript.map((item) => item.text),
    ["clean"],
  );
});

test("records order by activity descending, then by generation id", () => {
  const older = { ...TARGET, sessionId: "aaaa" };
  const newer = { ...TARGET, sessionId: "bbbb" };
  const sessions = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [],
    [
      transcriptEntry({
        target: older,
        eventSeq: 1,
        item: { kind: "status" },
        timestamp: 1_800_000_100_000,
      }),
      transcriptEntry({
        target: newer,
        eventSeq: 1,
        item: { kind: "status" },
        timestamp: 1_800_000_900_000,
      }),
    ],
  );
  assert.deepEqual(
    sessions.map((session) => session.commandTarget.sessionId),
    ["bbbb", "aaaa"],
  );

  const tied = mergeTrustedCodingSessionIngress(
    CHANNEL_ID,
    [],
    [
      transcriptEntry({
        target: newer,
        eventSeq: 1,
        item: { kind: "status" },
        timestamp: 1_800_000_100_000,
      }),
      transcriptEntry({
        target: older,
        eventSeq: 1,
        item: { kind: "status" },
        timestamp: 1_800_000_100_000,
      }),
    ],
  );
  assert.deepEqual(
    tied.map((session) => session.generationId),
    [...tied.map((session) => session.generationId)].sort(),
  );
});

test("a null channel yields nothing at all", () => {
  assert.deepEqual(
    mergeTrustedCodingSessionIngress(
      null,
      [metadataEntry()],
      [transcriptEntry({ eventSeq: 1, item: { kind: "status" } })],
    ),
    [],
  );
});
