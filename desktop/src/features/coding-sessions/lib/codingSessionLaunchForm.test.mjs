import assert from "node:assert/strict";
import test from "node:test";

import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_COMMAND,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_POLICY,
} from "../../../shared/constants/kinds.ts";
import {
  CODING_SESSION_LAUNCH_LEAD_BLOCKER,
  codingSessionLaunchBlockersBySurface,
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
const UNSET = { kind: "unset" };

/** A Team form with everything answered. */
const READY = {
  mode: "team",
  goal: "Close ledger item 103.",
  goalOverflow: null,
  goalReader: "resolved",
  nameReader: "resolved",
  nameDirty: false,
  lead: AGENT,
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

/** The same form in Solo: you lead, the picker's model is yours. */
const READY_SOLO = {
  ...READY,
  mode: "solo",
  lead: YOU,
  leadModel: "claude-opus-5",
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
  assert.equal(codingSessionLeadIdentityLine(UNSET), "No lead picked yet");
});

test("a ready form has no blockers and launches, in either mode", () => {
  for (const input of [READY, READY_SOLO]) {
    const readiness = codingSessionLaunchReadiness(input);
    assert.deepEqual(readiness.blockers, [], input.mode);
    assert.equal(readiness.canLaunch, true, input.mode);
  }
});

test("every blocker is a sentence, and every blocker stops the launch", () => {
  const cases = [
    [{ goal: "   " }, "goal"],
    [{ goalOverflow: { bytes: 9000, cap: 8192 } }, "goal-cap"],
    [{ goalReader: "unresolved" }, "goal-unresolved"],
    [{ goalReader: "errored" }, "goal-unresolved"],
    [{ nameDirty: true, nameReader: "unresolved" }, "name-unresolved"],
    [{ lead: UNSET }, "lead"],
    [{ providerInstanceRef: null }, "provider"],
    [
      { providerRefusal: "This runtime refuses that model." },
      "provider-refusal",
    ],
    [{ modelOverridden: true, modelOverrideReason: "  " }, "override-reason"],
    [
      { projectReadiness: { allowed: false, reason: "No checkout." } },
      "project-readiness",
    ],
    [
      { unresolvedBenchIdentities: ["cd".repeat(32)] },
      `bench:${"cd".repeat(32)}`,
    ],
    [{ leadModel: null }, "model"],
    [{ busySentence: "Publishing the name…" }, "busy"],
  ];
  for (const [patch, id] of cases) {
    const readiness = codingSessionLaunchReadiness({ ...READY, ...patch });
    const blocker = readiness.blockers.find((entry) => entry.id === id);
    assert.ok(blocker, `${id} must be a blocker: ${JSON.stringify(patch)}`);
    assert.ok(blocker.sentence.length > 0, `${id} must say something`);
    assert.equal(readiness.canLaunch, false);
  }
});

test("mode table: Team with no agent picked is the lead blocker; Solo never is", () => {
  const team = codingSessionLaunchReadiness({ ...READY, lead: UNSET });
  const lead = team.blockers.find((entry) => entry.id === "lead");
  assert.ok(lead);
  assert.equal(lead.sentence, CODING_SESSION_LAUNCH_LEAD_BLOCKER);
  assert.match(lead.sentence, /or switch to Solo/);
  // An unset lead has no model to block on: the lead sentence already names
  // what to do, and a second sentence about a lead nobody picked would not.
  assert.equal(
    team.blockers.some((entry) => entry.id === "model"),
    false,
  );
  // Team with a "you" lead is the setup hook's impossibility, but readiness
  // still refuses it rather than seating a person as the agent lead.
  assert.ok(
    codingSessionLaunchReadiness({ ...READY, lead: YOU }).blockers.find(
      (entry) => entry.id === "lead",
    ),
  );
  const solo = codingSessionLaunchReadiness(READY_SOLO);
  assert.equal(
    solo.blockers.some((entry) => entry.id === "lead"),
    false,
  );
});

test("mode table: Solo never needs a model; both modes need a provider", () => {
  const solo = codingSessionLaunchReadiness({ ...READY_SOLO, leadModel: null });
  assert.equal(
    solo.blockers.some((entry) => entry.id === "model"),
    false,
  );
  assert.ok(solo.unknowns.find((entry) => entry.id === "model"));
  assert.equal(solo.canLaunch, true);
  for (const input of [READY, READY_SOLO]) {
    const readiness = codingSessionLaunchReadiness({
      ...input,
      providerInstanceRef: null,
    });
    assert.ok(
      readiness.blockers.find((entry) => entry.id === "provider"),
      `${input.mode} needs a provider`,
    );
  }
});

test("the goal reader gates Start: nothing is published over an unread goal", () => {
  const unresolved = codingSessionLaunchReadiness({
    ...READY_SOLO,
    goalReader: "unresolved",
  });
  const blocker = unresolved.blockers.find(
    (entry) => entry.id === "goal-unresolved",
  );
  assert.ok(blocker);
  assert.match(blocker.sentence, /^Goal not read yet\./);
  assert.equal(blocker.surface, undefined, "inline: the button is off");
  const errored = codingSessionLaunchReadiness({
    ...READY_SOLO,
    goalReader: "errored",
  });
  assert.match(
    errored.blockers.find((entry) => entry.id === "goal-unresolved").sentence,
    /could not be read from the relay/,
  );
});

test("a dirty name over an unread wire name blocks; a clean one does not", () => {
  const dirty = codingSessionLaunchReadiness({
    ...READY_SOLO,
    nameDirty: true,
    nameReader: "unresolved",
  });
  const blocker = dirty.blockers.find(
    (entry) => entry.id === "name-unresolved",
  );
  assert.ok(blocker);
  assert.equal(
    blocker.sentence,
    "Name not read yet. Start waits until the relay has answered, so it cannot publish over a name it has not seen.",
  );
  assert.equal(blocker.surface, undefined, "inline: the button is off");
  // Nothing typed, nothing to publish: an unread wire name blocks nothing.
  const clean = codingSessionLaunchReadiness({
    ...READY_SOLO,
    nameDirty: false,
    nameReader: "unresolved",
  });
  assert.equal(
    clean.blockers.some((entry) => entry.id === "name-unresolved"),
    false,
  );
  assert.equal(clean.canLaunch, true);
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

test("the ungoverned sentence is true of a founded session", () => {
  // A founded session has a genesis; what Solo lacks is the authority chain.
  const solo = codingSessionLaunchReadiness(READY_SOLO);
  const unknown = solo.unknowns.find((entry) => entry.id === "ungoverned");
  assert.ok(unknown);
  assert.equal(
    unknown.sentence,
    "Ungoverned: no authority chain, so this session has no roster, no grants and no signed reports.",
  );
  assert.doesNotMatch(unknown.sentence, /no genesis/);
  assert.equal(
    codingSessionLaunchReadiness(READY).unknowns.some(
      (entry) => entry.id === "ungoverned",
    ),
    false,
  );
});

test("the plan names the kinds a Team Start publishes, in order", () => {
  const plan = codingSessionLaunchPlan({
    mode: "team",
    lead: AGENT,
    nameDirty: false,
    promptDirty: false,
    policySet: true,
    benchCount: 2,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [44245, 44221, 44228, 44220],
  );
  assert.match(
    plan.find((line) => line.id === "create").sentence,
    /Seat Keystone as lead — the only seat this launch creates/,
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
    mode: "team",
    lead: AGENT,
    nameDirty: false,
    promptDirty: false,
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

test("a Solo Start publishes one create and says so; a policy cannot sneak in", () => {
  const plan = codingSessionLaunchPlan({
    mode: "solo",
    lead: YOU,
    nameDirty: false,
    promptDirty: false,
    policySet: true,
    benchCount: 3,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [44221],
  );
  assert.match(
    plan[0].sentence,
    /^One execution under this session, led by you, with the initial prompt as its first message\.$/,
  );
});

test("the dirty text fields come first in the plan, and only when dirty", () => {
  const both = codingSessionLaunchPlan({
    mode: "solo",
    lead: YOU,
    nameDirty: true,
    promptDirty: true,
    policySet: false,
    benchCount: 0,
  });
  assert.deepEqual(
    both.map((line) => [line.id, line.kind]),
    [
      ["name", KIND_CODING_SESSION_NAME],
      ["goal", KIND_CODING_SESSION_GOAL],
      ["create", KIND_CODING_SESSION_LIFECYCLE_COMMAND],
    ],
  );
  const promptOnly = codingSessionLaunchPlan({
    mode: "team",
    lead: AGENT,
    nameDirty: false,
    promptDirty: true,
    policySet: false,
    benchCount: 0,
  });
  assert.deepEqual(
    promptOnly.map((line) => line.id),
    ["goal", "create", "grants", "turn"],
  );
  // A line for an unchanged field would name a record Start never publishes.
  const clean = codingSessionLaunchPlan({
    mode: "team",
    lead: UNSET,
    nameDirty: false,
    promptDirty: false,
    policySet: false,
    benchCount: 0,
  });
  assert.deepEqual(
    clean.map((line) => line.id),
    ["create", "grants", "turn"],
  );
  // With no agent picked the create line does not invent a name.
  assert.match(clean[0].sentence, /Seat the agent picked above/);
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
  assert.deepEqual(
    resolveCodingSessionLeadModel({
      lead: UNSET,
      pickedModel: null,
      pickedExplicitly: false,
      providerModel: "claude-opus-5",
    }),
    { model: "claude-opus-5", overridden: false },
    "with no lead picked the picker's model is shown, and nothing is an override",
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

test("F7: every busy state is a blocker with a sentence", () => {
  // A disabled button with nothing under it is the item-79 shape this form
  // claims to have removed.
  const readiness = codingSessionLaunchReadiness({
    ...READY,
    busySentence: "Publishing the initial prompt…",
  });
  const blocker = readiness.blockers.find((entry) => entry.id === "busy");
  assert.ok(blocker);
  assert.equal(blocker.sentence, "Publishing the initial prompt…");
  assert.equal(readiness.canLaunch, false);
});

test("F8: the plan's kinds are the shared constants, never literals", () => {
  const plan = codingSessionLaunchPlan({
    mode: "team",
    lead: AGENT,
    nameDirty: true,
    promptDirty: true,
    policySet: true,
    benchCount: 1,
  });
  assert.deepEqual(
    plan.map((line) => line.kind),
    [
      KIND_CODING_SESSION_NAME,
      KIND_CODING_SESSION_GOAL,
      KIND_CODING_SESSION_POLICY,
      KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      KIND_CODING_SESSION_COMMAND,
    ],
  );
});

test("only the blank prompt is surfaced on press; every other blocker is inline", () => {
  const blank = codingSessionLaunchBlockersBySurface(
    codingSessionLaunchReadiness({ ...READY_SOLO, goal: "   " }),
  );
  assert.deepEqual(blank.inline, []);
  assert.deepEqual(
    blank.onAttempt.map((entry) => entry.id),
    ["goal"],
  );
  assert.equal(blank.canPress, true);
  const blankAndBusy = codingSessionLaunchBlockersBySurface(
    codingSessionLaunchReadiness({
      ...READY_SOLO,
      goal: "",
      busySentence: "Publishing this session…",
    }),
  );
  assert.deepEqual(
    blankAndBusy.inline.map((entry) => entry.id),
    ["busy"],
  );
  assert.equal(blankAndBusy.canPress, false);
  // …and the blank-prompt sentence is one sentence in both modes.
  const team = codingSessionLaunchReadiness({ ...READY, goal: "" });
  const blocker = team.blockers.find((entry) => entry.id === "goal");
  assert.equal(
    blocker.sentence,
    "Please specify the initial prompt to start the session.",
  );
  assert.equal(blocker.surface, "attempt");
});

test("the Team lead and an unnamed worktree are surfaced on press, not inline", () => {
  const base = {
    mode: "team",
    goal: "Close ledger item 104.",
    goalOverflow: null,
    goalReader: "resolved",
    nameReader: "resolved",
    nameDirty: false,
    lead: { kind: "unset" },
    providerInstanceRef: "prov",
    providerAuthorityPubkey: "ab12cd34".repeat(8),
    leadModel: null,
    modelOverrideReason: "",
    modelOverridden: false,
    providerRefusal: null,
    projectReadiness: null,
    projectReadinessUnknown: false,
    policySet: false,
    unresolvedBenchIdentities: [],
    busySentence: null,
    useWorktree: true,
    worktreeName: "",
  };
  const readiness = codingSessionLaunchReadiness(base);
  const surfaces = codingSessionLaunchBlockersBySurface(readiness);
  assert.deepEqual(
    surfaces.onAttempt.map((blocker) => blocker.id),
    ["lead", "worktree-name"],
  );
  assert.deepEqual(surfaces.inline, []);
  assert.equal(
    surfaces.canPress,
    true,
    "Start stays pressable so the press can say it",
  );
  assert.equal(readiness.canLaunch, false);
  const named = codingSessionLaunchReadiness({
    ...base,
    lead: {
      kind: "agent",
      actor: "a".repeat(64),
      label: "Fable",
      role: "lead",
      model: "opus",
    },
    leadModel: "opus",
    worktreeName: "fresh-work",
  });
  assert.equal(codingSessionLaunchBlockersBySurface(named).onAttempt.length, 0);
  const off = codingSessionLaunchReadiness({
    ...base,
    useWorktree: false,
    lead: {
      kind: "agent",
      actor: "a".repeat(64),
      label: "Fable",
      role: "lead",
      model: "opus",
    },
    leadModel: "opus",
  });
  assert.equal(
    off.blockers.some((blocker) => blocker.id === "worktree-name"),
    false,
  );
});
