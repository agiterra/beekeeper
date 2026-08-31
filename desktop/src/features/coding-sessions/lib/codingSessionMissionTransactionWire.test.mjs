import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import {
  CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
  decodeCodingSessionTeamTransactionContent,
  decodeVerifiedCodingSessionTeamTransaction,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
} from "./codingSessionTeamTransactionWire.ts";

const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const SESSION = "683d55b3-d34e-410d-874a-55f9082d2631";
const GENESIS = "ab".repeat(32);
const OTHER_GENESIS = "ef".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const LEAD_SECRET = new Uint8Array(32).fill(2);
const LEAD = getPublicKey(LEAD_SECRET);

function sign({ kind, content, tags, secret = FOUNDER_SECRET, createdAt = 1 }) {
  return finalizeEvent({ kind, content, tags, created_at: createdAt }, secret);
}

function transaction(payload, secret = FOUNDER_SECRET, createdAt = 1) {
  return sign({
    kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
    content: JSON.stringify(payload),
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", GENESIS],
      ["cstx-type", payload.type],
    ],
    secret,
    createdAt,
  });
}

test("desktop decoder runs the canonical shared CSTX schema vectors", () => {
  const vectorPath = resolve(
    process.cwd(),
    "../conformance/coding-session-team-transaction/fixtures/schema-vectors.json",
  );
  const fixture = JSON.parse(readFileSync(vectorPath, "utf8"));
  assert.equal(
    fixture.schema,
    "buzz-coding-session-team-transaction-conformance/v1",
  );
  for (const vector of fixture.vectors) {
    assert.equal(
      decodeCodingSessionTeamTransactionContent(JSON.stringify(vector.content))
        .ok,
      vector.valid,
      vector.name,
    );
  }
});

test("transaction ingress requires signature, exact tags, and one session/genesis", () => {
  const payload = {
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "mission.blocked",
    supersedes: null,
    deliveryCommandId: null,
    body: {
      assignmentRefs: [],
      summary: "Signing is held",
      blockers: ["Keychain is locked"],
      heldOn: "founder",
      requiredAction: "Unlock signing keys",
    },
  };
  const event = transaction(payload);
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    true,
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: { ...event, sig: "00".repeat(64) },
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    false,
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: OTHER_GENESIS,
    }).ok,
    false,
  );
  const duplicate = JSON.stringify(payload).replace(
    '"schema":',
    `"schema":"${CODING_SESSION_TEAM_TRANSACTION_SCHEMA}","schema":`,
  );
  assert.equal(decodeCodingSessionTeamTransactionContent(duplicate).ok, false);

  const extraTag = sign({
    kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
    content: event.content,
    tags: [...event.tags, ["p", LEAD]],
  });
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: extraTag,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    false,
  );

  const oversizedBrief = transaction({
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "assignment",
    supersedes: null,
    deliveryCommandId: null,
    body: {
      assigneeActor: LEAD,
      assigneeRole: "builder",
      objective: "Stay bounded",
      brief: "x".repeat(32 * 1024 + 1),
      branch: null,
      baseSha: null,
      fileOwnership: [],
      acceptanceSteps: [],
    },
  });
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: oversizedBrief,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    false,
  );

  const emptyAcceptance = transaction({
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "assignment",
    supersedes: null,
    deliveryCommandId: null,
    body: {
      assigneeActor: LEAD,
      assigneeRole: "builder",
      objective: "Stay bounded",
      brief: "Use the canonical fold.",
      branch: null,
      baseSha: null,
      fileOwnership: [],
      acceptanceSteps: [],
    },
  });
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: emptyAcceptance,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    false,
  );
});
