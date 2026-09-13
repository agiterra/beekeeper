import { useQuery } from "@tanstack/react-query";
import { useLocation } from "@tanstack/react-router";
import * as React from "react";

import {
  resolveRolePacksProject,
  rolePacksProjectCandidates,
  type RolePacksProjectSource,
} from "@/features/agents/lib/rolePacksProject";
import type { ProjectContainer } from "@/features/projects-container/hooks";
import { useActiveProjectContainer } from "@/features/projects-container/useActiveProjectTint";
import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
import { useFeatureEnabled } from "@/shared/features";

/** Shared with `CodingSessionHireHost` so both read one cached answer. */
const WORKDIR_STATE_QUERY_KEY = ["coding-session-workdir-state"] as const;

export type RolePacksProjectState = {
  /** The project the installer will read packs from, or `null` for none. */
  project: ProjectContainer | null;
  /** Every project the viewer can choose between, in the app's own order. */
  projects: readonly ProjectContainer[];
  /** Why `project` is the one it is. */
  source: RolePacksProjectSource;
};

export type UseRolePacksProjectInput = {
  /**
   * The Agents tab's one project list — the list its directory filter shows.
   * The selector offers the same projects, minus the local General
   * placeholder (see `rolePacksProjectCandidates`).
   */
  projects: readonly ProjectContainer[];
  /**
   * The directory's selected project id, or `null` for "Any project". There
   * is no second selection: the installer reads the directory's.
   */
  selectedProjectId: string | null;
};

/**
 * The project the Agents tab's role-pack installer is about.
 *
 * The Agents tab is a Dashboard tab, so the route names no project and the
 * tint's resolution answers `null` — which is what left ledger 85's
 * pre-chosen folder unreachable. The directory's project filter is the
 * selection; when it is on "Any project" this resolves one from records the
 * app already keeps and reports `source` so the surface can say it fell back.
 */
export function useRolePacksProject(
  input: UseRolePacksProjectInput,
): RolePacksProjectState {
  const pathname = useLocation({ select: (location) => location.pathname });
  const routeProject = useActiveProjectContainer(pathname, null);
  const projectsEnabled = useFeatureEnabled("projects");
  const projects = React.useMemo(
    () => (projectsEnabled ? rolePacksProjectCandidates(input.projects) : []),
    [projectsEnabled, input.projects],
  );
  // Only asked for once there is something to order by; on a machine with no
  // projects this surface reads nothing from the workdir store at all.
  const workdirs = useQuery({
    enabled: projects.length > 1,
    queryKey: WORKDIR_STATE_QUERY_KEY,
    queryFn: getCodingSessionWorkdirState,
    staleTime: 60_000,
  });

  const resolution = React.useMemo(
    () =>
      resolveRolePacksProject({
        routeProject,
        chosenId: input.selectedProjectId,
        projects,
        workdirsByProject: workdirs.data?.byProject,
      }),
    [input.selectedProjectId, projects, routeProject, workdirs.data?.byProject],
  );

  return {
    project: resolution.project,
    projects,
    source: resolution.source,
  };
}
