import assert from "node:assert/strict";
import test from "node:test";

import {
  applyCodingSessionInstalledRoles,
  codingSessionCandidateExclusionSentence,
  codingSessionCandidateOptionLabel,
  codingSessionLeadEmptySentence,
  codingSessionProjectLeadDefault,
  groupCodingSessionCandidates,
  partitionCodingSessionCandidates,
  resolveCodingSessionLeadActor,
} from "./codingSessionLeadCandidateGroups.ts";

const OWNER = "a".repeat(64);
const TANK_LOOP = `30621:${OWNER}:tank-loop`;
const BEEKEEPER = `30621:${OWNER}:beekeeper`;

const LOOM = `1c47d440${"0".repeat(56)}`;
const TANK_BUILDER = `2b${"1".repeat(62)}`;
const BOB = `3a${"2".repeat(62)}`;
const BEE_LEAD = `4d${"3".repeat(62)}`;
const STRAY = `5e${"4".repeat(62)}`;
const ROLELESS = `6f${"5".repeat(62)}`;

/**
 * Two projects whose agents share role names, plus an agent in no project and
 * one with no role at all.
 */
const CANDIDATES = [
  { pubkey: BOB, name: "Bob", role: "builder", projectRef: BEEKEEPER },
  { pubkey: BEE_LEAD, name: "Keystone", role: "lead", projectRef: BEEKEEPER },
  {
    pubkey: TANK_BUILDER,
    name: "Builder",
    role: "builder",
    projectRef: TANK_LOOP,
  },
  { pubkey: LOOM, name: "Loom", role: "lead", projectRef: TANK_LOOP },
  { pubkey: STRAY, name: "Stray", role: "lead", projectRef: null },
  { pubkey: ROLELESS, name: "Plain", role: null, projectRef: null },
];

const pubkeys = (groups) =>
  groups.flatMap((group) => group.candidates.map((c) => c.pubkey));

test("two projects with the same role names: each picker lists only its own agents", () => {
  const tank = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: TANK_LOOP,
    projectName: "Tank Loop",
  });
  assert.deepEqual(
    tank.map((group) => [group.id, group.heading]),
    [["project", "Tank Loop agents"]],
  );
  assert.deepEqual(pubkeys(tank), [TANK_BUILDER, LOOM]);

  const bee = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: BEEKEEPER,
    projectName: "Beekeeper",
  });
  assert.deepEqual(pubkeys(bee), [BOB, BEE_LEAD]);
});

test("a project ref differing only in owner-hex case is the same project", () => {
  const upper = `30621:${OWNER.toUpperCase()}:tank-loop`;
  assert.deepEqual(
    pubkeys(
      groupCodingSessionCandidates({
        candidates: CANDIDATES,
        projectRef: upper,
        projectName: null,
      }),
    ),
    [TANK_BUILDER, LOOM],
  );
});

test("renaming an agent changes its label, never its eligibility", () => {
  const renamed = CANDIDATES.map((c) =>
    c.pubkey === BOB ? { ...c, name: "Builder" } : c,
  );
  for (const projectRef of [TANK_LOOP, BEEKEEPER, null]) {
    assert.deepEqual(
      pubkeys(
        groupCodingSessionCandidates({
          candidates: renamed,
          projectRef,
          projectName: null,
        }),
      ),
      pubkeys(
        groupCodingSessionCandidates({
          candidates: CANDIDATES,
          projectRef,
          projectName: null,
        }),
      ),
    );
  }
});

test("an unassociated agent is not an option for a project, and is counted", () => {
  const { eligible, excluded } = partitionCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: TANK_LOOP,
  });
  assert.equal(
    eligible.some((c) => c.pubkey === STRAY),
    false,
  );
  // Bob, Keystone (another project) and Stray (no project); the roleless
  // agent could lead nowhere, so it is not "excluded here".
  assert.deepEqual(
    excluded.map((c) => c.pubkey),
    [BOB, BEE_LEAD, STRAY],
  );
  assert.equal(
    codingSessionCandidateExclusionSentence({
      excludedCount: excluded.length,
      projectRef: TANK_LOOP,
      projectName: "Tank Loop",
      surface: "lead",
    }),
    "3 agents on this computer aren't Tank Loop agents, so they can't lead here. Associate one on the project's Agents tab.",
  );
  assert.equal(
    codingSessionCandidateExclusionSentence({
      excludedCount: 1,
      projectRef: TANK_LOOP,
      projectName: "Tank Loop",
      surface: "bench",
    }),
    "1 agent on this computer isn't a Tank Loop agent, so it can't be benched here. Associate one on the project's Agents tab.",
  );
  assert.equal(
    codingSessionCandidateExclusionSentence({
      excludedCount: 0,
      projectRef: TANK_LOOP,
      projectName: "Tank Loop",
      surface: "lead",
    }),
    null,
  );
});

test("a projectless session lists unassociated agents only, flat", () => {
  const groups = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: null,
    projectName: null,
  });
  assert.equal(groups.length, 1);
  assert.equal(groups[0].heading, null);
  assert.deepEqual(pubkeys(groups), [STRAY]);
  const { excluded } = partitionCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: null,
  });
  assert.equal(
    codingSessionCandidateExclusionSentence({
      excludedCount: excluded.length,
      projectRef: null,
      projectName: null,
      surface: "lead",
    }),
    "4 agents on this computer belong to a project, so they can't lead in a session outside one.",
  );
});

