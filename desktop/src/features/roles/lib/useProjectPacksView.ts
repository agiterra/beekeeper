import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useChannelsQuery } from "@/features/channels/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import {
  type ProjectContainer,
  useDisplayProjectContainers,
  useProjectCodingSessionBuckets,
} from "@/features/projects-container/hooks";
import type { ProjectCodingSessionShelfState } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { useGlobalCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import {
  compareProjectPackRevisions,
  listProjectRolePacks,
} from "@/shared/api/tauriRolePacks";
import type { RolePackSummary } from "@/shared/api/types";
import {
  fetchProjectPackSource,
  projectPackSourceQueryKey,
} from "@/features/projects-container/lib/projectPackSource";

import {
  buildRolesView,
  describePacksSource,
  type PacksSourceSummary,
  type RolesView,
} from "./rolesViewModel";
import {
  buildRolePackSnapshots,
  revisionShas,
  type RolePackSnapshots,
} from "./rolePackSnapshots";
import {
  projectPacksResolutionNeedsRefresh,
  rolePacksQueryKey,
  useProjectPacksLiveInvalidation,
} from "./rolePacksLiveInvalidation";

const NO_PROJECT_BUCKETS: ReadonlyMap<string, never[]> = new Map();

/** How often the ages re-measure while nothing else changes. */
const AGE_TICK_MS = 30_000;

const NO_PACKS: readonly RolePackSummary[] = [];
const NO_CHANNEL_IDS: readonly string[] = [];

export { rolePacksQueryKey };

/** The ladder's answer for `projectRef`, cached per project. */
type ResolvedRolePacks = {
  packs: RolePackSummary[];
  sourceEventId: string | null | undefined;
};

export function useRolePacksQuery(
  projectRef: string | null,
  sourceEventId: string | null | undefined,
) {
  const query = useQuery({
    queryKey: rolePacksQueryKey(projectRef),
    queryFn: async (): Promise<ResolvedRolePacks> => ({
      packs: await listProjectRolePacks(projectRef),
      sourceEventId,
    }),
    staleTime: 30_000,
  });
  const attemptedRevision = React.useRef<{
    projectRef: string | null;
    sourceEventId: string | null;
  } | null>(null);
  React.useEffect(() => {
    if (attemptedRevision.current?.projectRef !== projectRef) {
      attemptedRevision.current = null;
    }
    const alreadyAttempted =
      attemptedRevision.current?.projectRef === projectRef &&
      attemptedRevision.current.sourceEventId === sourceEventId;
    if (
      query.data === undefined ||
      query.isFetching ||
      !projectPacksResolutionNeedsRefresh(
        query.data.sourceEventId,
        sourceEventId,
      ) ||
      alreadyAttempted
    ) {
      return;
    }
    // The mismatch helper excludes `undefined`, so the attempted revision is
    // known here. Remember project + revision to avoid a failed auto-refresh
    // loop without suppressing the same revision in another project.
    attemptedRevision.current = {
      projectRef,
      sourceEventId: sourceEventId ?? null,
    };
    void query.refetch();
  }, [projectRef, query.data, query.isFetching, query.refetch, sourceEventId]);
  return query;
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
  /**
   * This machine's local pack-resolution snapshot compared with unverified
   * signed metadata claims from the project's readable channels.
   */
  rolePackSnapshots: RolePackSnapshots;
  /** When the most recent resolver refetch failed after a prior answer. */
  packsResolutionIsStale: boolean;
  /** The revision comparison's own disclosed error, or `null`. */
  revisionsError: string | null;
  /** The open metadata-claim read's disclosed state. */
  executionReports: {
    isLoading: boolean;
    error: string | null;
    authorityError: string | null;
  };
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
 * picker here to hand a fallback to). The packs read, managed agents, and
 * shelf stay project-scoped; revision reporting additionally keeps each raw
 * visible metadata generation before the shelf folds them into umbrellas.
 */
export function useProjectPacksView(projectId: string): ProjectPacksViewState {
  const projects = useDisplayProjectContainers();
  const project = React.useMemo(
    () => projects.find((candidate) => candidate.id === projectId) ?? null,
    [projects, projectId],
  );
  const projectRef = project?.address ?? null;

  const sourceQuery = useQuery({
    enabled: projectRef !== null,
    queryKey: projectPackSourceQueryKey(projectRef ?? ""),
    queryFn: () =>
      projectRef === null
        ? Promise.resolve(null)
        : fetchProjectPackSource(projectRef),
    staleTime: 30_000,
  });
  const liveRefresh = useProjectPacksLiveInvalidation(
    projectRef,
    sourceQuery.data ?? null,
  );

  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });

  // A project can name an ordinary channel itself, while its session transport
  // names the project from the channel side. Include both shapes, using the
  // same readable-member rule as the shelf. The second project-ref check in
  // buildRolePackSnapshots prevents a shared channel from lending another
  // project's metadata claim.
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

  const packsQuery = useRolePacksQuery(
    projectRef,
    sourceQuery.isSuccess ? (sourceQuery.data?.eventId ?? null) : undefined,
  );
  const packs = packsQuery.data?.packs ?? NO_PACKS;
  const packsError = React.useMemo(() => {
    const errors = [
      errorSentence(packsQuery.error),
      sourceQuery.isError
        ? `The pack-source refresh failed: ${errorSentence(sourceQuery.error) ?? "unknown error"}. Reconnect or reopen this Packs tab to retry.`
        : null,
      liveRefresh.error,
    ].filter((error): error is string => error !== null);
    return errors.length > 0 ? errors.join(" ") : null;
  }, [
    liveRefresh.error,
    packsQuery.error,
    sourceQuery.error,
    sourceQuery.isError,
  ]);
  const packsPending = packsQuery.isPending;

  const agentsQuery = useManagedAgentsQuery();
  const agents = agentsQuery.data;

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

  // The project's current source repo comes from the signed 30624 itself, so
  // a source this machine could not sync still names the repository the
  // claims are measured against. The project-origin rung's own coordinate is
  // the fallback (it can only exist once a source was read) and supplies the
  // sha this machine last landed on; `currentResolvedSha` only busts the
  // revisions query's cache when this machine's checkout advances — the
  // comparison's own `currentSha` is the fact that lands on screen.
  const projectOriginRef = React.useMemo(
    () =>
      packs.find((pack) => pack.origin === "project" && pack.packRef)
        ?.packRef ?? null,
    [packs],
  );
  const sourceRepo = sourceQuery.data?.repo ?? projectOriginRef?.repo ?? null;
  const sourceKnown = sourceQuery.isSuccess || projectOriginRef !== null;
  const currentResolvedSha = projectOriginRef?.sha ?? null;

  const shasToCompare = React.useMemo(
    () => revisionShas(executionCatalog.entries, sourceRepo),
    [executionCatalog.entries, sourceRepo],
  );
  const revisionsQuery = useQuery({
    queryKey: [
      "role-pack-revisions",
      projectRef,
      currentResolvedSha,
      shasToCompare,
    ],
    queryFn: () => compareProjectPackRevisions(projectRef ?? "", shasToCompare),
    enabled: projectRef !== null && shasToCompare.length > 0,
    staleTime: 30_000,
  });
  const revisionsError = errorSentence(revisionsQuery.error);

  const rolePackSnapshots = React.useMemo(
    () =>
      buildRolePackSnapshots({
        projectRef: projectRef ?? "",
        resolvedAt: packsQuery.dataUpdatedAt || null,
        resolvedPacks: packs,
        catalogEntries: executionCatalog.entries,
        sourceRepo,
        sourceKnown,
        revisions: revisionsQuery.data ?? null,
        revisionsError,
        nowSeconds,
      }),
    [
      executionCatalog.entries,
      nowSeconds,
      packs,
      packsQuery.dataUpdatedAt,
      projectRef,
      revisionsError,
      revisionsQuery.data,
      sourceKnown,
      sourceRepo,
    ],
  );
  // The session catalog is scoped by channelsQuery.data. If that prerequisite
  // has not completed or fails, an empty catalog is not evidence that this
  // project has no metadata claims.
  const executionReportsError = React.useMemo(() => {
    const errors = [
      errorSentence(channelsQuery.error),
      executionCatalog.errorMessage,
    ].filter((error): error is string => error !== null);
    return errors.length > 0 ? errors.join(" ") : null;
  }, [channelsQuery.error, executionCatalog.errorMessage]);

  return {
    project,
    packs,
    packsSource,
    isLoading: packsPending,
    error: packsError,
    shelfState: buckets.state,
    rolePackSnapshots,
    packsResolutionIsStale:
      packsQuery.dataUpdatedAt > 0 &&
      (packsQuery.isError || sourceQuery.isError || liveRefresh.error !== null),
    revisionsError,
    executionReports: {
      isLoading: channelsQuery.isPending || executionCatalog.isLoading,
      error: executionReportsError,
      authorityError: executionCatalog.authorityErrorMessage,
    },
    view,
    refetchPacks: () => {
      liveRefresh.retry();
      void sourceQuery.refetch();
      void packsQuery.refetch();
      void revisionsQuery.refetch();
    },
    refetchAgents: () => void agentsQuery.refetch(),
  };
}
