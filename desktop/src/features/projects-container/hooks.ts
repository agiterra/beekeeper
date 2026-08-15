import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useChannelsQuery } from "@/features/channels/hooks";
import { useGlobalCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { useCommunities } from "@/features/communities/useCommunities";
import { allWorkflowsQueryKey } from "@/features/workflows/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import { getChannelsWorkflows } from "@/shared/api/tauriWorkflows";
import type { Workflow } from "@/shared/api/workflowTypes";
import { relayClient } from "@/shared/api/relayClient";
import { KIND_DELETION, KIND_PROJECT } from "@/shared/constants/kinds";
import { useFeatureEnabled } from "@/shared/features";
import type { Channel } from "@/shared/api/types";
import {
  useProjectsQuery,
  type Repository as CodeRepo,
} from "@/features/projects/hooks";
import {
  bucketProjectCodingSessions,
  resolveProjectCodingSessionPlacement,
  resolveProjectCodingSessionShelf,
  type ProjectCodingSessionShelfEntry,
  type ProjectCodingSessionShelfState,
} from "./lib/projectCodingSessionShelf";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  canonicalizeProjectContainers,
  dedupProjectEvents,
  displayProjectsWithGeneral,
  eventToProjectContainer,
  isProjectContainerDeleted,
  normalizeProjectMemberEntries,
  parseMemberRef,
  partitionByChannelProject,
  partitionByProject,
  sortProjectContainers,
  type ProjectContainer,
} from "./lib/projectContainerModel";

export type { ProjectContainer };

export async function fetchProjectContainers(): Promise<ProjectContainer[]> {
  const [events, deletionEvents] = await Promise.all([
    relayClient.fetchEvents({ kinds: [KIND_PROJECT], limit: 200 }),
    relayClient.fetchEvents({ kinds: [KIND_DELETION], limit: 500 }),
  ]);

  const projects: ProjectContainer[] = [];
  for (const event of dedupProjectEvents(events)) {
    const project = eventToProjectContainer(event);
    if (project && !isProjectContainerDeleted(project, deletionEvents)) {
      projects.push(project);
    }
  }
  return sortProjectContainers(canonicalizeProjectContainers(projects));
}

export const projectContainersQueryKey = ["project-containers"] as const;

/** Relay-scoped cache key shared by the sidebar query and boot-time callers. */
export function projectContainersQueryKeyFor(relayUrl: string | undefined) {
  return [...projectContainersQueryKey, relayUrl ?? "none"] as const;
}

/**
 * Last-known containers per relay+viewer, so the sidebar renders the real
 * project layout on the very first frame instead of collapsing everything
 * into the General placeholder until the relay socket has connected and
 * answered. Scoped by viewer pubkey (not just relay URL): private projects
 * make this snapshot identity-sensitive — without the pubkey in the key, a
 * device with multiple local identities on the same relay could flash one
 * identity's project list (including private project names) while switched
 * to another.
 *
 * Registered in LOCAL_STORAGE_SWEEP_RULES (shared/lib/localStorageSweep.ts):
 * the payload carries a root `updatedAt` so relay+identity combinations that
 * have not been opened in 14 days are swept instead of accreting forever.
 */
const CONTAINER_SNAPSHOT_PREFIX = "buzz.projects.containers.v1:";

function containerSnapshotKey(
  relayUrl: string | undefined,
  viewerPubkey: string | undefined,
): string | undefined {
  if (!relayUrl || !viewerPubkey) return undefined;
  return `${CONTAINER_SNAPSHOT_PREFIX}${relayUrl}:${viewerPubkey}`;
}

/** Snapshots written before the roles migration hold `members: string[]`;
 * normalize each entry to the `{pubkey, role}` shape before use. */
function normalizeSnapshotProjects(projects: unknown[]): ProjectContainer[] {
  return (projects as ProjectContainer[]).map((project) => ({
    ...project,
    members: normalizeProjectMemberEntries(project.members),
  }));
}