test("a project with no agent of its own says so; Solo is the way out", () => {
  const sentence = codingSessionLeadEmptySentence({
    eligibleCount: 0,
    projectRef: `30621:${OWNER}:empty`,
    projectName: "Empty",
  });
  assert.match(sentence, /No Empty agent with a role is on this computer/);
  assert.match(sentence, /Agents tab/);
  assert.match(sentence, /Solo/);
  assert.equal(
    codingSessionLeadEmptySentence({
      eligibleCount: 2,
      projectRef: TANK_LOOP,
      projectName: "Tank Loop",
    }),
    null,
  );
});

test("an option names the agent, its role and its first 8 hex", () => {
  assert.equal(
    codingSessionCandidateOptionLabel(CANDIDATES[3]),
    "Loom · lead · 1c47d440…0000",
  );
  // Two agents called Builder are told apart by the key, not the name.
  assert.notEqual(
    codingSessionCandidateOptionLabel({ ...CANDIDATES[0], name: "Builder" }),
    codingSessionCandidateOptionLabel(CANDIDATES[2]),
  );
});

test("an unnamed project still gets a heading that says whose agents these are", () => {
  const [project] = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: TANK_LOOP,
    projectName: null,
  });
  assert.equal(project.heading, "This project's agents");
});

test("the installed role for this project replaces a home role in the label", () => {
  const applied = applyCodingSessionInstalledRoles(
    [{ pubkey: TANK_BUILDER, name: "Builder", role: "worker" }],
    [{ role: "builder", agentPubkey: TANK_BUILDER, packRef: {} }],
  );
  assert.equal(applied[0].role, "builder");
});

test("the default lead is the project's single agent whose role is lead", () => {
  assert.equal(
    codingSessionProjectLeadDefault({
      candidates: CANDIDATES,
      projectRef: TANK_LOOP,
    }),
    LOOM,
  );
  assert.equal(
    codingSessionProjectLeadDefault({
      candidates: CANDIDATES,
      projectRef: BEEKEEPER,
    }),
    BEE_LEAD,
  );
  // Another project's lead, or an unassociated one, is never the default.
  assert.equal(
    codingSessionProjectLeadDefault({
      candidates: CANDIDATES.filter((c) => c.pubkey !== LOOM),
      projectRef: TANK_LOOP,
    }),
    null,
  );
  // Two leads in the project: a choice for the person.
  assert.equal(
    codingSessionProjectLeadDefault({
      candidates: [
        ...CANDIDATES,
        {
          pubkey: `7a${"6".repeat(62)}`,
          name: "Loom Two",
          role: "lead",
          projectRef: TANK_LOOP,
        },
      ],
      projectRef: TANK_LOOP,
    }),
    null,
  );
  // A projectless session has no default, even with one unassociated lead.
  assert.equal(
    codingSessionProjectLeadDefault({
      candidates: CANDIDATES,
      projectRef: null,
    }),
    null,
  );
});

test("an explicit pick survives a late default; no pick follows the default", () => {
  const eligible = partitionCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: TANK_LOOP,
  }).eligible;
  let selection = { actor: null, explicit: false };
  assert.equal(
    resolveCodingSessionLeadActor({ selection, defaultActor: null, eligible }),
    null,
  );
  assert.equal(
    resolveCodingSessionLeadActor({ selection, defaultActor: LOOM, eligible }),
    LOOM,
  );
  selection = { actor: TANK_BUILDER, explicit: true };
  assert.equal(
    resolveCodingSessionLeadActor({ selection, defaultActor: LOOM, eligible }),
    TANK_BUILDER,
  );
  // Choosing "Pick an agent…" on purpose is a pick too.
  assert.equal(
    resolveCodingSessionLeadActor({
      selection: { actor: null, explicit: true },
      defaultActor: LOOM,
      eligible,
    }),
    null,
  );
});

test("an explicit pick that is not eligible is refused, not seated and not defaulted", () => {
  // Picked while the channel's project was still unread (projectless list),
  // then the project resolved: Stray belongs to no project and cannot lead.
  const projectless = partitionCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: null,
  }).eligible;
  const selection = { actor: STRAY, explicit: true };
  assert.equal(
    resolveCodingSessionLeadActor({
      selection,
      defaultActor: null,
      eligible: projectless,
    }),
    STRAY,
  );
  const tank = partitionCodingSessionCandidates({
    candidates: CANDIDATES,
    projectRef: TANK_LOOP,
  }).eligible;
  assert.equal(
    resolveCodingSessionLeadActor({
      selection,
      defaultActor: LOOM,
      eligible: tank,
    }),
    null,
  );
  // Another project's agent picked by hand (a stale DOM value) is refused too.
  assert.equal(
    resolveCodingSessionLeadActor({
      selection: { actor: BOB, explicit: true },
      defaultActor: LOOM,
      eligible: tank,
    }),
    null,
  );
});
