import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionMissionPolicyRefusedOmitted,
  codingSessionMissionPolicyView,
  decodeCodingSessionPolicyFoldResult,
  MAX_CODING_SESSION_POLICY_REFUSED_ROWS,
} from "./codingSessionMissionPolicyView.ts";

const FOUNDER = "11".repeat(32);
const STRANGER = "22".repeat(32);
const SESSION = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const GENESIS = "ce5d87ed".repeat(8);

// The CLI's own sentence, verbatim (`sessions/policy.rs`).
const ENFORCEMENT =
  "a published policy is a stated intention, not an enforced limit: only budget.turns is enforced (at the provider's turn gate); every other field is read and shown, never counted";

function record(overrides = {}) {
  return {
    sessionRef: SESSION,
    genesisRef: GENESIS,
    posture: "ship",
    budget: {
      turns: 40,
      tokensPerSeat: null,
      tokensPerSession: null,
      costUsdPerSession: 25,
      contextTier: null,
    },
    attention: null,
    gates: null,
    bench: null,
    irreversible: null,
    stop: null,
    setsAnyPolicy: true,
    ...overrides,
  };
}

function fold(overrides = {}) {
  return {
    schema: "buzz-coding-session-policy-fold-adapter/v1",
    implementation: "buzz-core",
    selected: {
      eventId: "a5956d50".repeat(8),
      authorPubkey: FOUNDER,
      authorIsFounder: true,
      createdAt: 1_788_359_807,
      record: record(),
    },
    excluded: [],
    enforcement: ENFORCEMENT,
    ...overrides,
  };
}

test("L2.3: a set record prints its fields, and only budget.turns claims enforcement", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(fold()),
    founderPubkey: FOUNDER,
  });
  assert.equal(view.kind, "record");
  assert.equal(view.sentence, ENFORCEMENT);
  const enforced = view.facts.filter((fact) => fact.enforced);
  assert.deepEqual(
    enforced.map((fact) => fact.field),
    ["budget.turns"],
  );
  // A cost ceiling renders as a stated intention with a value and nothing else.
  const cost = view.facts.find(
    (fact) => fact.field === "budget.costUsdPerSession",
  );
  assert.equal(cost.value, "$25");
  assert.equal(cost.enforced, false);
});

test("L2.3: a withdrawal is a withdrawal, not `none`", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(
      fold({
        selected: {
          eventId: "b1".repeat(32),
          authorPubkey: FOUNDER,
          authorIsFounder: true,
          createdAt: 1_788_359_900,
          record: record({ setsAnyPolicy: false, budget: null, posture: null }),
        },
      }),
    ),
    founderPubkey: FOUNDER,
  });
  assert.equal(view.kind, "withdrawn");
  assert.equal(view.sentence, "Policy withdrawn by the founder");
});

test("L2.3: no record is `No policy set for this session`, and no fold is unknown", () => {
  const none = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(fold({ selected: null })),
  });
  assert.equal(none.kind, "none");
  assert.equal(none.sentence, "No policy set for this session");
  const unknown = codingSessionMissionPolicyView({ fold: null });
  assert.equal(unknown.kind, "unknown");
  assert.notEqual(unknown.sentence, none.sentence);
});

test("L2.3: a stranger's refused record is listed with author, code and reason", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(
      fold({
        excluded: [
          {
            eventId: "c3".repeat(32),
            authorPubkey: STRANGER,
            createdAt: 1_788_360_000,
            code: "unauthorized",
            reason:
              "signed by an identity that could not steer this umbrella when it was published",
          },
        ],
      }),
    ),
    founderPubkey: FOUNDER,
    resolveActorLabel: () => null,
  });
  assert.equal(view.refused.length, 1);
  assert.equal(view.refused[0].code, "unauthorized");
  assert.equal(view.refused[0].authorLabel, `${"22".repeat(4)}…2222`);
  assert.match(view.refused[0].reason, /could not steer/);
});

