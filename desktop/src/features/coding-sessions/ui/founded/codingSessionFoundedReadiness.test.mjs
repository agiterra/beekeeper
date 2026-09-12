import assert from "node:assert/strict";
import test from "node:test";

import {
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_POLICY,
} from "../../../../shared/constants/kinds.ts";
import {
  codingSessionFoundedBusySentence,
  codingSessionFoundedReadiness,
} from "./codingSessionFoundedReadiness.ts";

const IDLE = {
  governed: false,
  channelSentence: null,
  isPublishing: false,
  isLaunching: false,
  isPreparing: false,
  transactionPending: false,
  isLaunchPreflighting: false,
  isPreparingRoles: false,
  isScanning: false,
};

const AGENT = {
  kind: "agent",
  actor: "a1".repeat(32),
  label: "Keystone",
  role: "lead",
  model: "opus",
};

const READY = {
  mode: "solo",
  goal: "Close it.",
  goalOverflow: null,
  goalReader: "resolved",
  nameResolved: true,
  lead: { kind: "you", label: "You" },
  providerInstanceRef: "claude-primary",
  providerAuthorityPubkey: "ab".repeat(32),
  leadModel: "claude-opus-5",
  modelOverrideReason: "",
  modelOverridden: false,
  providerRefusal: null,
  projectRef: null,
  useRoles: false,
  readinessGate: { allowed: true, reason: null },
  teamReadinessLoading: false,
  teamReadinessError: null,
  policySet: false,
  benchCount: 0,
  unresolvedBenchIdentities: [],
  busySentence: null,
  nameDirty: false,
  promptDirty: false,
};