function readContainerSnapshot(
  key: string | undefined,
): ProjectContainer[] | undefined {
  if (!key) return undefined;
  try {
    const raw = globalThis.localStorage?.getItem(key);
    if (!raw) return undefined;
    const parsed: unknown = JSON.parse(raw);
    // Legacy shape: a bare array (pre-sweep-registration). Still readable;
    // the next successful fetch rewrites it in the swept shape.
    if (Array.isArray(parsed)) return normalizeSnapshotProjects(parsed);
    if (typeof parsed !== "object" || parsed === null) return undefined;
    const projects = (parsed as { projects?: unknown }).projects;
    return Array.isArray(projects)
      ? normalizeSnapshotProjects(projects)
      : undefined;
  } catch {
    return undefined;
  }
}

function writeContainerSnapshot(
  key: string | undefined,
  projects: ProjectContainer[],
): void {
  if (!key) return;
  try {
    // Root `updatedAt` is what LOCAL_STORAGE_SWEEP_RULES keys the TTL on.
    globalThis.localStorage?.setItem(
      key,
      JSON.stringify({ updatedAt: Date.now(), projects }),
    );
  } catch {
    // Best-effort cache — quota/serialization failures just lose the
    // fast first paint, never the live data.
  }
}

/**
 * Fetch containers and refresh the first-paint snapshot for this
 * relay+viewer. Shared by the sidebar query and the boot-time migration so
 * whichever runs first keeps the snapshot warm.
 */
export async function fetchAndSnapshotProjectContainers(
  relayUrl: string | undefined,
  viewerPubkey: string | undefined,
): Promise<ProjectContainer[]> {
  const projects = await fetchProjectContainers();
  writeContainerSnapshot(
    containerSnapshotKey(relayUrl, viewerPubkey),
    projects,
  );
  return projects;
}

export function useProjectContainersQuery(options?: { enabled?: boolean }) {
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl;
  const identityQuery = useIdentityQuery();
  const viewerPubkey = identityQuery.data?.pubkey?.toLowerCase();
  const snapshotKey = containerSnapshotKey(relayUrl, viewerPubkey);
  return useQuery({
    enabled: options?.enabled ?? true,
    // Relay-scoped: community switches must not briefly show the previous
    // community's projects out of the shared cache slot.
    queryKey: projectContainersQueryKeyFor(relayUrl),
    queryFn: () => fetchAndSnapshotProjectContainers(relayUrl, viewerPubkey),
    // No placeholder until the viewer identity is known — showing a snapshot
    // before that would risk leaking a project list across identity switches.
    placeholderData: () => readContainerSnapshot(snapshotKey),
    staleTime: 60_000,
  });
}

export function useProjectContainerQuery(projectId: string) {
  const containers = useProjectContainersQuery();
  const project = React.useMemo(
    () =>
      (containers.data ?? []).find(
        (candidate) =>
          candidate.id === projectId || candidate.dtag === projectId,
      ) ?? null,
    [containers.data, projectId],
  );
  return { ...containers, project };
}

export type ProjectChannelBuckets = {
  /** Stream channels per project id. */
  channelsByProject: Map<string, Channel[]>;
  /** Forum channels per project id. */
  forumsByProject: Map<string, Channel[]>;
  /** Stream channels not claimed by any project (the global section). */
  globalChannels: Channel[];
  /** Forum channels not claimed by any project. */
  unclaimedForums: Channel[];
};

/**
 * Splits stream/forum channels across project containers. A channel belongs
 * to a project when the project's event lists its id (forward ref) or the
 * channel itself carries a matching `projectRef` back-reference.
 */
