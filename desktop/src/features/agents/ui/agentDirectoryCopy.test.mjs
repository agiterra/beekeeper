/**
 * The Agents directory's own sentences, asserted as values.
 *
 * The Add-agent affordance is here because replacing the old agents section
 * with the directory took its "Add" button with it, and a screen that lists
 * agents but offers no way to make one is a dead end. The button opens the
 * dialog that already existed; only the entry point is new.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AGENT_DIRECTORY_ADD_LABEL,
  AGENT_DIRECTORY_ADD_TESTID,
  AGENT_DIRECTORY_EMPTY,
  AGENT_FILTERS_TESTID,
  OFFERS_ELIGIBILITY_FOOTNOTE,
  currentSeatText,
  launchesAsText,
} from "@/features/agents/ui/agentDirectoryCopy";

test("the directory's creation affordance is one button, named plainly", () => {
  assert.equal(AGENT_DIRECTORY_ADD_LABEL, "Add agent");
  assert.equal(AGENT_DIRECTORY_ADD_TESTID, "agent-directory-add");
});

test("the add label claims nothing about membership", () => {
  for (const word of ["Eligible", "Offered", "team", "crew", "available"]) {
    assert.equal(AGENT_DIRECTORY_ADD_LABEL.includes(word), false);
  }
});

test("the empty state still has a way out of itself", () => {
  // The toolbar is not rendered with zero rows, so the empty sentence and the
  // Add button are shown together; asserting the copy here keeps the pair
  // named in one place.
  assert.equal(AGENT_DIRECTORY_EMPTY, "No agents on this computer yet.");
  assert.equal(AGENT_FILTERS_TESTID, "agent-filters");
});

test("the surface's two evidenced facts are unchanged by this addition", () => {
  assert.equal(
    OFFERS_ELIGIBILITY_FOOTNOTE,
    "Offers and eligibility are not recorded yet.",
  );
  assert.equal(currentSeatText(null), "not seated");
  assert.equal(launchesAsText(null), "launches as — not set");
});

// ── Project roles, installed agents, session participation ─────────────────

import {
  AGENT_FILTER_PROJECT_GROUP_LABEL,
  AGENT_FILTER_STATUS_PROJECT_AGENT,
  AGENT_FILTER_STATUS_RUNNING,
  AGENTS_PROJECT_ROLES_DESCRIPTION,
  SAVED_AGENT_GROUPS_DESCRIPTION,
  SAVED_AGENT_GROUPS_TITLE,
  agentProjectText,
  renameFailedText,
  unassociatedInstallationText,
} from "@/features/agents/ui/agentDirectoryCopy";
import { INSTALL_CREW_ROLES_BODY } from "@/features/agents/ui/installCrewRolesCopy";

test("a row names its project by association, and a seat reads as seated-in", () => {
  assert.equal(
    agentProjectText({ projectName: "Tank Loop" }, "builder", true),
    "Tank Loop · builder",
  );
  assert.equal(
    agentProjectText({ projectName: null }, "builder", true),
    "A project not listed here · builder",
  );
  assert.equal(agentProjectText(null, "builder", true), "No project");
  assert.equal(
    agentProjectText(null, null, false),
    "Project not known on this computer",
  );
  assert.equal(
    currentSeatText({
      projectName: "Tank Loop",
      status: "running",
      ageSeconds: 300,
    }),
    "seated in Tank Loop · running · 5m",
  );
});

test("an installation without association is a warning, never membership", () => {
  assert.equal(
    unassociatedInstallationText([{ projectName: "Tank Loop", role: "lead" }]),
    "Installed for Tank Loop but not associated yet — the lead can't hire it.",
  );
  assert.equal(
    unassociatedInstallationText([
      { projectName: "Tank Loop", role: "lead" },
      { projectName: null, role: "builder" },
    ]),
    "Installed for Tank Loop (+1 more) but not associated yet — the lead can't hire it.",
  );
  assert.equal(unassociatedInstallationText([]), null);
});

test("the project filter says seats do not count, and Running keeps its word", () => {
  assert.equal(
    AGENT_FILTER_PROJECT_GROUP_LABEL,
    "agents associated with it; seats do not count",
  );
  assert.equal(AGENT_FILTER_STATUS_PROJECT_AGENT, "Belongs to a project");
  assert.equal(AGENT_FILTER_STATUS_RUNNING, "Running");
});

test("saved groups are described as channel conveniences, not a project's roles", () => {
  assert.equal(SAVED_AGENT_GROUPS_TITLE, "Saved agent groups");
  assert.match(
    SAVED_AGENT_GROUPS_DESCRIPTION,
    /add several agents to a channel at once/,
  );
  assert.match(SAVED_AGENT_GROUPS_DESCRIPTION, /not a project's roles/);
  assert.match(AGENTS_PROJECT_ROLES_DESCRIPTION, /shared instructions/);
  assert.match(AGENTS_PROJECT_ROLES_DESCRIPTION, /agents on this computer/);
});

test("the installer no longer promises one team you can launch", () => {
  assert.equal(
    INSTALL_CREW_ROLES_BODY.includes("one team you can launch"),
    false,
  );
  assert.match(INSTALL_CREW_ROLES_BODY, /shared instructions/);
  assert.match(INSTALL_CREW_ROLES_BODY, /one agent on this computer/);
  assert.match(INSTALL_CREW_ROLES_BODY, /Installing starts nothing/);
});

test("a failed rename is said plainly with its cause", () => {
  assert.equal(
    renameFailedText("keychain locked"),
    "The name was not changed: keychain locked",
  );
});