test("L2.3: the refused list is bounded and says what it dropped", () => {
  const excluded = Array.from({ length: 26 }, (_, index) => ({
    eventId: `${index.toString(16).padStart(2, "0")}`.repeat(32),
    authorPubkey: STRANGER,
    createdAt: 1_788_360_000 + index,
    code: "unauthorized",
    reason: "no standing",
  }));
  const decoded = decodeCodingSessionPolicyFoldResult(fold({ excluded }));
  const view = codingSessionMissionPolicyView({ fold: decoded });
  assert.equal(MAX_CODING_SESSION_POLICY_REFUSED_ROWS, 20);
  assert.equal(view.refused.length, 20);
  assert.equal(codingSessionMissionPolicyRefusedOmitted(decoded), 6);
});

test("L2.3: an adapter that stops disclosing a key is a loud failure", () => {
  for (const broken of [
    { ...fold(), excluded: undefined },
    { ...fold(), enforcement: undefined },
    { ...fold(), schema: "buzz-coding-session-policy-adapter/v1" },
    { ...fold(), implementation: "desktop" },
    {
      ...fold(),
      selected: { eventId: "a".repeat(64), authorPubkey: FOUNDER },
    },
  ]) {
    assert.throws(() => decodeCodingSessionPolicyFoldResult(broken));
  }
});

// ── Fix round 1: F5 (a refused-only fold), F1b (canonical truncation) ───────

test("F5: a fold that refused a stranger's ceiling and selected nothing says so", () => {
  const decoded = decodeCodingSessionPolicyFoldResult(
    fold({
      selected: null,
      excluded: [
        {
          eventId: "c3".repeat(32),
          authorPubkey: STRANGER,
          createdAt: 1_788_360_000,
          code: "unauthorized",
          reason:
            "signed by an identity that could not steer this umbrella when it was published",
        },
      ],
    }),
  );
  const view = codingSessionMissionPolicyView({
    fold: decoded,
    founderPubkey: FOUNDER,
  });
  assert.equal(view.kind, "none");
  // "Nobody set one" is exactly what a forged ceiling must not look like.
  assert.equal(view.sentence, "No policy in force · 1 refused");
  assert.equal(view.refused.length, 1);
  assert.equal(view.refused[0].code, "unauthorized");
  assert.equal(view.refused[0].authorLabel, `${"22".repeat(4)}…2222`);
  assert.match(view.refused[0].reason, /could not steer/);
});

test("F5: a genuinely empty fold still reads as no policy set", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(
      fold({ selected: null, excluded: [] }),
    ),
    founderPubkey: FOUNDER,
  });
  assert.equal(view.kind, "none");
  assert.equal(view.sentence, "No policy set for this session");
  assert.deepEqual(view.refused, []);
});

test("F5: more than one refusal is counted, not summarised away", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(
      fold({
        selected: null,
        excluded: [
          {
            eventId: "c3".repeat(32),
            authorPubkey: STRANGER,
            createdAt: 2,
            code: "unauthorized",
            reason: "no standing",
          },
          {
            eventId: "c4".repeat(32),
            authorPubkey: STRANGER,
            createdAt: 1,
            code: "undecodable",
            reason: "event signature is invalid",
          },
        ],
      }),
    ),
  });
  assert.equal(view.sentence, "No policy in force · 2 refused");
});

test("F1b: a refused record's author is the canonical truncation", () => {
  const view = codingSessionMissionPolicyView({
    fold: decodeCodingSessionPolicyFoldResult(
      fold({
        excluded: [
          {
            eventId: "c3".repeat(32),
            authorPubkey: STRANGER,
            createdAt: 1,
            code: "unauthorized",
            reason: "no standing",
          },
        ],
      }),
    ),
  });
  assert.equal(view.refused[0].authorLabel, `${"22".repeat(4)}…2222`);
});
