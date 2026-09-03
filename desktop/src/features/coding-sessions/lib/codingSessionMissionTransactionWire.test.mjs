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

// --- B1c: the decoder must accept the two state-free verbs with exact keys,
// and refuse the terminal that clears itself.

const REF = "11".repeat(32);
const REQUEST = "22".repeat(32);

function envelope(type, body, supersedes = null) {
  return {
    schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type,
    supersedes,
    deliveryCommandId: null,
    body,
  };
}

function decode(type, body, supersedes = null) {
  return decodeCodingSessionTeamTransactionContent(
    JSON.stringify(envelope(type, body, supersedes)),
  );
}

test("note decodes with exact keys and bounded, unique, non-causal refs", () => {
  assert.equal(
    decode("note", { text: "nothing is blocked", refs: [] }).ok,
    true,
  );
  assert.equal(
    decode("note", { text: "nothing is blocked", refs: [REF] }).ok,
    true,
  );
  assert.equal(
    decode("note", {
      text: "x".repeat(8 * 1024),
      refs: Array.from({ length: 16 }, (_, index) =>
        index.toString(16).padStart(64, "0"),
      ),
    }).ok,
    true,
  );

  for (const body of [
    { text: "said" },
    { refs: [] },
    { text: "said", refs: [], extra: 1 },
    { text: "", refs: [] },
    { text: "   ", refs: [] },
    { text: "x".repeat(8 * 1024 + 1), refs: [] },
    { text: "said", refs: ["nothex"] },
    { text: "said", refs: [REF, REF] },
    {
      text: "said",
      refs: Array.from({ length: 17 }, (_, index) =>
        index.toString(16).padStart(64, "0"),
      ),
    },
  ]) {
    assert.equal(
      decode("note", body).ok,
      false,
      `note body must be refused: ${JSON.stringify(body)}`,
    );
  }

  // A note's refs are pointers, not causal references, so a note may point at
  // its own event id without self-referencing.
  const event = transaction(envelope("note", { text: "said", refs: [] }));
  const selfPointing = transaction(
    envelope("note", { text: "said", refs: [event.id] }),
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: selfPointing,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    true,
  );
});

test("a note never supersedes another record", () => {
  const refused = decode("note", { text: "said", refs: [] }, REF);
  assert.equal(refused.ok, false);
  assert.equal(refused.error, "a note never supersedes another record");
});

test("decision.request decodes with exact keys, heldOn vocabulary and bounds", () => {
  const good = {
    question: "Ship the CLI fix now, or after the app rebuild?",
    options: ["now", "after the rebuild"],
    heldOn: "founder",
    blocks: [REF],
    recommendation: "after the rebuild",
  };
  assert.equal(decode("decision.request", good).ok, true);
  assert.equal(decode("decision.request", { ...good, heldOn: LEAD }).ok, true);
  assert.equal(
    decode("decision.request", {
      ...good,
      options: [],
      blocks: [],
      recommendation: null,
    }).ok,
    true,
  );

  for (const body of [
    { ...good, heldOn: "lead" },
    { ...good, heldOn: "Founder" },
    { ...good, heldOn: "" },
    { ...good, options: ["a", "a"] },
    { ...good, options: Array.from({ length: 9 }, (_, i) => `option ${i}`) },
    { ...good, options: ["x".repeat(513)] },
    {
      ...good,
      blocks: Array.from({ length: 17 }, (_, index) =>
        index.toString(16).padStart(64, "0"),
      ),
    },
    { ...good, blocks: [REF, REF] },
    { ...good, recommendation: "x".repeat(2 * 1024 + 1) },
    { ...good, extra: 1 },
    (() => {
      const { recommendation, ...rest } = good;
      return rest;
    })(),
  ]) {
    assert.equal(
      decode("decision.request", body).ok,
      false,
      `decision.request body must be refused: ${JSON.stringify(body)}`,
    );
  }
});

test("decision.answer takes an option index or bounded text, never both shapes wrong", () => {
  const good = { requestRef: REQUEST, choice: 0, note: null, condition: null };
  assert.equal(decode("decision.answer", good).ok, true);
  assert.equal(decode("decision.answer", { ...good, choice: 7 }).ok, true);
  assert.equal(
    decode("decision.answer", { ...good, choice: "neither; hold" }).ok,
    true,
  );
  assert.equal(
    decode("decision.answer", { ...good, note: "the sidecar is stale" }).ok,
    true,
  );

  for (const body of [
    { ...good, choice: 8 },
    { ...good, choice: -1 },
    { ...good, choice: 1.5 },
    { ...good, choice: "" },
    { ...good, choice: "x".repeat(2 * 1024 + 1) },
    { ...good, choice: null },
    { ...good, choice: true },
    { ...good, choice: ["now"] },
    { ...good, requestRef: "nothex" },
    { ...good, extra: 1 },
    { requestRef: REQUEST, choice: 0 },
    { ...good, condition: "" },
    { ...good, condition: "   " },
    { ...good, condition: "x".repeat(513) },
    { ...good, condition: 1 },
  ]) {
    assert.equal(
      decode("decision.answer", body).ok,
      false,
      `decision.answer body must be refused: ${JSON.stringify(body)}`,
    );
  }
});

