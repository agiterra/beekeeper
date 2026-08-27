import assert from "node:assert/strict";
import test from "node:test";

import {
  hasStrictMetadataJson,
  isStrictMetadataContent,
} from "./sessionCoordinationStrictJson.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const ACTOR = "c".repeat(64);

const BASE = {
  schema: "buzz-coding-session-metadata/v1",
  session: {
    driver: "claude-agent-acp",
    instanceId: "0123456789abcdef",
    sessionId: "11111111-2222-3333-4444-555555555555",
    generation: 1,
  },
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
};

const FACTS = {
  observedCommit: "a".repeat(40),
  dirty: false,
  relayReachable: true,
  verifiedAt: 1_800_000_000,
};

/** Metadata content in the key order the Rust producer serializes. */
function metadata(overrides = {}) {
  return JSON.stringify({ ...BASE, ...overrides });
}

test("the sixteen shapes buzz-core accepts all decode", () => {
  const amendments = [
    { sessionRef: SESSION_REF },
    { agentRef: ACTOR, role: "builder" },
    { turnBudget: { used: 3, limit: 20 } },
    FACTS,
  ];
  let accepted = 0;
  for (let mask = 0; mask < 16; mask += 1) {
    const content = {};
    for (const [index, amendment] of amendments.entries()) {
      if (mask & (1 << index)) Object.assign(content, amendment);
    }
    // A budget describes an umbrella, so the two shapes that carry it always
    // carry a sessionRef too — that is the pairing buzz-core enforces, not an
    // extra shape.
    if (content.turnBudget) content.sessionRef = SESSION_REF;
    const source = metadata(content);
    assert.equal(
      isStrictMetadataContent(source),
      true,
      `shape ${mask} was rejected: ${source}`,
    );
    accepted += 1;
  }
  assert.equal(accepted, 16);
});

test("a turnBudget beside a sessionRef is accepted, not dropped", () => {
  const source = metadata({
    sessionRef: SESSION_REF,
    turnBudget: { used: 7, limit: 20 },
  });
  assert.equal(isStrictMetadataContent(source), true);
  assert.equal(hasStrictMetadataJson(source, JSON.parse(source)), true);
});

test("an explicit null role or turnBudget reads as absent, as Rust reads it", () => {
  assert.equal(isStrictMetadataContent(metadata({ role: null })), true);
  assert.equal(
    isStrictMetadataContent(
      metadata({ sessionRef: SESSION_REF, turnBudget: null }),
    ),
    true,
  );
});

test("a role without an actor is malformed, and so is a bad slug", () => {
  assert.equal(isStrictMetadataContent(metadata({ role: "builder" })), false);
  assert.equal(
    isStrictMetadataContent(metadata({ agentRef: ACTOR, role: "Builder" })),
    false,
  );
});

test("a turnBudget is refused without a sessionRef, and every malformed shape is refused", () => {
  assert.equal(
    isStrictMetadataContent(metadata({ turnBudget: { used: 1, limit: 20 } })),
    false,
  );
  const bad = [
    { used: 1 },
    { used: 1, limit: 0 },
    { used: -1, limit: 20 },
    { used: 1.5, limit: 20 },
    { used: 1, limit: 20, extra: true },
    { used: "1", limit: 20 },
    [1, 20],
  ];
  for (const turnBudget of bad) {
    assert.equal(
      isStrictMetadataContent(
        metadata({ sessionRef: SESSION_REF, turnBudget }),
      ),
      false,
      `accepted a malformed turnBudget: ${JSON.stringify(turnBudget)}`,
    );
  }
});

test("an unknown key is still a hard rejection", () => {
  assert.equal(isStrictMetadataContent(metadata({ nope: 1 })), false);
  assert.equal(
    isStrictMetadataContent(metadata({ observedCommit: "a".repeat(40) })),
    false,
  );
});
