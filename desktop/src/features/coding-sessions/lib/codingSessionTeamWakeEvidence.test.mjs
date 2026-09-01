/**
 * D-T4 and the evidence half of D-T1/D-T5: what Desktop is allowed to believe
 * about a 44220 it did not send.
 *
 * Every case here is a forgery attempt or a near miss — a foreign signer, the
 * previous generation's target, a pointer that differs by one byte — because
 * the whole point of reading receipts is that Desktop stops publishing when
 * somebody else already delivered, and "somebody else" has to be provable.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import {
  CodingSessionTeamWakeEvidenceIndex,
  decodeVerifiedTeamWakeCommand,
  decodeVerifiedTeamWakeReceipt,
} from "./codingSessionTeamWakeEvidence.ts";

const CHANNEL = "f4829942-15a8-4e74-accd-51c8448f250f";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOREIGN_SECRET = generateSecretKey();
const LEAD_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-primary",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const LEAD_TARGET_KEY = buildCodingSessionTargetKey(LEAD_TARGET);
const STALE_TARGET = { ...LEAD_TARGET, generation: 2 };
const POINTER = JSON.stringify({
  operationId: "e".repeat(64),
  type: "report",
});

function commandEvent({
  secret = PROVIDER_SECRET,
  commandId = "team-wake-2abd9f9a",
  target = LEAD_TARGET,
  targetTag = null,
  text = POINTER,
  channelId = CHANNEL,
  createdAt = 1_788_290_039,
} = {}) {
  return finalizeEvent(
    {
      kind: 44220,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cs-v", "csc1-1"],
        ["cs-target", targetTag ?? buildCodingSessionTargetKey(target)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-command/v1",
        commandId,
        target,
        action: { type: "thread.turn.start", text },
      }),
    },
    secret,
  );
}

function receiptEvent({
  secret = PROVIDER_SECRET,
  commandId = "team-wake-2abd9f9a",
  status = "turn_queued",
  target = LEAD_TARGET,
  error = null,
  turnId = null,
  createdAt = 1_788_290_045,
  channelId = CHANNEL,
} = {}) {
  const content = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId,
    status,
    session: target,
    error,
    ...(turnId ? { turnId } : {}),
  };
  return finalizeEvent(
    {
      kind: 44224,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csl-command", commandId],
      ],
      content: JSON.stringify(content),
    },
    secret,
  );
}

const CONTEXT = {
  channelId: CHANNEL,
  leadTargetKey: LEAD_TARGET_KEY,
  providerAuthorityPubkey: PROVIDER_PUBKEY,
};

test("D-T4: a verified provider command decodes to its exact pointer and command id", () => {
  const decoded = decodeVerifiedTeamWakeCommand(commandEvent(), CONTEXT);
  assert.equal(decoded.commandId, "team-wake-2abd9f9a");
  assert.equal(decoded.text, POINTER);
  assert.equal(decoded.targetKey, LEAD_TARGET_KEY);
  assert.equal(decoded.signerPubkey, PROVIDER_PUBKEY);
  assert.equal(decoded.createdAt, 1_788_290_039);
});

test("D-T4: a command addressed to another execution or another channel never decodes", () => {
  assert.equal(
    decodeVerifiedTeamWakeCommand(
      commandEvent({ target: STALE_TARGET }),
      CONTEXT,
    ),
    null,
    "a command for the next lead generation is not evidence for this one",
  );
  assert.equal(
    decodeVerifiedTeamWakeCommand(
      commandEvent({ channelId: "6ee7a1e5-0000-4000-8000-000000000000" }),
      CONTEXT,
    ),
    null,
  );
  assert.equal(
    decodeVerifiedTeamWakeCommand(
      commandEvent({ targetTag: LEAD_TARGET_KEY, target: STALE_TARGET }),
      CONTEXT,
    ),
    null,
    "a truthful tag cannot launder a payload aimed elsewhere",
  );
});

test("D-T4: a tampered signature or a body that is not a turn start never decodes", () => {
  const forged = { ...commandEvent(), content: "{}" };
  assert.equal(decodeVerifiedTeamWakeCommand(forged, CONTEXT), null);
  const interrupt = finalizeEvent(
    {
      kind: 44220,
      created_at: 1_788_290_039,
      tags: [
        ["h", CHANNEL],
        ["cs-v", "csc1-1"],
        ["cs-target", LEAD_TARGET_KEY],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-command/v1",
        commandId: "interrupt-1",
        target: LEAD_TARGET,
        action: { type: "thread.turn.interrupt" },
      }),
    },
    PROVIDER_SECRET,
  );
  assert.equal(decodeVerifiedTeamWakeCommand(interrupt, CONTEXT), null);
});

test("D-T4: receipt trust is exactly signer plus session target", () => {
  const honest = decodeVerifiedTeamWakeReceipt(receiptEvent(), CONTEXT);
  assert.equal(honest.commandId, "team-wake-2abd9f9a");
  assert.equal(honest.receipt.status, "turn_queued");
  assert.equal(
    decodeVerifiedTeamWakeReceipt(
      receiptEvent({ secret: FOREIGN_SECRET }),
      CONTEXT,
    ),
    null,
    "a foreign key cannot settle this operation",
  );
  assert.equal(
    decodeVerifiedTeamWakeReceipt(
      receiptEvent({ target: STALE_TARGET }),
      CONTEXT,
    ),
    null,
    "a receipt about another generation is not about this lead",
  );
  assert.equal(
    decodeVerifiedTeamWakeReceipt(
      { ...receiptEvent(), content: '{"schema":"nope"}' },
      CONTEXT,
    ),
    null,
  );
});

test("D-T1 (index): the index answers by exact pointer bytes and command id", () => {
  const index = new CodingSessionTeamWakeEvidenceIndex(CONTEXT);
  assert.equal(
    index.ingest([
      commandEvent(),
      receiptEvent(),
      commandEvent({ text: `${POINTER} `, commandId: "near-miss" }),
    ]),
    true,
  );
  const commands = index.commandsFor(POINTER);
  assert.deepEqual(
    commands.map((command) => command.commandId),
    ["team-wake-2abd9f9a"],
    "one trailing byte is a different pointer, never a normalised match",
  );
  assert.deepEqual(index.progressFor("team-wake-2abd9f9a"), {
    stage: "queued",
  });
  assert.equal(index.progressFor("unknown-command"), null);
  assert.equal(index.failureFor("team-wake-2abd9f9a"), null);
  assert.equal(index.overflowed, false);

  index.ingest([
    receiptEvent({ status: "turn_started", turnId: "turn-9", createdAt: 1 }),
  ]);
  assert.deepEqual(index.progressFor("team-wake-2abd9f9a"), {
    stage: "started",
    turnId: "turn-9",
  });
});

test("D-T5 (index): DUPLICATE_OPERATION refusals are disclosed apart from failures", () => {
  const index = new CodingSessionTeamWakeEvidenceIndex(CONTEXT);
  index.ingest([
    commandEvent({ commandId: "team-wake-v1:desktop" }),
    receiptEvent({
      commandId: "team-wake-v1:desktop",
      status: "turn_refused",
      error: {
        code: "DUPLICATE_OPERATION",
        message: "team-wake-2abd9f9a already owns this operation",
      },
    }),
  ]);
  assert.deepEqual(index.duplicateRefusedFor(POINTER), [
    "team-wake-v1:desktop",
  ]);
  assert.deepEqual(index.failureFor("team-wake-v1:desktop"), {
    code: "DUPLICATE_OPERATION",
    message: "team-wake-2abd9f9a already owns this operation",
    outcome: "refused",
  });
});

test("D-T4: the index is bounded and says so rather than dropping silently", () => {
  const index = new CodingSessionTeamWakeEvidenceIndex(CONTEXT, {
    maxCommands: 2,
    maxReceipts: 1,
  });
  index.ingest([
    commandEvent({ commandId: "a" }),
    commandEvent({ commandId: "b" }),
    commandEvent({ commandId: "c" }),
  ]);
  assert.equal(index.overflowed, true);
  assert.equal(index.commandsFor(POINTER).length, 2);
});
