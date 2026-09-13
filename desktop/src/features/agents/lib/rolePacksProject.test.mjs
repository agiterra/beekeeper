/**
 * Which project the team-role installer is about, on a surface whose route
 * names none (ledger 85's reachability finding).
 *
 * The Agents tab is a Dashboard tab: `/?tab=agents` names no project and has
 * no active channel, so the route-based resolution the tint uses answers
 * `null` there every time. That left the pre-chosen folder unreachable. These
 * pin the fallback order, and pin that the answer is never a silent guess —
 * the caller always gets a whole project back, so the surface can name it.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  resolveRolePacksProject,
  ROLE_PACKS_PROJECT_SOURCES,
} from "./rolePacksProject.ts";

const OWNER = "a".repeat(64);

function makeProject(dtag, createdAt = 1) {
  return {
    id: `${OWNER}:${dtag}`,
    dtag,
    owner: OWNER,
    name: dtag,
    description: "",
    createdAt,
    address: `30621:${OWNER}:${dtag}`,
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
    visibility: "public",
    members: [],
    icon: null,
    color: null,
  };
}

const GENERAL = makeProject("general", 1);
const ATTIC = makeProject("attic", 2);

test("no project at all resolves to nothing — the installer stays as it was", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [],
    workdirsByProject: {},
  });
  assert.equal(resolved.project, null);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.none);
});

test("one project and no route: that project, and the surface can name it", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [GENERAL],
    workdirsByProject: {},
  });
  assert.equal(resolved.project?.id, GENERAL.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.onlyMembership);
});

test("a route that names a project outranks every fallback", () => {
  const resolved = resolveRolePacksProject({
    routeProject: ATTIC,
    chosenId: GENERAL.id,
    projects: [GENERAL, ATTIC],
    workdirsByProject: { [GENERAL.address]: { updatedAt: "2026-08-28" } },
  });
  assert.equal(resolved.project?.id, ATTIC.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.route);
});

test("the operator's own choice outranks recency and order", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: ATTIC.id,
    projects: [GENERAL, ATTIC],
    workdirsByProject: { [GENERAL.address]: { updatedAt: "2026-08-28" } },
  });
  assert.equal(resolved.project?.id, ATTIC.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.chosen);
});

test("a choice naming a project that is gone falls through rather than blanking", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: `${OWNER}:deleted`,
    projects: [GENERAL, ATTIC],
    workdirsByProject: {},
  });
  assert.equal(resolved.project?.id, GENERAL.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.firstMembership);
});

test("with no choice, the project this computer last recorded a checkout for wins", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [GENERAL, ATTIC],
    workdirsByProject: {
      [GENERAL.address]: { updatedAt: "2026-08-01T00:00:00Z" },
      [ATTIC.address]: { updatedAt: "2026-08-27T00:00:00Z" },
    },
  });
  assert.equal(
    resolved.project?.id,
    ATTIC.id,
    "the newest recorded checkout, not the first project in the list",
  );
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.recentCheckout);
});

test("a checkout recorded for a project the viewer cannot see is not resolved", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [GENERAL],
    workdirsByProject: {
      [ATTIC.address]: { updatedAt: "2026-08-27T00:00:00Z" },
    },
  });
  assert.equal(resolved.project?.id, GENERAL.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.onlyMembership);
});

test("with nothing recorded, the first project in the list — never an arbitrary one", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [GENERAL, ATTIC],
    workdirsByProject: undefined,
  });
  assert.equal(resolved.project?.id, GENERAL.id);
  assert.equal(resolved.source, ROLE_PACKS_PROJECT_SOURCES.firstMembership);
});

// ── One selected project (Tank Loop walkthrough §5) ─────────────────────────
//
// The Agents tab has one selection: the directory's project filter. The
// installer resolves from that same value over the same project list, and a
// fallback is reported as one so the surface can say so.

import {
  isRolePacksProjectFallback,
  rolePacksProjectCandidates,
} from "./rolePacksProject.ts";
import { rolePacksProjectFallbackNote } from "../ui/installCrewRolesCopy.ts";

const LOCAL_GENERAL = {
  ...makeProject("general", 0),
  id: "local:general",
  owner: "",
};

test("the directory's selected project is the installer's project", () => {
  const projects = rolePacksProjectCandidates([LOCAL_GENERAL, GENERAL, ATTIC]);
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: ATTIC.id,
    projects,
    workdirsByProject: { [GENERAL.address]: { updatedAt: "2026-09-01" } },
  });
  assert.equal(resolved.project?.id, ATTIC.id);
  assert.equal(isRolePacksProjectFallback(resolved.source), false);
  assert.equal(rolePacksProjectFallbackNote(resolved.source, true), null);
});

test("the local General placeholder is not offered for role packs; the rest keep list order", () => {
  assert.deepEqual(
    rolePacksProjectCandidates([LOCAL_GENERAL, ATTIC, GENERAL]).map(
      (project) => project.id,
    ),
    [ATTIC.id, GENERAL.id],
  );
});

test("Any project in the directory: the installer falls back and the note says so", () => {
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: null,
    projects: [GENERAL, ATTIC],
    workdirsByProject: {
      [ATTIC.address]: { updatedAt: "2026-08-27T00:00:00Z" },
    },
  });
  assert.equal(isRolePacksProjectFallback(resolved.source), true);
  assert.equal(
    rolePacksProjectFallbackNote(resolved.source, false),
    "No project is chosen in the project filter, so this fell back to the project with this computer's newest checkout.",
  );
  assert.equal(
    rolePacksProjectFallbackNote(
      ROLE_PACKS_PROJECT_SOURCES.firstMembership,
      false,
    ),
    "No project is chosen in the project filter, so this fell back to the first project in the list.",
  );
});

test("a directory project that cannot hold role packs falls back, and says it was chosen but unusable", () => {
  const projects = rolePacksProjectCandidates([LOCAL_GENERAL, GENERAL, ATTIC]);
  const resolved = resolveRolePacksProject({
    routeProject: null,
    chosenId: LOCAL_GENERAL.id,
    projects,
    workdirsByProject: undefined,
  });
  assert.equal(resolved.project?.id, GENERAL.id);
  assert.equal(
    rolePacksProjectFallbackNote(resolved.source, true),
    "The project chosen in the project filter cannot hold role packs, so this fell back to the first project in the list.",
  );
});

test("route and chosen sources disclose nothing", () => {
  for (const source of [
    ROLE_PACKS_PROJECT_SOURCES.route,
    ROLE_PACKS_PROJECT_SOURCES.chosen,
    ROLE_PACKS_PROJECT_SOURCES.none,
  ]) {
    assert.equal(isRolePacksProjectFallback(source), false);
    assert.equal(rolePacksProjectFallbackNote(source, false), null);
  }
});