export function partitionChannels(
  projects: ProjectContainer[],
  channels: Channel[],
): ProjectChannelBuckets {
  const streams = channels.filter((c) => c.channelType === "stream");
  const forums = channels.filter((c) => c.channelType === "forum");
  const streamBuckets = partitionByProject(
    projects,
    streams,
    (channel) => channel.id,
    (project) => project.channelIds,
    (channel) => channel.projectRef,
  );
  const forumBuckets = partitionByProject(
    projects,
    forums,
    (channel) => channel.id,
    (project) => project.channelIds,
    (channel) => channel.projectRef,
  );
  return {
    channelsByProject: streamBuckets.byProject,
    forumsByProject: forumBuckets.byProject,
    globalChannels: streamBuckets.unclaimed,
    unclaimedForums: forumBuckets.unclaimed,
  };
}

export type ProjectRepoBuckets = {
  reposByProject: Map<string, CodeRepo[]>;
  unclaimedRepos: CodeRepo[];
};

/**
 * Splits NIP-34 repos (the Code section) across project containers, using the
 * project event's `a` refs plus each repo announcement's `project` tag.
 */
export function partitionRepos(
  projects: ProjectContainer[],
  repos: CodeRepo[],
): ProjectRepoBuckets {
  const { byProject, unclaimed } = partitionByProject(
    projects,
    repos,
    (repo) => repo.repoAddress,
    (project) => project.repoAddrs,
    (repo) => repo.projectRef,
  );
  return { reposByProject: byProject, unclaimedRepos: unclaimed };
}

/**
 * Project containers plus repo buckets in one hook — the shape the sidebar
 * needs. Channel partitioning is exposed separately (`partitionChannels`)
 * because the sidebar already owns the channel list as a prop.
 */
export function useProjectContainers(options?: { enabled?: boolean }) {
  const containersQuery = useProjectContainersQuery(options);
  const reposQuery = useProjectsQuery();

  const projects = React.useMemo(
    () => containersQuery.data ?? [],
    [containersQuery.data],
  );
  // NIP-MP projects are multi-repo; containers curate individual repo
  // announcements, so flatten and dedup by address.
  const repos = React.useMemo(
    () => [
      ...new Map(
        (reposQuery.data ?? [])
          .flatMap((project) => project.repositories)
          .map((repo) => [repo.repoAddress, repo] as const),
      ).values(),
    ],
    [reposQuery.data],
  );
  const repoBuckets = React.useMemo(
    () => partitionRepos(projects, repos),
    [projects, repos],
  );

  return {
    projects,
    isLoading: containersQuery.isLoading,
    ...repoBuckets,
  };
}

/**
 * The project list as displayed everywhere (sidebar, manage panel, filters):
 * the published containers, with the local General placeholder prepended
 * while no real `general` head exists yet.
 */
export function useDisplayProjectContainers(): ProjectContainer[] {
  const { projects } = useProjectContainers();
  return React.useMemo(() => displayProjectsWithGeneral(projects), [projects]);
}

/**
 * Workflows bucketed by the project owning their trigger channel (a
 * kind:30620 def is always channel-scoped via its `h` tag); workflows of
 * unclaimed channels land in `unclaimed` (displayed under General).
 */
export function useProjectWorkflowBuckets(
  channels: Channel[] | undefined,
  channelsByProject: ReadonlyMap<string, Channel[]>,
  forumsByProject: ReadonlyMap<string, Channel[]>,
): {
  workflowsEnabled: boolean;
  byProject: ReadonlyMap<string, Workflow[]>;
  unclaimed: Workflow[];
} {
  const workflowsEnabled = useFeatureEnabled("workflows");
  const workflowChannelIds = React.useMemo(
    () => (channels ?? []).map((channel) => channel.id).sort(),
    [channels],
  );
  const workflowsQuery = useQuery({
    enabled: workflowsEnabled && workflowChannelIds.length > 0,
    queryKey: allWorkflowsQueryKey(`project:${workflowChannelIds.join(",")}`),
    queryFn: () => getChannelsWorkflows(workflowChannelIds),
    staleTime: 30_000,
  });
  const buckets = React.useMemo(() => {
    const channelIdToProjectId = new Map<string, string>();
    for (const [ownerId, owned] of channelsByProject) {
      for (const channel of owned)
        channelIdToProjectId.set(channel.id, ownerId);
    }
    for (const [ownerId, owned] of forumsByProject) {
      for (const forum of owned) channelIdToProjectId.set(forum.id, ownerId);
    }
    return partitionByChannelProject(
      workflowsQuery.data ?? [],
      channelIdToProjectId,
    );
  }, [workflowsQuery.data, channelsByProject, forumsByProject]);
  return {
    workflowsEnabled,
    byProject: buckets.byProject,
    unclaimed: buckets.unclaimed,
  };
}

