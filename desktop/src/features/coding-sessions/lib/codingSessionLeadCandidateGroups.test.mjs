import assert from "node:assert/strict";
import test from "node:test";

import {
  applyCodingSessionInstalledRoles,
  codingSessionCandidateOptionLabel,
  codingSessionInstalledLeadDefault,
  groupCodingSessionCandidates,
  resolveCodingSessionLeadActor,
} from "./codingSessionLeadCandidateGroups.ts";

const LOOM = `1c47d440${"0".repeat(56)}`;
const TANK_REVIEWER = `2b${"1".repeat(62)}`;
const KEYSTONE = `3a${"2".repeat(62)}`;
const KEYSTONE_TWO = `4d${"3".repeat(62)}`;
const PROJECT = `30621:${"a".repeat(64)}:tank-loop`;

const CANDIDATES = [
  { pubkey: KEYSTONE, name: "Keystone", role: "lead", model: null },
  { pubkey: KEYSTONE_TWO, name: "Keystone", role: "reviewer", model: null },
  { pubkey: TANK_REVIEWER, name: "Sift", role: "reviewer", model: null },
  { pubkey: LOOM, name: "Loom", role: "lead", model: "claude-opus-5" },
];

const INSTALLED = [
  { role: "lead", agentPubkey: LOOM, packRef: {} },
  { role: "reviewer", agentPubkey: TANK_REVIEWER, packRef: {} },
];

test("the project's installed roles come first, under a heading naming the project", () => {
  const groups = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    installedRoles: INSTALLED,
    projectRef: PROJECT,
    projectName: "Tank Loop",
  });
  assert.deepEqual(
    groups.map((group) => [
      group.id,
      group.heading,
      group.candidates.map((c) => c.pubkey),
    ]),
    [
      ["project", "Tank Loop project roles", [TANK_REVIEWER, LOOM]],
      ["other", "Other agents on this computer", [KEYSTONE, KEYSTONE_TWO]],
    ],
  );
});

test("an option names the agent, its role and its first 8 hex", () => {
  assert.equal(
    codingSessionCandidateOptionLabel(CANDIDATES[3]),
    "Loom · lead · 1c47d440…0000",
  );
  // Two agents called Keystone are told apart by the key, not the name.
  assert.notEqual(
    codingSessionCandidateOptionLabel(CANDIDATES[0]),
    codingSessionCandidateOptionLabel(CANDIDATES[1]),
  );
});

test("no project, or a project that installed nobody here, keeps one ungrouped list", () => {
  for (const input of [
    { installedRoles: INSTALLED, projectRef: null },
    { installedRoles: [], projectRef: PROJECT },
  ]) {
    const groups = groupCodingSessionCandidates({
      candidates: CANDIDATES,
      projectName: "Tank Loop",
      ...input,
    });
    assert.equal(groups.length, 1);
    assert.equal(groups[0].heading, null);
    assert.deepEqual(
      groups[0].candidates.map((c) => c.pubkey),
      CANDIDATES.map((c) => c.pubkey),
    );
  }
});

test("an unnamed project still gets a heading that says whose roles these are", () => {
  const [project] = groupCodingSessionCandidates({
    candidates: CANDIDATES,
    installedRoles: INSTALLED,
    projectRef: PROJECT,
    projectName: null,
  });
  assert.equal(project.heading, "This project's roles");
});

test("the installed role for this project replaces a home role", () => {
  const applied = applyCodingSessionInstalledRoles(
    [{ pubkey: TANK_REVIEWER, name: "Sift", role: "worker", model: null }],
    INSTALLED,
  );
  assert.equal(applied[0].role, "reviewer");
});

test("exactly one installed lead that can be seated is the default", () => {
  assert.equal(
    codingSessionInstalledLeadDefault({
      candidates: CANDIDATES,
      installedRoles: INSTALLED,
    }),
    LOOM,
  );
  // Two installed leads: a choice for the person.
  assert.equal(
    codingSessionInstalledLeadDefault({
      candidates: CANDIDATES,
      installedRoles: [
        ...INSTALLED,
        { role: "lead", agentPubkey: KEYSTONE, packRef: {} },
      ],
    }),
    null,
  );
  // A retried install that recorded the same lead twice is still one lead.
  assert.equal(
    codingSessionInstalledLeadDefault({
      candidates: CANDIDATES,
      installedRoles: [...INSTALLED, INSTALLED[0]],
    }),
    LOOM,
  );
  // Installed, but not an identity on this computer: nothing to seat.
  assert.equal(
    codingSessionInstalledLeadDefault({
      candidates: CANDIDATES.filter((c) => c.pubkey !== LOOM),
      installedRoles: INSTALLED,
    }),
    null,
  );
  assert.equal(
    codingSessionInstalledLeadDefault({
      candidates: CANDIDATES,
      installedRoles: [],
    }),
    null,
  );
});

test("an explicit pick survives a late installed-roles result; no pick follows the default", () => {
  // The picker opens before the journals are read: nothing to default to.
  let selection = { actor: null, explicit: false };
  assert.equal(
    resolveCodingSessionLeadActor({ selection, installedDefault: null }),
    null,
  );
  // The result lands: the single installed lead is preselected.
  assert.equal(
    resolveCodingSessionLeadActor({ selection, installedDefault: LOOM }),
    LOOM,
  );
  // The person picks Keystone before the result lands...
  selection = { actor: KEYSTONE, explicit: true };
  assert.equal(
    resolveCodingSessionLeadActor({ selection, installedDefault: null }),
    KEYSTONE,
  );
  // ...and the late result does not take it back.
  assert.equal(
    resolveCodingSessionLeadActor({ selection, installedDefault: LOOM }),
    KEYSTONE,
  );
  // Choosing "Pick an agent…" on purpose is a pick too.
  assert.equal(
    resolveCodingSessionLeadActor({
      selection: { actor: null, explicit: true },
      installedDefault: LOOM,
    }),
    null,
  );
});
