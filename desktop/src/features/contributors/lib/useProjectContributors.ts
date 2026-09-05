import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useChannelsQuery } from "@/features/channels/hooks";
import {
  type ProjectContainer,
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfState } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { buildSeatRows } from "@/features/roles/lib/seatRows";

import { buildContributorRows, type ContributorRow } from "./contributorsModel";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();

/** How often the ages re-measure while nothing else changes. */
const AGE_TICK_MS = 30_000;

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

export type ProjectContributorsState = {
  /** The project the route names, or `null` when this reader cannot read it. */
  project: ProjectContainer | null;
  rows: ContributorRow[];
  /** True until the coding-session shelf has produced its first read. */
  isLoading: boolean;
  /** What the sessions shelf could and could not read — surfaced, not hidden. */
  shelfState: ProjectCodingSessionShelfState;
};

/**
 * Compose the Contributors tab of one project page: every seat this project
 * has ever held — open or closed — grouped into one row per agent.
 *
 * Deliberately **not** built on `buildRolesView`, which drops closed shelf
 * entries (`rolesViewModel.ts`, the seat join's `includeClosed` default):
 * this surface's whole point is seat history, so it calls `buildSeatRows`
 * itself with `includeClosed: true` and keeps its own record of which of
 * those seats are still open.
 */
export function useProjectContributors(
  projectId: string,
): ProjectContributorsState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );

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
  const projectShelfEntries = React.useMemo(
    () => shelfEntries.filter((entry) => entry.projectId === projectId),
    [shelfEntries, projectId],
  );

  const nowSeconds = useNowSeconds();

  const rows = React.useMemo(() => {
    const seats = buildSeatRows({
      shelfEntries: projectShelfEntries,
      agents: agents ?? [],
      projects,
      nowSeconds,
      includeClosed: true,
    });
    const openKeys = new Set(
      projectShelfEntries
        .filter((entry) => !entry.isClosed)
        .map((entry) => `${entry.channelId}/${entry.generationId}`),
    );
    return buildContributorRows({ seats, openKeys });
  }, [agents, nowSeconds, projectShelfEntries, projects]);

  return {
    project,
    rows,
    isLoading: buckets.state.kind === "loading",
    shelfState: buckets.state,
  };
}