/**
 * Trusted coding sessions bucketed by the project that owns them.
 *
 * Same shape as `useProjectWorkflowBuckets` because a session is the same kind
 * of child: its signed events are `h`-scoped to one channel, so the channel's
 * project owns it — except that a session can also *state* its project in the
 * signed 44223 metadata, and that statement wins (see
 * `resolveProjectCodingSessionPlacement`). Sessions nothing claims land in
 * `unclaimed`, displayed under General.
 *
 * Ingress is scoped to channels the viewer has joined: 442xx is strict-
 * membership, so a channel you are not in has no readable sessions to show and
 * subscribing to it would only cost a filter.
 */
export function useProjectCodingSessionBuckets(
  channels: Channel[] | undefined,
  channelsByProject: ReadonlyMap<string, Channel[]>,
  forumsByProject: ReadonlyMap<string, Channel[]>,
): {
  byProject: ReadonlyMap<string, ProjectCodingSessionShelfEntry[]>;
  unclaimed: ProjectCodingSessionShelfEntry[];
  state: ProjectCodingSessionShelfState;
} {
  const { projects } = useProjectContainers();
  const sessionChannelIds = React.useMemo(
    () =>
      (channels ?? [])
        // Session transports are readable through the project ACL without a
        // channel_members row, so a member subscribes to them too — the relay
        // only ever returns transports the reader is admitted to.
        .filter(
          (channel) => channel.isMember || isSessionTransportChannel(channel),
        )
        .map((channel) => channel.id)
        .sort(),
    [channels],
  );
  const stableChannelIds = useStableArrayShallow(sessionChannelIds);
  const catalog = useGlobalCodingSessionCatalog(stableChannelIds);

  const placementIndex = React.useMemo(() => {
    const projectIdByRef = new Map<string, string>();
    for (const project of projects) {
      projectIdByRef.set(project.address, project.id);
    }
    const projectIdByChannel = new Map<string, string>();
    for (const [ownerId, owned] of channelsByProject) {
      for (const channel of owned) projectIdByChannel.set(channel.id, ownerId);
    }
    for (const [ownerId, owned] of forumsByProject) {
      for (const forum of owned) projectIdByChannel.set(forum.id, ownerId);
    }
    return { projectIdByRef, projectIdByChannel };
  }, [projects, channelsByProject, forumsByProject]);

  const channelLabels = React.useMemo(
    () =>
      new Map((channels ?? []).map((channel) => [channel.id, channel.name])),
    [channels],
  );

  return React.useMemo(() => {
    const shelf = resolveProjectCodingSessionShelf(
      catalog,
      placementIndex,
      channelLabels,
    );
    return {
      ...bucketProjectCodingSessions(shelf.entries),
      state: shelf.state,
    };
  }, [catalog, channelLabels, placementIndex]);
}

/**
 * The project a single coding session belongs to, for the session header's
 * crumb.
 *
 * Deliberately the same `resolveProjectCodingSessionPlacement` the sidebar
 * buckets with, so the header can never name a project the session is not
 * filed under.
 */
