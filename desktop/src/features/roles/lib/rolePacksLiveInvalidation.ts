import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";

import {
  projectPackSourceQueryKey,
  projectPackSourceRepoId,
  type ProjectPackSource,
} from "@/features/projects-container/lib/projectPackSource";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_PROJECT_PACK_SOURCE,
  KIND_REPO_STATE,
} from "@/shared/constants/kinds";

export function rolePacksQueryKey(projectRef: string | null) {
  return ["role-packs", projectRef] as const;
}

/**
 * The `compare_project_pack_revisions` query key — every input that can
 * change *what this machine's checkout would answer* must be a key segment,
 * or a stale answer survives past the change that invalidated it:
 *
 * - `sourceEventId`: the signed 30624 revision the comparison was measured
 *   against. A source edit (even one that leaves `sourceRepo` unchanged,
 *   e.g. a path or ref change) must re-rank.
 * - `sourceRepo`: switching the configured repository must never reuse an
 *   answer measured against the old one.
 * - `currentResolvedSha` and `packsUpdatedAt`: a manual refresh can change
 *   which commits this machine's checkout actually holds (a `git fetch`
 *   that leaves `HEAD` itself unchanged) without moving either of those
 *   shas — `packsUpdatedAt` is the packs list's own refresh completion, so
 *   it catches that case too.
 * - `shas`: the exact set being asked about.
 *
 * Pure key-shape only — does not sort `shas`. Callers pass
 * {@link revisionShas}'s already-sorted output; sorting here as well would
 * hide a caller regression instead of surfacing it as a cache miss.
 */
export function rolePackRevisionsQueryKey(input: {
  projectRef: string | null;
  sourceEventId: string | null;
  sourceRepo: string | null;
  currentResolvedSha: string | null;
  packsUpdatedAt: number;
  shas: readonly string[];
}) {
  return [
    "role-pack-revisions",
    input.projectRef,
    input.sourceEventId,
    input.sourceRepo,
    input.currentResolvedSha,
    input.packsUpdatedAt,
    input.shas,
  ] as const;
}

type RolePacksLiveClient = {
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void | Promise<void>>;
  subscribeToReconnects(listener: () => void): () => void;
};

const MAX_SEEN_EVENT_IDS = 64;

export type ProjectPacksLiveEffect = "none" | "packs" | "source";

/**
 * Whether a native pack result was resolved against a different authoritative
 * source revision. `undefined` means the source read has not answered yet.
 */
export function projectPacksResolutionNeedsRefresh(
  resolvedSourceEventId: string | null | undefined,
  currentSourceEventId: string | null | undefined,
): boolean {
  return (
    currentSourceEventId !== undefined &&
    resolvedSourceEventId !== currentSourceEventId
  );
}

function tagValue(event: RelayEvent, name: string): string | null {
  return event.tags.find((tag) => tag[0] === name)?.[1] ?? null;
}

/** Classify one subscribed event as a refresh hint, never as pack data. */
export function projectPacksLiveEffect(input: {
  event: RelayEvent;
  projectRef: string;
  sourceEventId: string | null;
  sourceRef: string | null;
  sourceRepoId: string | null;
}): ProjectPacksLiveEffect {
  const dTag = tagValue(input.event, "d");
  if (
    input.event.kind === KIND_PROJECT_PACK_SOURCE &&
    dTag === input.projectRef
  ) {
    return input.event.id === input.sourceEventId ? "none" : "source";
  }
  if (
    input.event.kind === KIND_REPO_STATE &&
    input.sourceRef !== null &&
    dTag === input.sourceRepoId
  ) {
    // Any new repository-state head matters. The configured branch may have
    // been deleted, in which case its ref tag is deliberately absent.
    return "packs";
  }
  return "none";
}

/** Exact live filters for one project source and its optional moving ref. */
export function projectPacksLiveFilters(
  projectRef: string,
  source: ProjectPackSource | null,
): RelaySubscriptionFilter[] {
  return projectPacksLiveFiltersForRoute(
    projectRef,
    source?.ref ?? null,
    source ? projectPackSourceRepoId(source) : null,
  );
}

