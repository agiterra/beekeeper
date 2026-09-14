import * as React from "react";

import {
  useManagedAgentsQuery,
  useRelayAgentsQuery,
} from "@/features/agents/hooks";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useCommunities } from "@/features/communities/useCommunities";
import { useGlobalCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { projectPulseChannelIds } from "@/features/project-pulse/lib/pulseChannelSet";
import {
  projectPulseChannelSetUnresolved,
  useProjectPulseDigest,
  usePulseDeclaredWork,
} from "@/features/project-pulse/lib/pulseQueries";
import { PULSE_DECLARED_WORK_PAGE_SIZE } from "@/features/project-pulse/lib/pulseMissionSessionRead";
import type { PulseDeclaredWorkSession } from "@/features/project-pulse/lib/pulseDeclaredWorkWire";
import {
  type ProjectContainer,
  partitionChannels,
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfState } from "@/features/projects-container/lib/projectCodingSessionShelf";
import {
  installedRolesForProject,
  useProjectInstalledRolesQuery,
} from "@/features/roles/lib/projectInstalledRoles";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";

import {
  buildProjectAgents,
  type ProjectAgentsIdentityInput,
  type ProjectAgentsModel,
} from "./projectAgentsModel";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();
const NO_CHANNEL_IDS: readonly string[] = [];

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

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** What the assignment read covered, for the page's scope line. */
export type ProjectAgentsAssignmentScope = {
  kind: "loading" | "ready" | "unreadable";
  scannedSessions: number;
  visibleSessions: number;
  message: string | null;
  hasMore: boolean;
  isFetchingMore: boolean;
  fetchMore: () => void;
};

export type ProjectAgentsState = {
  /** The project the route names, or `null` when this reader cannot read it. */
  project: ProjectContainer | null;
  model: ProjectAgentsModel;
  /** True until the session catalog has produced its first read. */
  isLoading: boolean;
  shelfState: ProjectCodingSessionShelfState;
  assignments: ProjectAgentsAssignmentScope;
  /** Each identity or installation read that failed, in its own words. */
  readErrors: string[];
};

/**
 * Compose the project Agents tab: installations from this computer's setup
 * journals, executions from the signed session catalog *before* the umbrella
 * fold, assignments from the declared-work projection, and the umbrella shelf
 * only to name, close and open sessions.
 */
export function useProjectAgents(projectId: string): ProjectAgentsState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );
  const projectRef = project?.address ?? null;

  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });

  // The same readable-member rule the Roles page uses for its session reads.
  const projectChannelIds = React.useMemo(() => {
    if (!project) return NO_CHANNEL_IDS;
    const declared = new Set(project.channelIds);
    return (channelsQuery.data ?? [])
      .filter(
        (channel) =>
          (channel.isMember || isSessionTransportChannel(channel)) &&
          (declared.has(channel.id) || channel.projectRef === project.address),
      )
      .map((channel) => channel.id)
      .sort();
  }, [channelsQuery.data, project]);

  const executionCatalog = useGlobalCodingSessionCatalog(projectChannelIds, {
    authorityMode: "open",
  });

  const projectChannels = React.useMemo(() => {
    const ids = new Set(projectChannelIds);
    return channelsQuery.data?.filter((channel) => ids.has(channel.id));
  }, [channelsQuery.data, projectChannelIds]);
  const buckets = useProjectCodingSessionBuckets(
    projectChannels,
    NO_PROJECT_BUCKETS,
    NO_PROJECT_BUCKETS,
  );
  const umbrellas = React.useMemo(
    () =>
      [...[...buckets.byProject.values()].flat(), ...buckets.unclaimed].filter(
        (entry) => entry.projectId === projectId,
      ),
    [buckets.byProject, buckets.unclaimed, projectId],
  );

  // The declared-work read pages over the Pulse digest's sessions, on the
  // Pulse channel set — the same read Project Pulse shows, so the two
  // surfaces cannot disagree about who was assigned what.
  const pulseChannelIds = React.useMemo(() => {
    if (!project) return NO_CHANNEL_IDS as string[];
    const channels = channelsQuery.data ?? [];
    const partitioned = partitionChannels(projects, channels);
    const bucketed = [
      ...(partitioned.channelsByProject.get(project.id) ?? []),
      ...(partitioned.forumsByProject.get(project.id) ?? []),
    ].map((channel) => channel.id);
    return projectPulseChannelIds(project, channels, bucketed);
  }, [channelsQuery.data, project, projects]);
  const pulse = useProjectPulseDigest(
    projectRef,
    pulseChannelIds,
    projectPulseChannelSetUnresolved(channelsQuery),
  );
  const declared = usePulseDeclaredWork(projectRef, pulseChannelIds, {
    digest: pulse.digest,
  });
  const declaredSessions = React.useMemo(
    () =>
      declared.pages.flatMap(
        (page): readonly PulseDeclaredWorkSession[] => page.response.sessions,
      ),
    [declared.pages],
  );

  const { activeCommunity } = useCommunities();
  const installedQuery = useProjectInstalledRolesQuery(
    activeCommunity?.relayUrl ?? null,
  );
  const installations = React.useMemo(
    () => installedRolesForProject(installedQuery.data, projectRef),
    [installedQuery.data, projectRef],
  );

  const agentsQuery = useManagedAgentsQuery();
  const relayAgentsQuery = useRelayAgentsQuery({
    enabled: projectRef !== null,
  });
  const identities = React.useMemo<ProjectAgentsIdentityInput[]>(() => {
    const byKey = new Map<string, ProjectAgentsIdentityInput>();
    for (const agent of relayAgentsQuery.data ?? []) {
      byKey.set(agent.pubkey.toLowerCase(), {
        pubkey: agent.pubkey,
        name: agent.name || null,
        avatarUrl: null,
        managedHere: false,
      });
    }
    for (const agent of agentsQuery.data ?? []) {
      byKey.set(agent.pubkey.toLowerCase(), {
        pubkey: agent.pubkey,
        name: agent.name,
        avatarUrl: agent.avatarUrl,
        managedHere: true,
      });
    }
    return [...byKey.values()];
  }, [agentsQuery.data, relayAgentsQuery.data]);

  // Assigners are often people (a founder) or agents known only by profile.
  const signerPubkeys = React.useMemo(() => {
    const keys = new Set<string>();
    for (const session of declaredSessions) {
      for (const assignment of session.assignments) {
        keys.add(assignment.assignerPubkey.toLowerCase());
        keys.add(assignment.assigneeActor.toLowerCase());
      }
    }
    for (const entry of executionCatalog.entries) {
      if (entry.session.agentRef)
        keys.add(entry.session.agentRef.toLowerCase());
    }
    return [...keys].sort();
  }, [declaredSessions, executionCatalog.entries]);
  const profiles = useUsersBatchQuery(signerPubkeys).data?.profiles;
  const otherNames = React.useMemo(() => {
    const names = new Map<string, string>();
    if (!profiles) return names;
    for (const pubkey of signerPubkeys) {
      if (!profiles[pubkey]) continue;
      names.set(pubkey, resolveUserLabel({ pubkey, profiles }));
    }
    return names;
  }, [profiles, signerPubkeys]);

  const nowSeconds = useNowSeconds();
  const model = React.useMemo(
    () =>
      buildProjectAgents({
        projectRef: projectRef ?? "",
        executions: executionCatalog.entries,
        umbrellas,
        declaredSessions,
        installations,
        agents: identities,
        otherNames,
        nowSeconds,
      }),
    [
      declaredSessions,
      executionCatalog.entries,
      identities,
      installations,
      nowSeconds,
      otherNames,
      projectRef,
      umbrellas,
    ],
  );

  const readErrors = React.useMemo(() => {
    const errors: string[] = [];
    if (channelsQuery.isError) {
      errors.push(
        `Channels could not be read: ${errorSentence(channelsQuery.error)}`,
      );
    }
    if (executionCatalog.errorMessage)
      errors.push(executionCatalog.errorMessage);
    if (installedQuery.isError) {
      errors.push(
        `Installations on this computer could not be read: ${errorSentence(installedQuery.error)}`,
      );
    }
    if (agentsQuery.isError) {
      errors.push(
        `Local agent identities unavailable: ${errorSentence(agentsQuery.error)}`,
      );
    }
    if (relayAgentsQuery.isError) {
      errors.push(
        `Shared agent identities unavailable: ${errorSentence(relayAgentsQuery.error)}`,
      );
    }
    return errors;
  }, [
    agentsQuery.error,
    agentsQuery.isError,
    channelsQuery.error,
    channelsQuery.isError,
    executionCatalog.errorMessage,
    installedQuery.error,
    installedQuery.isError,
    relayAgentsQuery.error,
    relayAgentsQuery.isError,
  ]);

  const fetchMore = declared.fetchNextPage;
  const assignments = React.useMemo<ProjectAgentsAssignmentScope>(
    () => ({
      kind: declared.kind,
      scannedSessions: Math.min(
        declared.loadedPageCount * PULSE_DECLARED_WORK_PAGE_SIZE,
        declared.visibleSessionCount,
      ),
      visibleSessions: declared.visibleSessionCount,
      message: declared.message,
      hasMore: declared.hasNextPage,
      isFetchingMore: declared.isFetchingNextPage,
      fetchMore,
    }),
    [
      declared.hasNextPage,
      declared.isFetchingNextPage,
      declared.kind,
      declared.loadedPageCount,
      declared.message,
      declared.visibleSessionCount,
      fetchMore,
    ],
  );

  return {
    project,
    model,
    isLoading:
      channelsQuery.isPending ||
      executionCatalog.isLoading ||
      buckets.state.kind === "loading",
    shelfState: buckets.state,
    assignments,
    readErrors,
  };
}
