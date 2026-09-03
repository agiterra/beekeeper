import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA,
  codingSessionDecisionAnswerContent,
  decodeCodingSessionTeamTransactionBuildResult,
  decodeCodingSessionTeamTransactionCapabilities,
  publishCodingSessionDecisionAnswer,
} from "./codingSessionTeamTransactionPublish.ts";

/**
 * The adapter's own output, not a hand-written shape. A hand-written fixture is
 * what let the team-fold decoder stay green while it would have thrown for
 * every real session (REVIEW-B1c B1).
 */
const FIXTURE = JSON.parse(
  readFileSync(
    new URL(
      "./codingSessionTeamTransactionAdapterResponse.fixture.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS = "ab".repeat(32);
const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const REQUEST = "22".repeat(32);

function deps(overrides = {}) {
  const calls = { built: [], signed: [], published: [] };
  const base = {
    capabilities: async () => ({
      schema: CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA,
      implementation: "buzz-core",
      choiceMaxBytes: 2048,
      noteMaxBytes: 8192,
      conditionMaxBytes: 512,
      supportsDecisionAnswerCondition: false,
    }),
    buildEvent: async (input) => {
      calls.built.push(input);
      return FIXTURE.build;
    },
    signer: async (unsigned) => {
      calls.signed.push(unsigned);
      return { ...unsigned, id: "11".repeat(32), pubkey: "cd".repeat(32) };
    },
    publisher: {
      publishEvent: async (event) => {
        calls.published.push(event);
        return { id: event.id };
      },
    },
  };
  return { calls, deps: { ...base, ...overrides } };
}

test("L8.1: the build fixture the adapter generates is what the decoder accepts", () => {
  const built = decodeCodingSessionTeamTransactionBuildResult(FIXTURE.build);
  assert.equal(built.kind, 44244);
  assert.equal(built.record.type, "decision.answer");
  assert.equal(built.tags.length, 5);
  const capabilities = decodeCodingSessionTeamTransactionCapabilities(
    FIXTURE.capabilities,
  );
  assert.equal(typeof capabilities.supportsDecisionAnswerCondition, "boolean");
  assert.ok(capabilities.choiceMaxBytes > 0);
});

test("L8.1: a decoder that loses a key is a loud failure, never a silent unset", () => {
  const { supportsDecisionAnswerCondition, ...missing } = FIXTURE.capabilities;
  assert.equal(typeof supportsDecisionAnswerCondition, "boolean");
  assert.throws(
    () => decodeCodingSessionTeamTransactionCapabilities(missing),
    /malformed capabilities/,
  );
  assert.throws(
    () =>
      decodeCodingSessionTeamTransactionBuildResult({
        ...FIXTURE.build,
        implementation: "desktop",
      }),
    /malformed build response/,
  );
});

test("L8.1: an answer publishes Rust's own bytes, and TypeScript signs nothing it wrote", async () => {
  const { calls, deps: wiring } = deps();
  const published = await publishCodingSessionDecisionAnswer({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    draft: { requestRef: REQUEST, choice: 1, note: null, condition: null },
    deps: wiring,
  });

  assert.equal(calls.built.length, 1);
  assert.equal(calls.built[0].channelRef, CHANNEL);
  assert.deepEqual(Object.keys(calls.built[0].transaction.body), [
    "requestRef",
    "choice",
    "note",
  ]);
  // The signed content is the adapter's string, byte for byte.
  assert.equal(calls.signed[0].content, FIXTURE.build.content);
  assert.equal(calls.signed[0].kind, 44244);
  assert.equal(calls.published.length, 1);
  assert.equal(published.eventId, "11".repeat(32));
  assert.equal(published.conditionSent, false);
});

test("L8.4: `condition` is omitted when the decoder does not carry the key", () => {
  const without = codingSessionDecisionAnswerContent({
    draft: { requestRef: REQUEST, choice: 0, note: null, condition: null },
    sessionRef: SESSION,
    genesisRef: GENESIS,
    supportsCondition: false,
  });
  assert.deepEqual(Object.keys(without.body), ["requestRef", "choice", "note"]);

  const with_ = codingSessionDecisionAnswerContent({
    draft: {
      requestRef: REQUEST,
      choice: 0,
      note: null,
      condition: "every commit on this branch",
    },
    sessionRef: SESSION,
    genesisRef: GENESIS,
    supportsCondition: true,
  });
  assert.deepEqual(Object.keys(with_.body), [
    "requestRef",
    "choice",
    "note",
    "condition",
  ]);
  assert.equal(with_.body.condition, "every commit on this branch");
});

test("L8.4: a condition typed against a core that refuses the key is never silently dropped", async () => {
  const { calls, deps: wiring } = deps();
  await assert.rejects(
    publishCodingSessionDecisionAnswer({
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      draft: {
        requestRef: REQUEST,
        choice: 0,
        note: null,
        condition: "every commit on this branch",
      },
      deps: wiring,
    }),
    /cannot carry a condition/,
  );
  assert.equal(calls.built.length, 0, "nothing is built");
  assert.equal(calls.published.length, 0, "nothing reaches the relay");
});

test("L8.1: a rejected publish throws the relay's own words and signs nothing twice", async () => {
  const { calls, deps: wiring } = deps({
    publisher: {
      publishEvent: async () => {
        throw new Error("blocked: not a member of this channel");
      },
    },
  });
  await assert.rejects(
    publishCodingSessionDecisionAnswer({
      channelRef: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      draft: { requestRef: REQUEST, choice: 0, note: null, condition: null },
      deps: wiring,
    }),
    /blocked: not a member of this channel/,
  );
  assert.equal(calls.signed.length, 1);
});
