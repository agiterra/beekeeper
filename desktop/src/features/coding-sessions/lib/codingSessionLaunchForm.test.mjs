import assert from "node:assert/strict";
import test from "node:test";

import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_COMMAND,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_POLICY,
} from "../../../shared/constants/kinds.ts";
import {
  codingSessionGovernedLockReason,
  codingSessionLaunchIsGoverned,
  codingSessionLaunchPlan,
  codingSessionLaunchReadiness,
  codingSessionLeadIdentityLine,
  resolveCodingSessionLeadModel,
} from "./codingSessionLaunchForm.ts";

const AGENT = {
  kind: "agent",
  actor: "a1".repeat(32),
  label: "Keystone",
  role: "lead",
  model: "opus",
};
const YOU = { kind: "you", label: "You" };

const READY = {
  channelId: "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
  canCreateChannel: false,
  goal: "Close ledger item 103.",
  goalOverflow: null,
  lead: AGENT,
  governed: true,
  providerInstanceRef: "claude-primary",
  providerAuthorityPubkey: "ab".repeat(32),
  leadModel: "opus",
  modelOverrideReason: null,
  modelOverridden: false,
  providerRefusal: null,
  projectReadiness: null,
  projectReadinessUnknown: false,
  policySet: false,
  unresolvedBenchIdentities: [],
  busySentence: null,
};

test("two identities with one name are told apart by the identity line", () => {
  // The crew installer offers a name field per pack and "Keystone" is the
  // obvious thing to type twice; this is the screen where picking the wrong
  // one costs a whole session.
  const other = { ...AGENT, actor: "b2".repeat(32) };
  const first = codingSessionLeadIdentityLine(AGENT);
  const second = codingSessionLeadIdentityLine(other);
  assert.notEqual(first, second);
  assert.match(first, /^Keystone · /);
  // The canonical short form, not a hand-rolled slice.
  assert.match(first, /a1a1a1a1…a1a1$/);
  assert.equal(codingSessionLeadIdentityLine(YOU), "You");
});

test("governed is decided by who leads, and says so either way", () => {
  assert.equal(codingSessionLaunchIsGoverned(AGENT), true);
  assert.equal(codingSessionLaunchIsGoverned(YOU), false);
  assert.match(codingSessionGovernedLockReason(AGENT), /always governed/);
  assert.match(codingSessionGovernedLockReason(YOU), /Pick an agent to lead/);
});

test("a ready form has no blockers and launches", () => {
  const readiness = codingSessionLaunchReadiness(READY);
  assert.deepEqual(readiness.blockers, []);
  assert.equal(readiness.canLaunch, true);
});

test("every blocker is a sentence, and every blocker stops the launch", () => {
  const cases = [
    [{ goal: "   " }, "goal"],
    [{ goalOverflow: { bytes: 9000, cap: 8192 } }, "goal-cap"],
    [{ providerInstanceRef: null }, "provider"],
    [{ channelId: null, canCreateChannel: false }, "channel"],
    [{ modelOverridden: true, modelOverrideReason: "  " }, "override-reason"],
    [
      { projectReadiness: { allowed: false, reason: "No checkout." } },
      "project-readiness",
    ],
    [
      { unresolvedBenchIdentities: ["cd".repeat(32)] },
      `bench:${"cd".repeat(32)}`,
    ],
  ];
  for (const [patch, id] of cases) {
    const readiness = codingSessionLaunchReadiness({ ...READY, ...patch });
    const blocker = readiness.blockers.find((entry) => entry.id === id);
    assert.ok(blocker, `${id} must be a blocker: ${JSON.stringify(patch)}`);
    assert.ok(blocker.sentence.length > 0, `${id} must say something`);
    assert.equal(readiness.canLaunch, false);
  }
});

test("an unknown is disclosed and never blocks", () => {
  // "This computer could not check" and "it is fine" are different facts, and
  // only the first one is ever an excuse for silence.
  const readiness = codingSessionLaunchReadiness({
    ...READY,
    projectReadinessUnknown: true,
    policySet: true,
    lead: { ...AGENT, hasRolePack: false },
    leadHasRolePack: false,
  });
  assert.deepEqual(readiness.blockers, []);
  assert.equal(readiness.canLaunch, true);
  const ids = readiness.unknowns.map((entry) => entry.id).sort();
  assert.deepEqual(ids, ["policy", "project-readiness", "role-pack-missing"]);
  assert.match(
    readiness.unknowns.find((entry) => entry.id === "policy").sentence,
    /Turn budgets and verifier\/required-gate settings have scoped consumers/,
  );
});

test("nobody asked about the role pack is its own unknown, not a finding", () => {
  const readiness = codingSessionLaunchReadiness({ ...READY, lead: AGENT });
  const unknown = readiness.unknowns.find((entry) => entry.id === "role-pack");
  assert.ok(unknown);
  assert.match(unknown.sentence, /Nobody asked/);
  // …and it is a different sentence from "this computer holds no pack".
  const missing = codingSessionLaunchReadiness({
    ...READY,
    lead: { ...AGENT, hasRolePack: false },
    leadHasRolePack: false,
  }).unknowns.find((entry) => entry.id === "role-pack-missing");
  assert.notEqual(unknown.sentence, missing.sentence);
});

