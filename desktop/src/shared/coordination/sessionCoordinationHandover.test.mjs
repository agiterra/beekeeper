/**
 * §3.1 through the shared coordination fold: a fenced execution says so.
 *
 * A provider that comes back to find the session claimed publishes its
 * execution as `disconnected` **and** carries the fence — claimant, body and
 * the accepted claim event. This proves the fold carries that disclosure
 * through to the generation, so the desktop reads it from the read it already
 * makes rather than from a second query, and never from reachability: an
 * offline provider is quiet, which is not the same fact as forbidden.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { foldSessionCoordination } from "@/shared/coordination/sessionCoordinationFold";

const NOW = 1_785_600_000;
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = "a1".repeat(32);
const PROVIDER = "d4".repeat(32);
const COMMAND_ID = "create-1";
const TARGET = {
  driver: "acp",
  instanceId: "inst-1",
  sessionId: "sess-1",
  generation: 1,
};
const HANDOVER = {
  state: "active",
  claimant: "b7".repeat(32),
  bodyPubkey: "c8".repeat(32),
  acceptedEventId: "e9".repeat(32),
};

function createEvent() {
  return {
    id: "01".repeat(32),
    pubkey: FOUNDER,
    created_at: NOW - 100,
    kind: 44221,
    tags: [
      ["h", CHANNEL],
      ["csl-v", "csl1-1"],
      ["csl-command", COMMAND_ID],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId: COMMAND_ID,
      action: {
        type: "session.create",
        projectRef: null,
        repoRef: null,
        sessionRef: SESSION,
        providerInstanceRef: "provider-1",
        providerAuthorityPubkey: PROVIDER,
        model: null,
        title: null,
        initialTurn: null,
      },
    }),
  };
}

function receiptEvent() {
  return {
    id: "02".repeat(32),
    pubkey: PROVIDER,
    created_at: NOW - 90,
    kind: 44224,
    tags: [
      ["h", CHANNEL],
      ["cslr-v", "cslr1-1"],
      ["csl-command", COMMAND_ID],
      [
        "csl-key",
        `coding-session-lifecycle-receipt/v1|${COMMAND_ID.length}:${COMMAND_ID}`,
      ],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-receipt/v1",
      commandId: COMMAND_ID,
      status: "created",
      session: TARGET,
      error: null,
    }),
  };
}

function metadataEvent(extra = {}) {
  const targetKey = `coding-session/v1|${TARGET.driver.length}:${TARGET.driver}|${TARGET.instanceId.length}:${TARGET.instanceId}|${TARGET.sessionId.length}:${TARGET.sessionId}|1:1`;
  return {
    id: "03".repeat(32),
    pubkey: PROVIDER,
    created_at: NOW - 80,
    kind: 44223,
    tags: [
      ["h", CHANNEL],
      ["csm-v", "csm1-1"],
      ["cs-target", targetKey],
      ["csm-key", "unused-by-this-reader"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      session: TARGET,
      projectRef: null,
      repoRef: null,
      title: "Handover",
      agentRef: null,
      provider: "claude-primary",
      runtime: "claude",
      model: null,
      status: "disconnected",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
      sessionRef: SESSION,
      ...extra,
    }),
  };
}

function generationOf(extra) {
  const fold = foldSessionCoordination({
    now: NOW,
    events: [createEvent(), receiptEvent(), metadataEvent(extra)],
    commissioners: [FOUNDER],
  });
  assert.equal(fold.sessions.length, 1, JSON.stringify(fold.ambiguities));
  return fold.sessions[0].generations[0];
}

test("a fenced execution carries the fence its provider disclosed", () => {
  const generation = generationOf({ handover: HANDOVER });
  assert.deepEqual(generation.handover, HANDOVER);
  assert.equal(
    generation.reachability,
    "terminal",
    "the fence is a separate fact from reachability, and neither implies the other",
  );
});

test("a voided disclosure keeps its word through the fold", () => {
  const generation = generationOf({
    handover: { ...HANDOVER, state: "voided" },
  });
  assert.equal(generation.handover.state, "voided");
});

test("an unfenced execution carries no handover key at all", () => {
  const generation = generationOf({});
  assert.equal(
    Object.hasOwn(generation, "handover"),
    false,
    "absent, not null: a build that never disclosed a fence must not look like one that disclosed none",
  );
});

test("an explicit null handover is refused, so the metadata proves nothing", () => {
  // Rust refuses `"handover": null` by name, so this event is not readable
  // metadata at all: the generation exists (its create and receipt stand) and
  // carries no observed status rather than a fence read from a bad shape.
  const generation = generationOf({ handover: null });
  assert.equal(Object.hasOwn(generation, "handover"), false);
  assert.equal(
    generation.status,
    null,
    "a refused metadata event contributes nothing, including its status",
  );
});
