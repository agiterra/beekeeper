import assert from "node:assert/strict";
import { test } from "node:test";

const CHANNEL = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const PROVIDER = "d4".repeat(32);

async function targetKeyFor(target) {
  const { buildCodingSessionTargetKey } = await import(
    "@/features/coding-sessions/lib/codingSessionCommand"
  );
  return buildCodingSessionTargetKey(target);
}

function generation(overrides = {}) {
  return {
    targetKey: "coding-session/v1|3:acp:5:inst1:6:sess-1:1:1",
    executionKey: "coding-execution/v1|x",
    providerAuthorityPubkey: PROVIDER,
    current: false,
    reachability: "unverified",
    status: "running",
    statusAt: 1_000,
    branch: null,
    observedCommit: null,
    dirty: null,
    relayReachable: null,
    verifiedAt: null,
    commitConfirmation: "Commit not checked",
    leaseState: null,
    leaseIssuedAt: null,
    leaseAcceptedAt: null,
    leaseExpiresAt: null,
    leaseSigner: null,
    leaseSourceEventId: null,
    leaseSequence: null,
    lifecycleCommandEventId: "c1".padStart(64, "0"),
    lifecycleReceiptEventId: "r1".padStart(64, "0"),
    sourceEventIds: [],
    ...overrides,
  };
}

function session(generations) {
  return {
    sessionKey: "session-key",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    name: "Pulse plumbing",
    goal: null,
    lifecycle: "open",
    coordinationState: "open_unverified",
    latestObservationAt: 1_000,
    observedAgeSeconds: 10,
    generations,
    sourceEventIds: [],
  };
}

test("a real target key round-trips into the transcript generation id", async () => {
  const { pulseSessionRouteParams } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  const { buildCodingSessionTranscriptGenerationId } = await import(
    "@/features/coding-sessions/lib/codingSessionTranscriptPresentation"
  );
  const target = {
    driver: "claude-agent-acp",
    instanceId: "pulse-instance",
    sessionId: "sess-11",
    generation: 3,
  };
  const targetKey = await targetKeyFor(target);
  const params = pulseSessionRouteParams({
    channelId: CHANNEL,
    session: session([generation({ targetKey, current: true })]),
  });
  assert.deepEqual(params, {
    channelId: CHANNEL,
    generationId: buildCodingSessionTranscriptGenerationId(
      CHANNEL,
      PROVIDER,
      target,
    ),
  });
});

test("the current generation wins over a newer-looking sibling", async () => {
  const { pulseSessionRouteParams, pulseSessionRouteGeneration } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  const currentKey = await targetKeyFor({
    driver: "acp",
    instanceId: "inst",
    sessionId: "sess-current",
    generation: 1,
  });
  const newerKey = await targetKeyFor({
    driver: "acp",
    instanceId: "inst",
    sessionId: "sess-newer",
    generation: 2,
  });
  const rows = session([
    generation({ targetKey: newerKey, statusAt: 9_999 }),
    generation({ targetKey: currentKey, current: true, statusAt: 1 }),
  ]);
  assert.equal(pulseSessionRouteGeneration(rows)?.targetKey, currentKey);
  const params = pulseSessionRouteParams({ channelId: CHANNEL, session: rows });
  assert.ok(params !== null);
  assert.ok(params.generationId.includes("sess-current"));
});

test("with no current generation the newest by statusAt is opened", async () => {
  const { pulseSessionRouteGeneration } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  const oldKey = await targetKeyFor({
    driver: "acp",
    instanceId: "inst",
    sessionId: "sess-old",
    generation: 1,
  });
  const newKey = await targetKeyFor({
    driver: "acp",
    instanceId: "inst",
    sessionId: "sess-new",
    generation: 2,
  });
  const picked = pulseSessionRouteGeneration(
    session([
      generation({ targetKey: oldKey, statusAt: 100 }),
      generation({ targetKey: newKey, statusAt: 500 }),
      generation({ targetKey: "coding-session/v1|bogus", statusAt: null }),
    ]),
  );
  assert.equal(picked?.targetKey, newKey);
});

test("a session with no generation has no route", async () => {
  const { pulseSessionRouteParams } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  assert.equal(
    pulseSessionRouteParams({ channelId: CHANNEL, session: session([]) }),
    null,
  );
});

test("an undecodable target key has no route, and no invented one", async () => {
  const { pulseSessionRouteParams } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  assert.equal(
    pulseSessionRouteParams({
      channelId: CHANNEL,
      session: session([
        generation({ targetKey: "not-a-coding-session-key", current: true }),
      ]),
    }),
    null,
  );
});

test("an empty channel id has no route: the channel is never guessed", async () => {
  const { pulseSessionRouteParams } = await import(
    "@/features/project-pulse/lib/pulseSessionRoute"
  );
  const targetKey = await targetKeyFor({
    driver: "acp",
    instanceId: "inst",
    sessionId: "sess",
    generation: 1,
  });
  assert.equal(
    pulseSessionRouteParams({
      channelId: "",
      session: session([generation({ targetKey, current: true })]),
    }),
    null,
  );
});