function projectPacksLiveFiltersForRoute(
  projectRef: string,
  sourceRef: string | null,
  sourceRepoId: string | null,
): RelaySubscriptionFilter[] {
  const filters: RelaySubscriptionFilter[] = [
    {
      kinds: [KIND_PROJECT_PACK_SOURCE],
      "#d": [projectRef],
      limit: 1,
    },
  ];
  if (sourceRef !== null && sourceRepoId !== null) {
    filters.push({
      kinds: [KIND_REPO_STATE],
      "#d": [sourceRepoId],
      limit: 1,
    });
  }
  return filters;
}

/**
 * Refresh the authoritative native pack resolver when its signed source or
 * moving repository ref changes. Event ids are remembered for this mounted
 * scope so reconnect replay cannot repeatedly run the git resolver.
 */
export function useProjectPacksLiveInvalidation(
  projectRef: string | null,
  source: ProjectPackSource | null,
  client: RolePacksLiveClient = defaultRelayClient,
): { error: string | null; retry: () => void } {
  const queryClient = useQueryClient();
  const [error, setError] = React.useState<string | null>(null);
  const [retryEpoch, setRetryEpoch] = React.useState(0);
  const sourceEventId = source?.eventId ?? null;
  const sourceRef = source?.ref ?? null;
  const sourceRepoId = source ? projectPackSourceRepoId(source) : null;
  const filters = React.useMemo(
    () =>
      projectRef === null
        ? []
        : projectPacksLiveFiltersForRoute(projectRef, sourceRef, sourceRepoId),
    [projectRef, sourceRef, sourceRepoId],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: retryEpoch is an intentional rearm signal after a failed watch.
  React.useEffect(() => {
    if (projectRef === null) return;
    let disposed = false;
    let watchFailed = false;
    const disposers: Array<() => void | Promise<void>> = [];
    const seenEventIds = new Set<string>();
    if (sourceEventId !== null) seenEventIds.add(sourceEventId);
    setError(null);

    const rememberEvent = (eventId: string) => {
      seenEventIds.add(eventId);
      if (seenEventIds.size <= MAX_SEEN_EVENT_IDS) return;
      const oldest = seenEventIds.values().next().value;
      if (oldest !== undefined) seenEventIds.delete(oldest);
    };

    const invalidatePacks = () => {
      if (disposed) return;
      void queryClient.invalidateQueries({
        queryKey: rolePacksQueryKey(projectRef),
      });
    };
    const invalidateSourceAndPacks = () => {
      if (disposed) return;
      void queryClient.invalidateQueries({
        queryKey: projectPackSourceQueryKey(projectRef),
      });
      invalidatePacks();
    };
    const onEvent = (event: RelayEvent) => {
      if (disposed) return;
      if (seenEventIds.has(event.id)) return;
      const effect = projectPacksLiveEffect({
        event,
        projectRef,
        sourceEventId,
        sourceRef,
        sourceRepoId,
      });
      if (effect === "none") return;
      rememberEvent(event.id);
      if (effect === "source") {
        // The event is only a hint. Refresh the authoritative source first;
        // useProjectPacksView refreshes native packs if that revision changed.
        void queryClient.invalidateQueries({
          queryKey: projectPackSourceQueryKey(projectRef),
        });
      } else {
        invalidatePacks();
      }
    };

    for (const filter of filters) {
      void client
        .subscribeLive(filter, onEvent)
        .then((unsubscribe) => {
          if (disposed) void unsubscribe();
          else disposers.push(unsubscribe);
        })
        .catch((error: unknown) => {
          if (disposed) return;
          watchFailed = true;
          setError(
            error instanceof Error
              ? `Live pack refresh is unavailable: ${error.message}. Reconnect or reopen this Packs tab to retry.`
              : "Live pack refresh is unavailable. Reconnect or reopen this Packs tab to retry.",
          );
        });
    }
    const unsubscribeReconnect = client.subscribeToReconnects(() => {
      if (disposed) return;
      invalidateSourceAndPacks();
      // A reconnect is the deterministic retry path promised by the error UI.
      // Keep successful watchers mounted so their replay dedupe survives.
      if (watchFailed) setRetryEpoch((value) => value + 1);
    });

    return () => {
      disposed = true;
      unsubscribeReconnect();
      for (const unsubscribe of disposers) void unsubscribe();
    };
  }, [
    client,
    filters,
    projectRef,
    queryClient,
    retryEpoch,
    sourceEventId,
    sourceRef,
    sourceRepoId,
  ]);
  const retry = React.useCallback(
    () => setRetryEpoch((value) => value + 1),
    [],
  );
  return React.useMemo(() => ({ error, retry }), [error, retry]);
}
