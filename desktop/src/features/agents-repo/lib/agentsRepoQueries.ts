/**
 * Reading the Files tab's two sources and saying which is which:
 *
 * - **`main`**, through the host (`agents_repo_ls`, `agents_repo_read`):
 *   the packs cache's fetched tip, never its working copy;
 * - **the draft log** (kind 44250 by `#a`): one cold read of the whole log,
 *   kept fresh by a live subscription and a jittered poll, folded with the
 *   same fold `bee agents-repo` and Mobile bind to.
 *
 * A kind:30618 subscription on the repository invalidates the `main` reads
 * when a push lands — the committer's, a seat's, anyone's.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  fetchProjectPackSource,
  projectPackSourceQueryKey,
  projectPackSourceRepoId,
  type ProjectPackSource,
} from "@/features/projects-container/lib/projectPackSource";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { agentsRepoLs, agentsRepoRead } from "@/shared/api/tauriAgentsRepo";
import type {
  AgentsRepoFile,
  AgentsRepoListing,
} from "@/shared/api/agentsRepoTypes";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_AGENTS_REPO_DRAFT_OP,
  KIND_REPO_STATE,
} from "@/shared/constants/kinds";
import { phaseJitteredPeriodMs } from "@/shared/lib/pollSchedule";

import {
  type AgentsRepoDraftDigest,
  foldAgentsRepoDrafts,
} from "./agentsRepoDraftFold";

export const DRAFT_PAGE_LIMIT = 1_000;
export const DRAFT_MAX_PAGES = 10;
export const DRAFT_REFETCH_INTERVAL_MS = 60_000;
export const RELAY_TIMESTAMP_DRIFT_SECS = 900;

export function agentsRepoDraftsQueryKey(coordinate: string) {
  return ["agents-repo-drafts", coordinate] as const;
}
export function agentsRepoListingQueryKey(coordinate: string) {
  return ["agents-repo-listing", coordinate] as const;
}
export function agentsRepoFileQueryKey(coordinate: string, path: string) {
  return ["agents-repo-file", coordinate, path] as const;
}

export type AgentsRepoDraftsRead = {
  events: RelayEvent[];
  digest: AgentsRepoDraftDigest;
  truncated: boolean;
  /** Greatest `created_at` per path (`""` = commit records), for the write bump. */
  latestByPath: Record<string, number>;
};

export function draftOpsFilter(
  coordinate: string,
  extra: Partial<RelaySubscriptionFilter> = {},
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_AGENTS_REPO_DRAFT_OP],
    "#a": [coordinate],
    limit: DRAFT_PAGE_LIMIT,
    ...extra,
  };
}

export function draftsReadFromEvents(
  coordinate: string,
  repo: string,
  events: RelayEvent[],
  truncated: boolean,
): AgentsRepoDraftsRead {
  const byId = new Map<string, RelayEvent>();
  for (const event of events) byId.set(event.id, event);
  const unique = [...byId.values()];
  const latestByPath: Record<string, number> = {};
  for (const event of unique) {
    const paths = event.tags
      .filter((t) => t[0] === "ad-path")
      .map((t) => t[1] ?? "");
    for (const path of paths.length > 0 ? paths : [""]) {
      latestByPath[path] = Math.max(latestByPath[path] ?? 0, event.created_at);
    }
  }
  return {
    events: unique,
    digest: foldAgentsRepoDrafts(coordinate, repo, unique),
    truncated,
    latestByPath,
  };
}

export async function fetchAgentsRepoDrafts(
  coordinate: string,
  repo: string,
): Promise<AgentsRepoDraftsRead> {
  const events: RelayEvent[] = [];
  let until: number | undefined;
  let truncated = false;
  for (let page = 0; ; page++) {
    if (page >= DRAFT_MAX_PAGES) {
      truncated = true;
      break;
    }
    const batch = await relayClient.fetchEventsCoalesced(
      draftOpsFilter(coordinate, until === undefined ? {} : { until }),
    );
    events.push(...batch);
    if (batch.length < DRAFT_PAGE_LIMIT) break;
    const oldest = Math.min(...batch.map((event) => event.created_at));
    if (until !== undefined && oldest >= until) {
      truncated = true;
      break;
    }
    until = oldest;
  }
  return draftsReadFromEvents(coordinate, repo, events, truncated);
}

