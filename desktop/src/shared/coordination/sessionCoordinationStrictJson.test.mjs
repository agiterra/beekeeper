import assert from "node:assert/strict";
import test from "node:test";

import {
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
  hasStrictLifecycleReceiptJson,
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

test("a seated create (actor + role) is a strict lifecycle command", () => {
  const create = (extra = {}) => ({
    schema: "buzz-coding-session-lifecycle-command/v1",
    commandId: "csl-1",
    action: {
      type: "session.create",
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      genesisRef: "b".repeat(64),
      providerInstanceRef: "claude-primary",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: "roletest",
      initialTurn: null,
      ...extra,
    },
  });
  const strict = (content) =>
    hasStrictLifecycleCommandJson(JSON.stringify(content), content) &&
    hasStrictLifecycleCommandValues(content);
  assert.equal(strict(create()), true);
  assert.equal(strict(create({ actor: ACTOR, role: "lead" })), true);
  assert.equal(strict(create({ role: "lead" })), false);
  assert.equal(strict(create({ actor: ACTOR })), false);
  assert.equal(strict(create({ actor: ACTOR, role: "Lead" })), false);
  assert.equal(strict(create({ actor: "C".repeat(64), role: "lead" })), false);
});

// ── Where per-turn usage lives, and where it does not ────────────────────────
//
// The provider reports token usage on the 44225 transcript `result` item, not
// on a 44224 receipt — no receipt status ends a turn (`turn_started` is the
// last stage a turn command produces; the turn's end is the `result` item).
// These two pin that boundary so a later change cannot quietly move usage onto
// the receipt: this gate is byte-exact on the receipt's key set and would drop
// every receipt on the floor.

test("the receipt gate still accepts the exact five-key lifecycle shape", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "created",
    session: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    },
    error: null,
  };
  const source = JSON.stringify(receipt);
  assert.equal(hasStrictLifecycleReceiptJson(source, receipt), true);
});

test("a receipt that tries to carry a usage block is refused", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "created",
    session: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    },
    error: null,
    usage: { inputTokens: 1200, contextWindow: 1000000 },
  };
  const source = JSON.stringify(receipt);
  assert.equal(
    hasStrictLifecycleReceiptJson(source, receipt),
    false,
    "usage belongs on the transcript result item, not on a receipt",
  );
});
