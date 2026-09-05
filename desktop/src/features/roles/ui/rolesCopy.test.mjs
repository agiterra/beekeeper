import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_NO_ROLE_PACK_REMEDY,
  AGENT_SHARED_HOME_LABEL,
  AGENT_SHARED_HOME_REMEDY,
} from "@/features/agents/ui/AgentHomeRoleBadges";
import {
  AGE_UNKNOWN,
  AGENT_CHIP_NO_PACK_TITLE,
  AGENT_CHIP_SHARED_HOME_TITLE,
  formatAge,
  INSTALL_ROLES_BUTTON_LABEL,
  packsSourceSentence,
  PROJECT_PACKS_MISSING,
  PROJECT_SEATS_EMPTY,
  projectSeatCount,
  ROLE_AGENTS_EMPTY,
  ROLE_NO_PACK,
  ROLE_SEATS_EMPTY,
  ROLE_SEATS_TITLE,
  ROLE_SKILLS_EMPTY,
  ROLES_EMPTY,
  ROLES_TITLE,
  rolesErrorSentence,
  SEAT_NO_AGENT,
  SEAT_UNMANAGED_AGENT,
  seatStatusText,
  SHA_UNKNOWN,
  shaText,
  SHARED_SKILL_MARK,
  UNPLACED_TITLE,
} from "@/features/roles/ui/rolesCopy";

test("the empty states are the brief's exact words", () => {
  assert.equal(ROLES_TITLE, "Roles");
  assert.equal(ROLE_SKILLS_EMPTY, "no skills");
  assert.equal(ROLE_AGENTS_EMPTY, "no agents carry this role");
  // Fix 1: a seat is authorization and assignment, not proof of work — the
  // heading and its empty state stop claiming liveness.
  assert.equal(ROLE_SEATS_TITLE, "Seats");
  assert.equal(ROLE_SEATS_EMPTY, "no open seats");
  assert.equal(PROJECT_SEATS_EMPTY, "no agents seated");
  assert.equal(UNPLACED_TITLE, "Unplaced");
  assert.equal(SHA_UNKNOWN, "sha unknown");
  assert.equal(SHARED_SKILL_MARK, "(shared)");
  assert.equal(
    ROLES_EMPTY,
    "No roles resolve for this project, and no seat or agent names one.",
  );
  assert.equal(PROJECT_PACKS_MISSING, "This project is not readable here.");
  assert.equal(INSTALL_ROLES_BUTTON_LABEL, "Install roles");
});

test("the agent chip tooltips are the Agents tab's own badge copy, unchanged", () => {
  assert.equal(
    AGENT_CHIP_NO_PACK_TITLE,
    `${AGENT_NO_ROLE_PACK_LABEL} — ${AGENT_NO_ROLE_PACK_REMEDY}`,
  );
  assert.equal(
    AGENT_CHIP_SHARED_HOME_TITLE,
    `${AGENT_SHARED_HOME_LABEL} — ${AGENT_SHARED_HOME_REMEDY}`,
  );
  assert.match(AGENT_CHIP_NO_PACK_TITLE, /^Role pack not installed here — /);
});

test("a role without a pack says why it is listed at all", () => {
  assert.match(ROLE_NO_PACK, /^No pack for this role/);
  assert.match(ROLE_NO_PACK, /a seat or an agent names it/);
  assert.equal(SEAT_NO_AGENT, "no agent seated");
  assert.equal(SEAT_UNMANAGED_AGENT, "unmanaged agent");
});

test("formatAge is compact and discloses an unknown age", () => {
  assert.equal(formatAge(null), AGE_UNKNOWN);
  assert.equal(formatAge(0), "just now");
  assert.equal(formatAge(59), "just now");
  assert.equal(formatAge(60), "1m");
  assert.equal(formatAge(3_599), "59m");
  assert.equal(formatAge(3_600), "1h");
  assert.equal(formatAge(86_399), "23h");
  assert.equal(formatAge(86_400), "1d");
  assert.equal(formatAge(3 * 86_400 + 5), "3d");
});

test("seat status is the catalog word verbatim, then the age", () => {
  assert.equal(seatStatusText("running", 300), "running (5m)");
  assert.equal(
    seatStatusText("waiting_for_input", null),
    "waiting_for_input (age unknown)",
  );
});

test("shaText shortens a commit the way the seat pack line does, and never invents one", () => {
  assert.equal(shaText("a".repeat(40)), "aaaaaaaa");
  assert.equal(shaText("0.4.2-block"), "0.4.2-block");
  assert.equal(shaText(null), "sha unknown");
});

test("projectSeatCount pluralizes", () => {
  assert.equal(projectSeatCount(0), "0 agents");
  assert.equal(projectSeatCount(1), "1 agent");
  assert.equal(projectSeatCount(2), "2 agents");
});

test("rolesErrorSentence attributes the backend's words", () => {
  assert.equal(
    rolesErrorSentence("no checkout recorded"),
    "Role packs could not be read: no checkout recorded",
  );
});

test("packsSourceSentence follows the brief's shape and refuses to average (Fix 3)", () => {
  const sha = "c".repeat(40);
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 2, {
      origins: ["project"],
      location: "30617:owner:packs",
      sha,
      shasDiffer: false,
    }),
    {
      text: `2 role packs for Beekeeper, from the project rung at 30617:owner:packs, pinned to ${sha.slice(0, 8)}.`,
      shaTitle: sha,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 1, {
      origins: ["shipped"],
      location: "/app/shipped",
      sha: null,
      shasDiffer: false,
    }),
    {
      text: "1 role pack for Beekeeper, from the shipped rung at /app/shipped, with no sha recorded.",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 2, {
      origins: ["project", "shipped"],
      location: null,
      sha: null,
      shasDiffer: true,
    }),
    {
      text: "2 role packs for Beekeeper, from the project and shipped rungs at several locations, pinned to different shas by role.",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 3, {
      origins: ["project", "shipped", "user"],
      location: "/packs",
      sha: null,
      shasDiffer: false,
    }),
    {
      text: "3 role packs for Beekeeper, from the project, shipped and user rungs at /packs, with no sha recorded.",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 0, {
      origins: [],
      location: null,
      sha: null,
      shasDiffer: false,
    }),
    { text: "No role packs resolve for Beekeeper.", shaTitle: null },
  );
  // A non-hex "sha" (a shipped pack's version string) is shown verbatim in
  // the text but never offered as a tooltip's full sha.
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 1, {
      origins: ["shipped"],
      location: "/app/shipped",
      sha: "0.4.2-block",
      shasDiffer: false,
    }),
    {
      text: "1 role pack for Beekeeper, from the shipped rung at /app/shipped, pinned to 0.4.2-block.",
      shaTitle: null,
    },
  );
});