/** The project's newest kind:30624, shared with the Roles tab's query. */
export function useAgentsRepoSource(coordinate: string | null) {
  return useQuery({
    enabled: coordinate !== null,
    queryKey: projectPackSourceQueryKey(coordinate ?? ""),
    queryFn: () =>
      coordinate === null
        ? Promise.resolve(null)
        : fetchProjectPackSource(coordinate),
    staleTime: 30_000,
  });
}

export type AgentsRepoDraftsState =
  | { kind: "loading"; read: AgentsRepoDraftsRead | null }
  | { kind: "ready"; read: AgentsRepoDraftsRead; refreshing: boolean }
  | { kind: "error"; read: AgentsRepoDraftsRead | null; message: string };

/** The project's open drafts, live. */
export function useAgentsRepoDrafts(
  coordinate: string | null,
  repo: string | null,
): AgentsRepoDraftsState {
  const queryClient = useQueryClient();
  const key = React.useMemo(
    () => agentsRepoDraftsQueryKey(coordinate ?? "none"),
    [coordinate],
  );

  React.useEffect(() => {
    if (coordinate === null) return;
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        draftOpsFilter(coordinate, {
          since: Math.floor(Date.now() / 1_000) - RELAY_TIMESTAMP_DRIFT_SECS,
          limit: 100,
        }),
        () => {
          void queryClient.invalidateQueries({ queryKey: key });
        },
      )
      .then((handle) => {
        if (!handle) return;
        if (disposed) void handle();
        else unsubscribe = handle;
      })
      .catch(() => {
        // The poll below is the fallback.
      });
    return () => {
      disposed = true;
      if (unsubscribe) void unsubscribe();
    };
  }, [coordinate, key, queryClient]);

  const identity = useIdentityQuery();
  const query = useQuery({
    queryKey: key,
    enabled: coordinate !== null && repo !== null,
    refetchInterval: phaseJitteredPeriodMs(
      `agents-repo-drafts:${coordinate ?? "none"}`,
      DRAFT_REFETCH_INTERVAL_MS,
      identity.data?.pubkey,
    ),
    queryFn: () => fetchAgentsRepoDrafts(coordinate ?? "", repo ?? ""),
  });

  if (query.isError) {
    return {
      kind: "error",
      read: query.data ?? null,
      message:
        query.error instanceof Error
          ? query.error.message
          : String(query.error),
    };
  }
  if (!query.data) return { kind: "loading", read: null };
  return { kind: "ready", read: query.data, refreshing: query.isFetching };
}

/** The listing of `main`, through the host. */
export function useAgentsRepoListing(
  coordinate: string | null,
  enabled: boolean,
) {
  return useQuery<AgentsRepoListing>({
    enabled: coordinate !== null && enabled,
    queryKey: agentsRepoListingQueryKey(coordinate ?? ""),
    queryFn: () => agentsRepoLs(coordinate ?? "", true),
    staleTime: 30_000,
    retry: false,
  });
}

/** One file at `main`'s tip, through the host. */
export function useAgentsRepoFile(
  coordinate: string | null,
  path: string | null,
) {
  return useQuery<AgentsRepoFile>({
    enabled: coordinate !== null && path !== null,
    queryKey: agentsRepoFileQueryKey(coordinate ?? "", path ?? ""),
    queryFn: () => agentsRepoRead(coordinate ?? "", path ?? "", false),
    staleTime: 30_000,
    retry: false,
  });
}

/**
 * Invalidate every `main` read when the repository's ref state moves: a
 * kind:30618 by `#d` = repository id, relay-signed on every push.
 */
export function useAgentsRepoLiveInvalidation(
  coordinate: string | null,
  source: ProjectPackSource | null,
) {
  const queryClient = useQueryClient();
  const repoId = source ? projectPackSourceRepoId(source) : null;
  React.useEffect(() => {
    if (coordinate === null || repoId === null) return;
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        { kinds: [KIND_REPO_STATE], "#d": [repoId], limit: 1 },
        () => {
          if (disposed) return;
          void queryClient.invalidateQueries({
            queryKey: agentsRepoListingQueryKey(coordinate),
          });
          void queryClient.invalidateQueries({
            queryKey: ["agents-repo-file", coordinate],
          });
        },
      )
      .then((handle) => {
        if (!handle) return;
        if (disposed) void handle();
        else unsubscribe = handle;
      })
      .catch(() => {
        // The listing's staleTime and the person's Refresh are the fallback.
      });
    return () => {
      disposed = true;
      if (unsubscribe) void unsubscribe();
    };
  }, [coordinate, repoId, queryClient]);
}
