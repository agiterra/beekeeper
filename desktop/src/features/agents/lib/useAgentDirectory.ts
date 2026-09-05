import * as React from "react";

import {
  useBakedBuildEnvQuery,
  useManagedAgentsQuery,
  useRelayAgentsQuery,
} from "@/features/agents/hooks";
import { getInheritedAgentDefaults } from "@/features/agents/ui/bakedEnvHelpers";
import { useGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { useChannelsQuery } from "@/features/channels/hooks";
import {
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
  type ProjectContainer,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { buildSeatRows } from "@/features/roles/lib/seatRows";
import {
  buildAgentDirectory,
  type AgentDirectoryRow,
} from "./agentDirectoryModel";

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

function errorSentence(error: unknown): string | null {
  if (error === null || error === undefined) return null;
  if (error instanceof Error) return error.message;
  return String(error);
}

export type AgentDirectoryState = {
  rows: AgentDirectoryRow[];
  projects: ProjectContainer[];
  isLoading: boolean;
  error: string | null;
  /** True while the seat read is partial/unavailable (§A States — Partial). */
  seatNotice: { message: string; detail: string } | null;
  refetchAgents: () => void;
  refetchRelayAgents: () => void;
};

/**
 * Compose the Agents directory: managed agents, wire-only agents, and seat
 * history (open and closed) across **every** project the viewer can read —
 * unlike `useProjectPacksView` (U2), which scopes the same shelf read to the
 * route's one project, this surface is not project-scoped, so every
 * project's shelf bucket is flattened in, the same way `useProjectPacksView`
 * flattens `buckets.byProject.values()` plus `buckets.unclaimed` for its one
 * project's worth of entries.
 */
export function useAgentDirectory(): AgentDirectoryState {
  const { globalConfig } = useGlobalAgentConfig();
  const { data: bakedEnv } = useBakedBuildEnvQuery({ enabled: true });
  const inheritedDefaults = getInheritedAgentDefaults(globalConfig, bakedEnv);

  const managedAgentsQuery = useManagedAgentsQuery();
  const relayAgentsQuery = useRelayAgentsQuery();
  const projects = useDisplayProjectContainers();

  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const buckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    NO_PROJECT_BUCKETS,
    NO_PROJECT_BUCKETS,
  );

  const shelfEntries = React.useMemo<ProjectCodingSessionShelfEntry[]>(
    () => [...[...buckets.byProject.values()].flat(), ...buckets.unclaimed],
    [buckets.byProject, buckets.unclaimed],
  );
  const openSeatKeys = React.useMemo(
    () =>
      new Set(
        shelfEntries
          .filter((entry) => !entry.isClosed)
          .map((entry) => `${entry.channelId}/${entry.generationId}`),
      ),
    [shelfEntries],
  );

  const nowSeconds = useNowSeconds();
  const seats = React.useMemo(
    () =>
      buildSeatRows({
        shelfEntries,
        agents: managedAgentsQuery.data ?? [],
        projects,
        nowSeconds,
        includeClosed: true,
      }),
    [shelfEntries, managedAgentsQuery.data, projects, nowSeconds],
  );

  const rows = React.useMemo(
    () =>
      buildAgentDirectory({
        managedAgents: managedAgentsQuery.data ?? [],
        relayAgents: relayAgentsQuery.data ?? [],
        seats,
        openSeatKeys,
        defaultModel: inheritedDefaults.model.value,
      }),
    [
      managedAgentsQuery.data,
      relayAgentsQuery.data,
      seats,
      openSeatKeys,
      inheritedDefaults.model.value,
    ],
  );

  const error =
    errorSentence(managedAgentsQuery.error) ??
    errorSentence(relayAgentsQuery.error);

  const seatNotice =
    buckets.state.kind === "partial" || buckets.state.kind === "unavailable"
      ? { message: buckets.state.message, detail: buckets.state.detail }
      : null;

  return {
    rows,
    projects,
    isLoading: managedAgentsQuery.isLoading,
    error,
    seatNotice,
    refetchAgents: () => void managedAgentsQuery.refetch(),
    refetchRelayAgents: () => void relayAgentsQuery.refetch(),
  };
}