test("the plan names the kinds a governed launch publishes, in order", () => {
  const plan = codingSessionLaunchPlan({
    governed: true,
    lead: AGENT,
    goal: "Close it.",
    policySet: true,
    benchCount: 2,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [44226, 44227, 44245, 44221, 44228, 44220],
  );
  assert.match(
    plan.find((line) => line.id === "create").sentence,
    /the only seat this launch creates/,
  );
  assert.match(
    plan.find((line) => line.id === "policy").sentence,
    /turn budgets and verifier gates have scoped effects/i,
  );
  assert.match(
    plan.find((line) => line.id === "turn").sentence,
    /2 identities the lead may hire/,
  );
});

test("a launch that sets no policy publishes no 44245", () => {
  // The withdrawal record is a deliberate act of taking a policy back, never
  // the default shape of a session nobody wrote a policy for.
  const plan = codingSessionLaunchPlan({
    governed: true,
    lead: AGENT,
    goal: "Close it.",
    policySet: false,
    benchCount: 0,
  });
  assert.equal(
    plan.some((line) => line.kind === 44245),
    false,
  );
  assert.match(
    plan.find((line) => line.id === "turn").sentence,
    /Nobody is benched/,
  );
});

test("an ungoverned launch publishes one create and says so", () => {
  const plan = codingSessionLaunchPlan({
    governed: false,
    lead: YOU,
    goal: "Poke at it.",
    policySet: false,
    benchCount: 0,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [44221],
  );
});

// ── REVIEW-B3 F1/F2/F7/F8 ───────────────────────────────────────────────────

test("F1: a lead's model comes from its identity, or from an explicit pick", () => {
  // The live path, which is where finding 12 actually lives. Removing
  // `fallbackModel` from `resolveCodingSessionCrewSeats` closed the leak on a
  // function the form does not call; this is the one it does.
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: AGENT,
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "claude-opus-5",
    }),
    { model: "opus", overridden: false },
    "the identity's own model wins over anything the picker is showing",
  );
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: { ...AGENT, model: null },
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "claude-opus-5",
    }),
    { model: null, overridden: false },
    "an identity that declares none must not borrow the picker's default",
  );
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: { ...AGENT, model: null },
      pickedModel: "haiku",
      pickedExplicitly: true,
      providerModel: "claude-opus-5",
    }),
    { model: "haiku", overridden: true },
    "an explicit pick is the model, and it is an override",
  );
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: YOU,
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "claude-opus-5",
    }),
    { model: "claude-opus-5", overridden: false },
    "when you lead, the picker IS your choice and overrides nothing",
  );
});

test("F2: an empty model is not a model, on either path", () => {
  // `bootstrapClaudeCodingSessionRuntime` publishes `defaultModel: ""` until
  // the models command answers. That used to reach the create builder on the
  // governed branch and throw `action.model must not be empty` — after the
  // genesis, the goal and the policy were already on the wire.
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: { ...AGENT, model: "  " },
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "",
    }),
    { model: null, overridden: false },
  );
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: YOU,
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "",
    }),
    { model: null, overridden: false },
  );
});

test("F1: an agent lead with no model blocks the launch, and says what to do", () => {
  const readiness = codingSessionLaunchReadiness({ ...READY, leadModel: null });
  const blocker = readiness.blockers.find((entry) => entry.id === "model");
  assert.ok(blocker, "an agent lead with no settled model must be blocked");
  assert.match(blocker.sentence, /Keystone declares no model/);
  assert.match(blocker.sentence, /recorded as an override/);
  assert.equal(readiness.canLaunch, false);
  // …and it is a blocker, not an unknown that a person would never see.
  assert.equal(
    readiness.unknowns.some((entry) => entry.id === "model"),
    false,
  );
});

test("F1: leading it yourself with no model is an unknown, not a blocker", () => {
  const readiness = codingSessionLaunchReadiness({
    ...READY,
    lead: YOU,
    governed: false,
    leadModel: null,
  });
  assert.equal(
    readiness.blockers.some((entry) => entry.id === "model"),
    false,
  );
  assert.ok(readiness.unknowns.find((entry) => entry.id === "model"));
  assert.equal(readiness.canLaunch, true);
});

test("F7: every busy state is a blocker with a sentence", () => {
  // A disabled button with nothing under it is the item-79 shape this form
  // claims to have removed, and during channel preparation it still was.
  const readiness = codingSessionLaunchReadiness({
    ...READY,
    busySentence: "Preparing this project's sessions channel…",
  });
  const blocker = readiness.blockers.find((entry) => entry.id === "busy");
  assert.ok(blocker);
  assert.equal(blocker.sentence, "Preparing this project's sessions channel…");
  assert.equal(readiness.canLaunch, false);
});

test("F8: the plan's kinds are the shared constants, never literals", () => {
  const plan = codingSessionLaunchPlan({
    governed: true,
    lead: AGENT,
    goal: "Close it.",
    policySet: true,
    benchCount: 1,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [
      KIND_CODING_SESSION_GENESIS,
      KIND_CODING_SESSION_GOAL,
      KIND_CODING_SESSION_POLICY,
      KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      KIND_CODING_SESSION_COMMAND,
    ],
  );
});
