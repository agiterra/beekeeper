import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useChannelsQuery } from "@/features/channels/hooks";
import {
  type ProjectContainer,
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfState } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { listProjectRolePacks } from "@/shared/api/tauriRolePacks";
import type { RolePackSummary } from "@/shared/api/types";

import {
  buildRolesView,
  describePacksSource,
  type PacksSourceSummary,
  type RolesView,
} from "./rolesViewModel";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();

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

export type ProjectPacksViewState = {
  /** The project the route names, or `null` when this reader cannot read it. */
  project: ProjectContainer | null;
  packs: readonly RolePackSummary[];
  packsSource: PacksSourceSummary;
  /** True until the first packs read for this project resolves. */
  isLoading: boolean;
  /** The backend's own sentence when the packs read failed; `null` otherwise. */
  error: string | null;
  /** What the sessions shelf could and could not read — surfaced, not hidden. */
  shelfState: ProjectCodingSessionShelfState;
  view: RolesView;
  /** Refetch after an install writes new packs or agents. */
  refetchPacks: () => void;
  refetchAgents: () => void;
};

/**
 * Compose the Packs tab of one project page: the ladder's packs for the
 * project the *route* names, the managed agents, and the coding-session
 * shelf across every readable channel, joined by {@link buildRolesView}.
 *
 * Adapted from the old Dashboard Roles tab's `useRolesView` — the only
 * change is the project itself, which this surface's route names outright
 * instead of falling back through `resolveRolePacksProject` (there is no
 * picker here to hand a fallback to). Everything downstream of `project` is
 * unchanged: the packs read, the managed agents, the shelf, and the join.
 */
export function useProjectPacksView(projectId: string): ProjectPacksViewState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );
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
    packs,
    packsSource,
    isLoading: packsPending,
    error: packsError,
    shelfState: buckets.state,
    view,
    refetchPacks: () => void packsQuery.refetch(),
    refetchAgents: () => void agentsQuery.refetch(),
  };
}