test("a mission.blocked correction naming no blocker is refused with the remedy", () => {
  // Keystone's exact shape on 2026-09-01.
  const clearing = {
    assignmentRefs: [],
    summary: "Nothing is blocked; work resumed.",
    blockers: [],
    heldOn: null,
    requiredAction: "None — the lanes are running again.",
  };
  const refused = decode("mission.blocked", clearing, REF);
  assert.equal(refused.ok, false);
  assert.equal(
    refused.error,
    "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself",
  );

  // Without `supersedes` the same body keeps its older refusal; the rule adds
  // a remedy, it relaxes nothing.
  assert.equal(decode("mission.blocked", clearing).ok, false);
  // A correction that still names a blocker stays legitimate.
  assert.equal(
    decode(
      "mission.blocked",
      { ...clearing, blockers: ["the keychain is locked"] },
      REF,
    ).ok,
    true,
  );
});

test("the new verbs verify through the exact five-tag envelope", () => {
  for (const [type, body] of [
    ["note", { text: "said", refs: [] }],
    [
      "decision.request",
      {
        question: "Ship now?",
        options: [],
        heldOn: "founder",
        blocks: [],
        recommendation: null,
      },
    ],
    [
      "decision.answer",
      { requestRef: REQUEST, choice: 0, note: null, condition: null },
    ],
  ]) {
    const event = transaction(envelope(type, body));
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    });
    assert.equal(decoded.ok, true, `${type} must verify`);
    assert.equal(decoded.value.payload.type, type);
    assert.deepEqual(event.tags[4], ["cstx-type", type]);
  }

  // A decision.answer cannot answer itself; a decision.request cannot block
  // itself. Both are causal references.
  const answer = transaction(
    envelope("decision.answer", {
      requestRef: REQUEST,
      choice: 0,
      note: null,
      condition: null,
    }),
  );
  const selfAnswer = transaction(
    envelope("decision.answer", {
      requestRef: answer.id,
      choice: 0,
      note: null,
      condition: null,
    }),
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: selfAnswer,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    true,
  );
});

test("a request may point at its own event id, because blocks is not causal", () => {
  // REVIEW-B1c F3: `blocks` are pointers. Rust's `causal_references` returns
  // nothing for a request, so the self-reference guard must agree — otherwise
  // the two surfaces disagree about which records are decodable at all.
  const request = transaction(
    envelope("decision.request", {
      question: "Ship it?",
      options: [],
      heldOn: "founder",
      blocks: [],
      recommendation: null,
    }),
  );
  const selfBlocking = transaction(
    envelope("decision.request", {
      question: "Ship it?",
      options: [],
      heldOn: "founder",
      blocks: [request.id],
      recommendation: null,
    }),
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: selfBlocking,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    true,
  );
  // An answer's requestRef IS causal, so naming its own id is still refused.
  const answer = transaction(
    envelope("decision.answer", {
      requestRef: REQUEST,
      choice: 0,
      note: null,
      condition: null,
    }),
  );
  const selfAnswer = transaction(
    envelope("decision.answer", {
      requestRef: answer.id,
      choice: 0,
      note: null,
      condition: null,
    }),
  );
  assert.equal(
    decodeVerifiedCodingSessionTeamTransaction({
      event: selfAnswer,
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
    }).ok,
    true,
  );
});

test("a decision.answer may carry the class its ruling covers", () => {
  // Finding 21: the same ruling was asked for twice because the first answer
  // was about one SHA. `condition` names the class instead — text a reader
  // reads, never a predicate this surface evaluates.
  const condition = "any SHA whose buzz-acp diff against origin/main is empty";
  const decoded = decode("decision.answer", {
    requestRef: REQUEST,
    choice: 0,
    note: null,
    condition,
  });
  assert.equal(decoded.ok, true);
  assert.equal(decoded.value.body.condition, condition);
});

test("a decision.answer signed before `condition` existed still decodes", () => {
  // Live run 2's own body, byte-for-byte off hive: `4847ff06…` in channel
  // d3e440ea…, the founder's answer to 2099cdb3…, published before the key
  // existed. `condition` is required on WRITE and optional on READ, because a
  // reader must never lose history.
  const live = {
    requestRef: REQUEST,
    choice: 2,
    note: "C. Role always present, null outside a session.",
  };
  const decoded = decode("decision.answer", live);
  assert.equal(decoded.ok, true, "the six-key body must decode");
  // Absent and null are the same answer, and nothing downstream can tell them
  // apart.
  assert.equal(decoded.value.body.condition ?? null, null);
  assert.equal(
    decode("decision.answer", { ...live, condition: null }).value.body
      .condition,
    null,
  );

  // Optional does not mean lax: an unknown key is still refused on both
  // shapes, and a required key is still required.
  for (const bad of [
    { ...live, extra: 1 },
    { ...live, condition: null, extra: 1 },
    { choice: 2, note: null, condition: null },
    { requestRef: REQUEST, choice: 2, condition: null },
    { ...live, condition: "" },
    { ...live, condition: "x".repeat(513) },
  ]) {
    assert.equal(
      decode("decision.answer", bad).ok,
      false,
      `decision.answer body must be refused: ${JSON.stringify(bad)}`,
    );
  }
});
