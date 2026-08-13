import assert from "node:assert/strict";
import test from "node:test";

import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel.ts";
import {
  buildUmbrellaTimeline,
  groupTranscriptIntoTurnBlocks,
} from "./codingSessionUmbrellaTimeline.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);

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

function item(id, timestamp, turnId = null) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text: `text-${id}`,
    timestamp,
    ...(turnId === null ? {} : { turnId }),
  };
}

function record({
  target = CLAUDE_TARGET,
  signerPubkey = CLAUDE_SIGNER,
  sessionRef = SESSION_REF,
  transcript = [],
  lastEventAt = "2026-08-12T10:00:00.000Z",
  runtime = "claude",
} = {}) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 4)}-${target.driver}-${target.generation}`,
    label: `${target.driver} · generation ${target.generation}`,
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status: "running",
    transcript,
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: `${runtime}-primary`,
    runtime,
    model: null,
    capabilities: null,
  };
}

function laneMessage(eventId, timestampMs, content = "hello") {
  return {
    eventId,
    sessionRef: SESSION_REF,
    channelId: "channel-1",
    authorPubkey: "c".repeat(64),
    content,
    timestampMs,
  };
}

test("an umbrella of one renders exactly today's block sequence: flattening reproduces the transcript", () => {
  const transcript = [
    item("i1", "2026-08-12T10:00:00.000Z", "turn-1"),
    item("i2", "2026-08-12T10:00:01.000Z", "turn-1"),
    item("i3", "2026-08-12T10:00:02.000Z"),
    item("i4", "2026-08-12T10:00:03.000Z", "turn-2"),
  ];
  const [umbrella] = groupCodingSessionCatalog([
    record({ sessionRef: null, transcript }),
  ]);
  const timeline = buildUmbrellaTimeline(umbrella);

  assert.equal(
    timeline.every((entry) => entry.kind === "turn-block"),
    true,
  );
  // The single-execution identity: same items, same order, nothing added.
  assert.deepEqual(
    timeline.flatMap((entry) => entry.items),
    transcript,
  );
  assert.deepEqual(
    timeline.map((entry) => entry.turnId),
    ["turn-1", null, "turn-2"],
  );
  for (const entry of timeline) {
    assert.equal(entry.signerPubkey, CLAUDE_SIGNER);
    assert.equal(entry.generation, 1);
  }
});

test("consecutive items group by turnId; a turn interrupted by another turn does not re-merge", () => {
  const blocks = groupTranscriptIntoTurnBlocks(
    record({
      transcript: [
        item("a1", "2026-08-12T10:00:00.000Z", "turn-a"),
        item("a2", "2026-08-12T10:00:01.000Z", "turn-a"),
        item("b1", "2026-08-12T10:00:02.000Z", "turn-b"),
        item("a3", "2026-08-12T10:00:03.000Z", "turn-a"),
      ],
    }),
    "execution-key",
  );
  assert.deepEqual(
    blocks.map((block) => [block.turnId, block.items.length]),
    [
      ["turn-a", 2],
      ["turn-b", 1],
      ["turn-a", 1],
    ],
  );
});

test("two executions interleave at turn-block granularity, each block one signer's stream", () => {
  const claude = record({
    transcript: [
      item("c1", "2026-08-12T10:00:00.000Z", "claude-turn-1"),
      item("c2", "2026-08-12T10:00:01.000Z", "claude-turn-1"),
      item("c3", "2026-08-12T10:00:10.000Z", "claude-turn-2"),
    ],
  });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    runtime: "codex",
    transcript: [
      item("x1", "2026-08-12T10:00:05.000Z", "codex-turn-1"),
      item("x2", "2026-08-12T10:00:06.000Z", "codex-turn-1"),
    ],
  });
  const [umbrella] = groupCodingSessionCatalog([claude, codex]);
  const timeline = buildUmbrellaTimeline(umbrella);

  assert.deepEqual(
    timeline.map((entry) => [entry.signerPubkey, entry.items.map((i) => i.id)]),
    [
      [CLAUDE_SIGNER, ["c1", "c2"]],
      [CODEX_SIGNER, ["x1", "x2"]],
      [CLAUDE_SIGNER, ["c3"]],
    ],
  );
  // Items are never cross-ordered between executions: within each signer the
  // original stream order survives the interleave.
  for (const signer of [CLAUDE_SIGNER, CODEX_SIGNER]) {
    const ids = timeline
      .filter((entry) => entry.signerPubkey === signer)
      .flatMap((entry) => entry.items.map((i) => i.id));
    assert.deepEqual(ids, [...ids].sort());
  }
  // Every block carries provenance for exactly one (signer, target) stream.
  for (const entry of timeline) {
    const signers = new Set(
      entry.items.map((i) =>
        i.id.startsWith("c") ? CLAUDE_SIGNER : CODEX_SIGNER,
      ),
    );
    assert.deepEqual([...signers], [entry.signerPubkey]);
  }
});

test("equal timestamps break ties deterministically by (signer, executionKey)", () => {
  const at = "2026-08-12T10:00:00.000Z";
  const claude = record({ transcript: [item("c1", at, "t1")] });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    runtime: "codex",
    transcript: [item("x1", at, "t2")],
  });
  const forward = buildUmbrellaTimeline(
    groupCodingSessionCatalog([claude, codex])[0],
  );
  const reversed = buildUmbrellaTimeline(
    groupCodingSessionCatalog([codex, claude])[0],
  );
  assert.deepEqual(
    forward.map((entry) => entry.signerPubkey),
    reversed.map((entry) => entry.signerPubkey),
  );
});

test("a block with an out-of-order timestamp pins behind its predecessor instead of jumping the queue", () => {
  const claude = record({
    transcript: [
      item("c1", "2026-08-12T10:00:10.000Z", "t1"),
      // Clock skew: a later block claims an earlier time.
      item("c2", "2026-08-12T09:00:00.000Z", "t2"),
    ],
  });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    runtime: "codex",
    transcript: [item("x1", "2026-08-12T09:30:00.000Z", "t3")],
  });
  const timeline = buildUmbrellaTimeline(
    groupCodingSessionCatalog([claude, codex])[0],
  );
  // Codex's genuinely-earlier block sorts first; Claude's skewed second block
  // still renders after its own predecessor.
  assert.deepEqual(
    timeline.map((entry) => entry.items[0].id),
    ["x1", "c1", "c2"],
  );
});

test("conversation-lane messages interleave by time and sort after blocks at the same instant", () => {
  const claude = record({
    transcript: [
      item("c1", "2026-08-12T10:00:00.000Z", "t1"),
      item("c2", "2026-08-12T10:00:10.000Z", "t2"),
    ],
  });
  const [umbrella] = groupCodingSessionCatalog([claude]);
  const timeline = buildUmbrellaTimeline(umbrella, [
    laneMessage("m1", Date.parse("2026-08-12T10:00:05.000Z")),
    laneMessage("m2", Date.parse("2026-08-12T10:00:00.000Z")),
  ]);
  assert.deepEqual(
    timeline.map((entry) =>
      entry.kind === "conversation" ? entry.message.eventId : entry.items[0].id,
    ),
    ["c1", "m2", "m1", "c2"],
  );
});

test("a generation bump surfaces as a lifecycle row before the new generation's blocks", () => {
  const generationOne = record({
    transcript: [item("g1", "2026-08-12T09:00:00.000Z", "t1")],
    lastEventAt: "2026-08-12T09:00:00.000Z",
  });
  const generationTwo = record({
    target: { ...CLAUDE_TARGET, generation: 2 },
    transcript: [item("g2", "2026-08-12T10:00:00.000Z", "t2")],
  });
  const [umbrella] = groupCodingSessionCatalog([generationTwo, generationOne]);
  const timeline = buildUmbrellaTimeline(umbrella);

  assert.deepEqual(
    timeline.map((entry) =>
      entry.kind === "lifecycle"
        ? `lifecycle:${entry.event}:${entry.generation}`
        : entry.items[0].id,
    ),
    ["g1", "lifecycle:generation-started:2", "g2"],
  );
});

test("hostile timestamps degrade to a pinned position, never a throw", () => {
  const claude = record({
    transcript: [
      item("c1", "not-a-timestamp", "t1"),
      item("c2", "2026-08-12T10:00:00.000Z", "t2"),
    ],
  });
  const timeline = buildUmbrellaTimeline(
    groupCodingSessionCatalog([claude])[0],
  );
  assert.deepEqual(
    timeline.map((entry) => entry.items[0].id),
    ["c1", "c2"],
  );
  assert.equal(
    timeline.every((entry) => Number.isFinite(entry.timestampMs)),
    true,
  );
});
