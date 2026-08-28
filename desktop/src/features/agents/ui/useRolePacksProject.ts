import { useQuery } from "@tanstack/react-query";
import { useLocation } from "@tanstack/react-router";
import * as React from "react";

import {
  resolveRolePacksProject,
  type RolePacksProjectSource,
} from "@/features/agents/lib/rolePacksProject";
import {
  useProjectContainersQuery,
  type ProjectContainer,
} from "@/features/projects-container/hooks";
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
  /** Record the operator's pick; it outranks every fallback below the route. */
  chooseProject: (projectId: string) => void;
};

/**
 * The project the Agents tab's role-pack installer is about.
 *
 * The Agents tab is a Dashboard tab, so the route names no project and the
 * tint's resolution answers `null` — which is what left ledger 85's
 * pre-chosen folder unreachable. This resolves one anyway, from records the
 * app already keeps, and hands back the whole list so the surface can offer
 * the choice rather than making it silently.
 */
export function useRolePacksProject(): RolePacksProjectState {
  const pathname = useLocation({ select: (location) => location.pathname });
  const routeProject = useActiveProjectContainer(pathname, null);
  const projectsEnabled = useFeatureEnabled("projects");
  const projectsQuery = useProjectContainersQuery({ enabled: projectsEnabled });
  const projects = React.useMemo(
    () => (projectsEnabled ? (projectsQuery.data ?? []) : []),
    [projectsEnabled, projectsQuery.data],
  );
  // Only asked for once there is something to order by; on a machine with no
  // projects this surface reads nothing from the workdir store at all.
  const workdirs = useQuery({
    enabled: projects.length > 1,
    queryKey: WORKDIR_STATE_QUERY_KEY,
    queryFn: getCodingSessionWorkdirState,
    staleTime: 60_000,
  });
  const [chosenId, setChosenId] = React.useState<string | null>(null);

  const resolution = React.useMemo(
    () =>
      resolveRolePacksProject({
        routeProject,
        chosenId,
        projects,
        workdirsByProject: workdirs.data?.byProject,
      }),
    [chosenId, projects, routeProject, workdirs.data?.byProject],
  );

  return {
    project: resolution.project,
    projects,
    source: resolution.source,
    chooseProject: setChosenId,
  };
}