export function useCodingSessionProject(
  channelId: string | null,
  projectRef: string | null,
): { id: string; name: string } | null {
  const { projects } = useProjectContainers();
  // Transports included: a session hosted in a hidden transport channel must
  // still resolve to its project for the header crumb.
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  return React.useMemo(() => {
    if (!channelId) return null;
    const buckets = partitionChannels(projects, channelsQuery.data ?? []);
    const projectIdByChannel = new Map<string, string>();
    for (const [ownerId, owned] of buckets.channelsByProject) {
      for (const channel of owned) projectIdByChannel.set(channel.id, ownerId);
    }
    for (const [ownerId, owned] of buckets.forumsByProject) {
      for (const forum of owned) projectIdByChannel.set(forum.id, ownerId);
    }
    const { projectId } = resolveProjectCodingSessionPlacement(
      projectRef,
      channelId,
      {
        projectIdByRef: new Map(
          projects.map((project) => [project.address, project.id]),
        ),
        projectIdByChannel,
      },
    );
    const owner = projectId
      ? projects.find((project) => project.id === projectId)
      : undefined;
    return owner ? { id: owner.id, name: owner.name } : null;
  }, [channelId, channelsQuery.data, projectRef, projects]);
}

/**
 * Resolves which project container a repo should be opened under: its own
 * `project` back-reference first, then any container whose curated list
 * claims it, then General (the published one, or the local placeholder).
 */
export type RepoContainerRef = Pick<CodeRepo, "repoAddress" | "projectRef">;

export function useRepoContainerId(): (
  repo: RepoContainerRef | undefined,
) => string {
  const { projects } = useProjectContainers();
  return React.useCallback(
    (repo: RepoContainerRef | undefined) => {
      if (repo?.projectRef) {
        const ref = parseMemberRef(repo.projectRef);
        if (ref && ref.kind === KIND_PROJECT) {
          return `${ref.owner}:${ref.dtag}`;
        }
      }
      const claiming = repo
        ? projects.find((project) =>
            project.repoAddrs.includes(repo.repoAddress),
          )
        : undefined;
      if (claiming) return claiming.id;
      const general = projects.find(
        (project) => project.dtag === GENERAL_PROJECT_DTAG,
      );
      return general ? general.id : LOCAL_GENERAL_ID;
    },
    [projects],
  );
}

/**
 * Everything AppSidebar needs from the Projects experiment in one call:
 * containers, repo buckets, channel buckets, and the stream-channel list for
 * the global Channels section (all streams when the experiment is off;
 * only project-unclaimed streams when it is on).
 */
export function useProjectSidebarData(channels: Channel[], enabled: boolean) {
  const containers = useProjectContainers({ enabled });
  // Project groups are a browse surface: they list every open channel in the
  // community (join-on-open in the sidebar), plus private channels you're a
  // member of — not just joined ones. The member-only `channels` prop still
  // drives the flag-off global sections below.
  const allChannelsQuery = useChannelsQuery({
    enabled,
    // Transports ride along for session/workflow placement; every display
    // list downstream filters them via withoutProjectSessionTransportChannels.
    includeSessionTransports: true,
  });
  const projectChannels = React.useMemo(() => {
    if (!enabled) return channels;
    const community = (allChannelsQuery.data ?? []).filter(
      (channel) =>
        channel.archivedAt === null &&
        (channel.isMember ||
          channel.visibility === "open" ||
          // A transport in the list is one the relay already admitted this
          // reader to (project member) — membership rows never exist for it.
          isSessionTransportChannel(channel)),
    );
    // Until the full community list arrives, fall back to the member list so
    // the sidebar never blanks out.
    return community.length > 0 ? community : channels;
  }, [enabled, allChannelsQuery.data, channels]);
  const channelBuckets = React.useMemo(
    () =>
      partitionChannels(enabled ? containers.projects : [], projectChannels),
    [enabled, containers.projects, projectChannels],
  );
  const globalStreamChannels = React.useMemo(
    () =>
      enabled
        ? channelBuckets.globalChannels
        : channels.filter((channel) => channel.channelType === "stream"),
    [enabled, channelBuckets.globalChannels, channels],
  );
  return { ...containers, ...channelBuckets, globalStreamChannels };
}
