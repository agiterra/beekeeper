/**
 * `session.restart` through the shared coordination fold.
 *
 * A restart detaches the live child and reattaches it as the next generation
 * under the restart command's id, answered with a `resumed` receipt (spec
 * § 4.9). Its predecessor's last metadata says `disconnected`, so a fold that
 * stops the chain at the predecessor reads a healthy restarted session as
 * terminal. These pin that the chain walks through a restart exactly as it
 * walks through a resume, and that a restart's receipt still has to come from
 * the provider the command names.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { foldSessionCoordination } from "@/shared/coordination/sessionCoordinationFold";

const NOW = 1_785_600_000;
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = "a1".repeat(32);
const PROVIDER = "d4".repeat(32);
const STRANGER = "e5".repeat(32);

function target(generation) {
  return {
    driver: "acp",
    instanceId: "inst-1",
    sessionId: "sess-1",
    generation,
  };
}

function targetKey(generation) {
  return `coding-session/v1|3:acp6:inst-16:sess-11:${generation}`;
}

function command(id, commandId, action, at) {
  return {
    id,
    pubkey: FOUNDER,
    created_at: at,
    kind: 44221,
    tags: [
      ["h", CHANNEL],
      ["csl-v", "csl1-1"],
      ["csl-command", commandId],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId,
      action,
    }),
  };
}

function createEvent() {
  return command(
    "01".repeat(32),
    "create-1",
    {
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
    NOW - 300,
  );
}

function restartEvent() {
  return command(
    "04".repeat(32),
    "restart-1",
    {
      type: "session.restart",
      session: target(1),
      providerAuthorityPubkey: PROVIDER,
    },
    NOW - 100,
  );
}

function receiptEvent(id, commandId, status, generation, signer, at) {
  return {
    id,
    pubkey: signer,
    created_at: at,
    kind: 44224,
    tags: [
      ["h", CHANNEL],
      ["cslr-v", "cslr1-1"],
      ["csl-command", commandId],
      [
        "csl-key",
        `coding-session-lifecycle-receipt/v1|${commandId.length}:${commandId}`,
      ],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-receipt/v1",
      commandId,
      status,
      session: target(generation),
      error: null,
    }),
  };
}

function metadataEvent(id, generation, status, at) {
  return {
    id,
    pubkey: PROVIDER,
    created_at: at,
    kind: 44223,
    tags: [
      ["h", CHANNEL],
      ["csm-v", "csm1-1"],
      ["cs-target", targetKey(generation)],
      ["csm-key", "unused-by-this-reader"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      session: target(generation),
      projectRef: null,
      repoRef: null,
      title: "Restart",
      agentRef: null,
      provider: "claude-primary",
      runtime: "claude",
      model: null,
      status,
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
    }),
  };
}

function fold(restartReceiptSigner) {
  return foldSessionCoordination({
    now: NOW,
    events: [
      createEvent(),
      receiptEvent(
        "02".repeat(32),
        "create-1",
        "created",
        1,
        PROVIDER,
        NOW - 290,
      ),
      // The restart's detach: the predecessor's last word is `disconnected`.
      metadataEvent("03".repeat(32), 1, "disconnected", NOW - 95),
      restartEvent(),
      receiptEvent(
        "05".repeat(32),
        "restart-1",
        "resumed",
        2,
        restartReceiptSigner,
        NOW - 90,
      ),
      metadataEvent("06".repeat(32), 2, "idle", NOW - 80),
    ],
    commissioners: [FOUNDER],
  });
}

test("a restart mints the next generation, which becomes current", () => {
  const result = fold(PROVIDER);
  assert.equal(result.sessions.length, 1, JSON.stringify(result.ambiguities));
  const [session] = result.sessions;
  assert.equal(session.lifecycle, "open");
  const byKey = new Map(session.generations.map((g) => [g.targetKey, g]));
  const restarted = byKey.get(targetKey(2));
  assert.ok(restarted, "the restarted generation is in the chain");
  assert.equal(restarted.current, true);
  assert.equal(restarted.status, "idle");
  assert.notEqual(restarted.reachability, "terminal");
  assert.equal(restarted.lifecycleCommandEventId, "04".repeat(32));
  assert.equal(byKey.get(targetKey(1))?.current, false);
});

test("a restart answered by anyone but the named provider proves nothing", () => {
  const [session] = fold(STRANGER).sessions;
  assert.deepEqual(
    session.generations.map((g) => [g.targetKey, g.current]),
    [[targetKey(1), true]],
  );
});
