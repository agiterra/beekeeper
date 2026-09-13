import type { ProjectContainer } from "@/features/projects-container/hooks";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

/**
 * Which project the team-role installer reads packs from, on a surface whose
 * route names none.
 *
 * Ledger 85 pre-chose `<checkout>/personas/roles` for the installer, then
 * found the pre-chosen folder unreachable: the only entry point is the Agents
 * tab of the Dashboard (`/?tab=agents`), and the route-based resolution the
 * project tint uses (`resolveActiveProjectId`) answers `null` there — no
 * `/projects/<id>` segment, no active channel to carry a `projectRef`. So the
 * installer needs a *fallback*, and a fallback is where a surface starts
 * lying: pick a project silently and the operator installs another project's
 * packs believing they installed this one's.
 *
 * The rule here is therefore two-part. Pick by a recorded fact, in a fixed
 * order, and hand the caller the whole project plus *why* it was picked, so
 * the surface can name it. Nothing in this module decides to stay quiet.
 */

/** Why `resolveRolePacksProject` answered the way it did. */
export const ROLE_PACKS_PROJECT_SOURCES = {
  /** The route named it — a project surface, not the Dashboard. */
  route: "route",
  /** The operator picked it from the selector. */
  chosen: "chosen",
  /** The project this computer most recently recorded a checkout for. */
  recentCheckout: "recent-checkout",
  /** The only project there is. */
  onlyMembership: "only-membership",
  /** First in the app's own project order, nothing else to go on. */
  firstMembership: "first-membership",
  /** There is no project at all. */
  none: "none",
} as const;

export type RolePacksProjectSource =
  (typeof ROLE_PACKS_PROJECT_SOURCES)[keyof typeof ROLE_PACKS_PROJECT_SOURCES];

export type RolePacksProjectResolution = {
  /** The project whose packs the installer will read, or `null` for none. */
  project: ProjectContainer | null;
  source: RolePacksProjectSource;
};

export type ResolveRolePacksProjectInput = {
  /** The project the route names, when it names one. */
  routeProject: ProjectContainer | null;
  /** The project id the operator picked on this surface, if any. */
  chosenId: string | null;
  /** Every project this viewer can see, in the app's own order. */
  projects: readonly ProjectContainer[];
  /**
   * `CodingSessionWorkdirState.byProject` — the host-local record of which
   * checkout directory belongs to which project coordinate, and when it was
   * last written. This is the only durable per-project recency the desktop
   * keeps; there is no "last opened project" anywhere in the app. It is also
   * the right one for this job: a project with no checkout has no
   * `personas/roles` folder to open on.
   */
  workdirsByProject: Record<string, { updatedAt: string }> | undefined;
};

/**
 * Resolve the installer's project, newest recorded checkout first.
 *
 * Order: the route, then the operator's own pick, then the project whose
 * checkout directory this computer wrote most recently, then the app's first
 * project. A `chosenId` naming a project that is gone falls through rather
 * than resolving to nothing — a stale pick must not blank a surface that has
 * a perfectly good answer.
 */
export function resolveRolePacksProject(
  input: ResolveRolePacksProjectInput,
): RolePacksProjectResolution {
  const { routeProject, chosenId, projects, workdirsByProject } = input;
  if (routeProject) {
    return { project: routeProject, source: ROLE_PACKS_PROJECT_SOURCES.route };
  }
  if (projects.length === 0) {
    return { project: null, source: ROLE_PACKS_PROJECT_SOURCES.none };
  }
  if (chosenId) {
    const chosen = projects.find((candidate) => candidate.id === chosenId);
    if (chosen) {
      return { project: chosen, source: ROLE_PACKS_PROJECT_SOURCES.chosen };
    }
  }
  if (projects.length === 1) {
    const only = projects[0];
    return only
      ? { project: only, source: ROLE_PACKS_PROJECT_SOURCES.onlyMembership }
      : { project: null, source: ROLE_PACKS_PROJECT_SOURCES.none };
  }
  // ISO-8601 from `now_iso()` in workdir_store.rs, so string order is time
  // order. A checkout recorded for a project this viewer cannot see is
  // ignored — the answer has to be a project the surface can name.
  let recent: { project: ProjectContainer; updatedAt: string } | null = null;
  for (const project of projects) {
    const updatedAt = workdirsByProject?.[project.address]?.updatedAt;
    if (typeof updatedAt !== "string" || updatedAt.length === 0) continue;
    if (!recent || updatedAt > recent.updatedAt) {
      recent = { project, updatedAt };
    }
  }
  if (recent) {
    return {
      project: recent.project,
      source: ROLE_PACKS_PROJECT_SOURCES.recentCheckout,
    };
  }
  const first = projects[0];
  return first
    ? { project: first, source: ROLE_PACKS_PROJECT_SOURCES.firstMembership }
    : { project: null, source: ROLE_PACKS_PROJECT_SOURCES.none };
}

/**
 * The projects the role-pack selector offers, drawn from the Agents tab's one
 * project list (the same list the directory's project filter shows).
 *
 * The local General placeholder is left out: it is displayed until a real
 * `general` project is published, has no owner and no published address, and
 * so cannot have role packs or a recorded checkout. Every other project is
 * offered in the list's own order.
 */
export function rolePacksProjectCandidates(
  projects: readonly ProjectContainer[],
): ProjectContainer[] {
  return projects.filter((project) => project.id !== LOCAL_GENERAL_ID);
}

/** True when `source` means nobody chose the project — it was resolved. */
export function isRolePacksProjectFallback(
  source: RolePacksProjectSource,
): boolean {
  return (
    source === ROLE_PACKS_PROJECT_SOURCES.recentCheckout ||
    source === ROLE_PACKS_PROJECT_SOURCES.onlyMembership ||
    source === ROLE_PACKS_PROJECT_SOURCES.firstMembership
  );
}
