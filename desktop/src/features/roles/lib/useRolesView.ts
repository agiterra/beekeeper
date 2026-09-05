import { useQuery } from "@tanstack/react-query";
import { useLocation } from "@tanstack/react-router";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import {
  resolveRolePacksProject,
  type RolePacksProjectSource,
} from "@/features/agents/lib/rolePacksProject";
import { useChannelsQuery } from "@/features/channels/hooks";
import {
  type ProjectContainer,
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfState } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { useActiveProjectContainer } from "@/features/projects-container/useActiveProjectTint";
import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
import { listProjectRolePacks } from "@/shared/api/tauriRolePacks";
import type { RolePackSummary } from "@/shared/api/types";

import {
  buildRolesView,
  describePacksSource,
  type PacksSourceSummary,
  type RolesView,
} from "./rolesViewModel";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();

/**
 * The same key `useRolePacksProject` (Agents tab) reads the workdir store
 * under, so the two tabs share one cached answer instead of two reads.
 */
const WORKDIR_STATE_QUERY_KEY = ["coding-session-workdir-state"] as const;

/** How often the ages re-measure while nothing else changes. */
const AGE_TICK_MS = 30_000;

const NO_PACKS: readonly RolePackSummary[] = [];

/** React Query key for one project's packs; `null` is the no-project read. */
export function rolePacksQueryKey(projectRef: string | null) {
  return ["role-packs", projectRef] as const;
}

/** The ladder's answer for `projectRef`, cached per project. */
export function useRolePacksQuery(projectRef: string | null) {
  return useQuery({
    queryKey: rolePacksQueryKey(projectRef),
    queryFn: () => listProjectRolePacks(projectRef),
    staleTime: 30_000,
  });
}

function useNowSeconds(): number {
  const [now, setNow] = React.useState(() => Math.floor(Date.now() / 1_000));
  React.useEffect(() => {
    const id = window.setInterval(
      () => setNow(Math.floor(Date.now() / 1_000)),
      AGE_TICK_MS,
    );
    return () => window.clearInterval(id);
  }, []);
  return now;
}

function errorSentence(error: unknown): string | null {
  if (error === null || error === undefined) return null;
  if (error instanceof Error) return error.message;
  return String(error);
}

export type RolesViewState = {
  /** The project whose packs are shown, or `null` when there is none. */
  project: ProjectContainer | null;
  /** Every project the picker offers, in the app's display order. */
  projects: readonly ProjectContainer[];
  /** Why `project` is the one it is (route, chosen, recent checkout, ...). */
  source: RolePacksProjectSource;
  chooseProject: (projectId: string) => void;
  packs: readonly RolePackSummary[];
  packsSource: PacksSourceSummary;
  /** True until the first packs read for this project resolves. */
  isLoading: boolean;
  /** The backend's own sentence when the packs read failed; `null` otherwise. */
  error: string | null;
  /** What the sessions shelf could and could not read — surfaced, not hidden. */
  shelfState: ProjectCodingSessionShelfState;
  view: RolesView;
};

/**
 * Compose the Roles tab: the ladder's packs for the resolved project, the
 * managed agents, and the coding-session shelf across every readable channel,
 * joined by {@link buildRolesView}.
 *
 * The project is resolved the way the Agents tab's installer resolves it
 * (`resolveRolePacksProject`): the route, then the operator's pick, then the
 * most recently recorded checkout, then the first project — and the picker
 * lists `useDisplayProjectContainers()`, so General is offered too. Every
 * memo below depends on data and stable methods, never on a query result
 * object, which is a new reference each render.
 */
export function useRolesView(): RolesViewState {
  const pathname = useLocation({ select: (location) => location.pathname });
  const routeProject = useActiveProjectContainer(pathname, null);
  const projects = useDisplayProjectContainers();
  const workdirs = useQuery({
    enabled: projects.length > 1,
    queryKey: WORKDIR_STATE_QUERY_KEY,
    queryFn: getCodingSessionWorkdirState,
    staleTime: 60_000,
  });
  const workdirsByProject = workdirs.data?.byProject;
  const [chosenId, setChosenId] = React.useState<string | null>(null);
  const resolution = React.useMemo(
    () =>
      resolveRolePacksProject({
        routeProject,
        chosenId,
        projects,
        workdirsByProject,
      }),
    [chosenId, projects, routeProject, workdirsByProject],
  );
  const project = resolution.project;
  const projectRef = project?.address ?? null;

  const packsQuery = useRolePacksQuery(projectRef);
  const packs = packsQuery.data ?? NO_PACKS;
  const packsError = errorSentence(packsQuery.error);
  const packsPending = packsQuery.isPending;

  const agentsQuery = useManagedAgentsQuery();
  const agents = agentsQuery.data;

  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const buckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    NO_PROJECT_BUCKETS,
    NO_PROJECT_BUCKETS,
  );
  const shelfEntries = React.useMemo(
    () => [...[...buckets.byProject.values()].flat(), ...buckets.unclaimed],
    [buckets.byProject, buckets.unclaimed],
  );

  const nowSeconds = useNowSeconds();
  const view = React.useMemo(
    () =>
      buildRolesView({
        rolePacks: packs,
        agents: agents ?? [],
        shelfEntries,
        projects,
        nowSeconds,
      }),
    [agents, nowSeconds, packs, projects, shelfEntries],
  );
  const packsSource = React.useMemo(() => describePacksSource(packs), [packs]);

  return {
    project,
    projects,
    source: resolution.source,
    chooseProject: setChosenId,
    packs,
    packsSource,
    isLoading: packsPending,
    error: packsError,
    shelfState: buckets.state,
    view,
  };
}