test("every busy state is a sentence, in precedence order, and idle is null", () => {
  assert.equal(codingSessionFoundedBusySentence(IDLE), null);
  const cases = [
    [{ starting: true }, /Starting again would seat a second lead/],
    [{ isPublishing: true }, /^Publishing this session…$/],
    [{ isLaunching: true }, /^Starting…$/],
    [{ isPreparing: true }, /^Cutting the worktree…$/],
    [{ isPreparing: true, governed: true }, /^Cutting the lead's worktree…$/],
    [
      { channelSentence: "Reading the channel's project…" },
      /^Reading the channel's project…$/,
    ],
    [{ transactionPending: true }, /already in flight/],
    [{ isLaunchPreflighting: true }, /Re-checking readiness/],
    [{ isPreparingRoles: true }, /Preparing the project's roles/],
    [{ isScanning: true }, /Scanning the project's role packs/],
  ];
  for (const [patch, words] of cases) {
    assert.match(
      codingSessionFoundedBusySentence({ ...IDLE, ...patch }) ?? "",
      words,
    );
  }
  // A held Start outranks the create. A field's own publish is not a busy
  // state at all: it is said under the field, so a Start pressed during a
  // blur-publish is not disabled under the cursor.
  assert.match(
    codingSessionFoundedBusySentence({
      ...IDLE,
      starting: true,
      isPublishing: true,
    }),
    /second lead/,
  );
});

test("the Solo plan is one create line, plus the dirty text lines first", () => {
  const clean = codingSessionFoundedReadiness(READY);
  assert.deepEqual(clean.readiness.blockers, []);
  assert.equal(clean.readiness.canLaunch, true);
  assert.deepEqual(
    clean.plan.map((line) => line.kind),
    [KIND_CODING_SESSION_LIFECYCLE_COMMAND],
  );
  const dirty = codingSessionFoundedReadiness({
    ...READY,
    nameDirty: true,
    promptDirty: true,
  });
  assert.deepEqual(
    dirty.plan.map((line) => line.kind),
    [
      KIND_CODING_SESSION_NAME,
      KIND_CODING_SESSION_GOAL,
      KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    ],
  );
  // Solo never claims a 44245, whatever the hidden draft holds.
  assert.equal(
    codingSessionFoundedReadiness({ ...READY, policySet: true }).plan.some(
      (line) => line.kind === KIND_CODING_SESSION_POLICY,
    ),
    false,
  );
});

test("the Team plan is the governed lines; no agent picked is the lead blocker", () => {
  const unset = codingSessionFoundedReadiness({
    ...READY,
    mode: "team",
    lead: { kind: "unset" },
  });
  assert.deepEqual(
    unset.readiness.blockers.map((entry) => entry.id),
    ["lead"],
  );
  assert.equal(unset.readiness.canLaunch, false);
  const picked = codingSessionFoundedReadiness({
    ...READY,
    mode: "team",
    lead: AGENT,
    leadModel: "opus",
    policySet: true,
    benchCount: 1,
    promptDirty: true,
  });
  assert.deepEqual(picked.readiness.blockers, []);
  assert.deepEqual(
    picked.plan.map((line) => line.id),
    ["goal", "policy", "create", "grants", "turn"],
  );
});

test("the goal and name readers gate Start and the busy sentence reaches readiness", () => {
  const unread = codingSessionFoundedReadiness({
    ...READY,
    goalReader: "unresolved",
  });
  assert.ok(
    unread.readiness.blockers.find((entry) => entry.id === "goal-unresolved"),
  );
  // A typed name over an unread wire name: the plan lists "Publish the
  // name." and Start must not create and then erase it unpublished.
  const unreadName = codingSessionFoundedReadiness({
    ...READY,
    nameResolved: false,
    nameDirty: true,
  });
  assert.ok(
    unreadName.readiness.blockers.find(
      (entry) => entry.id === "name-unresolved",
    ),
  );
  assert.equal(unreadName.readiness.canLaunch, false);
  assert.equal(unreadName.plan[0].id, "name");
  assert.equal(
    codingSessionFoundedReadiness({
      ...READY,
      nameResolved: false,
      nameDirty: false,
    }).readiness.canLaunch,
    true,
  );
  const busy = codingSessionFoundedReadiness({
    ...READY,
    busySentence: "Publishing the initial prompt…",
  });
  assert.deepEqual(
    busy.readiness.blockers.map((entry) => [entry.id, entry.sentence]),
    [["busy", "Publishing the initial prompt…"]],
  );
});

test("project roles gate only when opted in on a project", () => {
  const gated = codingSessionFoundedReadiness({
    ...READY,
    mode: "team",
    lead: AGENT,
    projectRef: "30621:owner:beekeeper",
    useRoles: true,
    readinessGate: { allowed: false, reason: "No checkout." },
  });
  assert.equal(
    gated.readiness.blockers.find((entry) => entry.id === "project-readiness")
      .sentence,
    "No checkout.",
  );
  const optedOut = codingSessionFoundedReadiness({
    ...READY,
    mode: "team",
    lead: AGENT,
    projectRef: "30621:owner:beekeeper",
    useRoles: false,
    readinessGate: { allowed: false, reason: "No checkout." },
    teamReadinessLoading: true,
  });
  assert.equal(
    optedOut.readiness.blockers.some(
      (entry) => entry.id === "project-readiness",
    ),
    false,
  );
  assert.equal(
    optedOut.readiness.unknowns.some(
      (entry) => entry.id === "project-readiness",
    ),
    false,
  );
});

test("switching Team to Solo ignores retained role readiness and missing bench identities", () => {
  const retained = {
    ...READY,
    projectRef: `30621:${"ab".repeat(32)}:tankloop`,
    useRoles: true,
    readinessGate: {
      allowed: false,
      reason: "Project role packs unavailable.",
    },
    teamReadinessError: "Cannot read role packs.",
    unresolvedBenchIdentities: ["cd".repeat(32)],
    benchCount: 1,
    policySet: true,
  };
  const team = codingSessionFoundedReadiness({
    ...retained,
    mode: "team",
    lead: AGENT,
  });
  assert.equal(team.readiness.canLaunch, false);
  assert.deepEqual(
    team.readiness.blockers.map((entry) => entry.id),
    ["project-readiness", `bench:${"cd".repeat(32)}`],
  );
  const solo = codingSessionFoundedReadiness({ ...retained, mode: "solo" });
  assert.equal(solo.readiness.canLaunch, true);
  assert.deepEqual(solo.readiness.blockers, []);
  assert.equal(
    solo.readiness.unknowns.some((entry) => entry.id === "project-readiness"),
    false,
  );
  assert.deepEqual(
    solo.plan.map((entry) => entry.kind),
    [KIND_CODING_SESSION_LIFECYCLE_COMMAND],
  );
  for (const patch of [
    { providerInstanceRef: null },
    { providerRefusal: "Runtime authentication missing." },
    { useWorktree: true, worktreeName: "" },
    { goalReader: "unresolved" },
  ]) {
    assert.equal(
      codingSessionFoundedReadiness({ ...retained, ...patch, mode: "solo" })
        .readiness.canLaunch,
      false,
    );
  }
});
