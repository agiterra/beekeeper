/**
 * Reading a project's artifact pins (NIP-AR, kind 44251): one cold read of the
 * whole op log, kept fresh by a live `#a` subscription that invalidates it and
 * a jittered poll that catches what the subscription misses (a kind:5 on an
 * op, a reconnect).
 *
 * The pattern is the to-do read's (`project-todos/lib/todoQueries.ts`), down
 * to the pagination: a sidebar must see every surviving `pin.set`, not just
 * recent ones, so the read pages by `until` until a short page and reports
 * `truncated` rather than presenting a sidebar that silently lost a pin.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_ARTIFACT_PIN_OP } from "@/shared/constants/kinds";
import { phaseJitteredPeriodMs } from "@/shared/lib/pollSchedule";

import {
  type ProjectArtifactPinDigest,
  foldProjectArtifactPins,
} from "./artifactPinFold";

/** The relay pages at 1000; ask for the cap. */
export const PIN_PAGE_LIMIT = 1_000;
/** After this many full pages the read stops and says so. */
export const PIN_MAX_PAGES = 10;
/** Poll fallback period; the live subscription is the primary signal. */
export const PIN_REFETCH_INTERVAL_MS = 60_000;
/** The relay's ingest window; a peer may legally stamp this far in the past. */
export const RELAY_TIMESTAMP_DRIFT_SECS = 900;

export function artifactPinsQueryKey(coordinate: string) {
  return ["project-artifact-pins", coordinate] as const;
}

export type ArtifactPinsRead = {
  /** Every op event read, deduplicated by id. */
  events: RelayEvent[];
  digest: ProjectArtifactPinDigest;
  /** The read hit `PIN_MAX_PAGES` full pages and stopped; older ops are missing. */
  truncated: boolean;
  /** The greatest `created_at` seen per target, for the write bump. */
  latestByTarget: Record<string, number>;
};

/** The op filter for one coordinate. */
export function artifactPinOpsFilter(
  coordinate: string,
  extra: Partial<RelaySubscriptionFilter> = {},
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_PROJECT_ARTIFACT_PIN_OP],
    "#a": [coordinate],
    limit: PIN_PAGE_LIMIT,
    ...extra,
  };
}

/** Fold a set of op events into the read the UI consumes. */
export function pinsReadFromEvents(
  coordinate: string,
  repo: string,
  events: RelayEvent[],
  truncated: boolean,
): ArtifactPinsRead {
  const byId = new Map<string, RelayEvent>();
  for (const event of events) byId.set(event.id, event);
  const unique = [...byId.values()];
  const latestByTarget: Record<string, number> = {};
  for (const event of unique) {
    const target = event.tags.find((t) => t[0] === "ar-target")?.[1];
    if (!target) continue;
    latestByTarget[target] = Math.max(
      latestByTarget[target] ?? 0,
      event.created_at,
    );
  }
  return {
    events: unique,
    digest: foldProjectArtifactPins(coordinate, repo, unique),
    truncated,
    latestByTarget,
  };
}

/**
 * Read every pin op for `coordinate`, oldest page last. Stops after
 * `PIN_MAX_PAGES` full pages and reports it.
 */
export async function fetchArtifactPins(
  coordinate: string,
  repo: string,
): Promise<ArtifactPinsRead> {
  const events: RelayEvent[] = [];
  let until: number | undefined;
  let truncated = false;
  for (let page = 0; ; page++) {
    if (page >= PIN_MAX_PAGES) {
      truncated = true;
      break;
    }
    const batch = await relayClient.fetchEventsCoalesced(
      artifactPinOpsFilter(coordinate, until === undefined ? {} : { until }),
    );
    events.push(...batch);
    if (batch.length < PIN_PAGE_LIMIT) break;
    // `until` is inclusive and second-granular; dedupe by id handles the
    // overlap, and a whole page inside one second cannot advance the cursor,
    // which the cap turns into a disclosed stop rather than a spin.
    const oldest = Math.min(...batch.map((event) => event.created_at));
    if (until !== undefined && oldest >= until) {
      truncated = true;
      break;
    }
    until = oldest;
  }
  return pinsReadFromEvents(coordinate, repo, events, truncated);
}

export type ArtifactPinsState =
  | { kind: "loading"; read: ArtifactPinsRead | null }
  | { kind: "ready"; read: ArtifactPinsRead; refreshing: boolean }
  | { kind: "error"; read: ArtifactPinsRead | null; message: string };

/**
 * A project's pins, live. Pass `null` for either argument when the project has
 * no coordinate or no agents repository yet: nothing is read, because a pin
 * names a path in a repository and there is none to name.
 */
export function useArtifactPins(
  coordinate: string | null,
  repo: string | null,
): ArtifactPinsState {
  const queryClient = useQueryClient();
  const enabled = coordinate !== null && repo !== null;
  const key = React.useMemo(
    () => artifactPinsQueryKey(coordinate ?? "none"),
    [coordinate],
  );

  React.useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        artifactPinOpsFilter(coordinate, {
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
        // The poll below is the fallback; a subscription transport failure
        // does not manufacture a read result.
      });
    return () => {
      disposed = true;
      if (unsubscribe) void unsubscribe();
    };
  }, [coordinate, enabled, key, queryClient]);

  const identity = useIdentityQuery();
  const query = useQuery({
    queryKey: key,
    enabled,
    refetchInterval: phaseJitteredPeriodMs(
      `project-artifact-pins:${coordinate ?? "none"}`,
      PIN_REFETCH_INTERVAL_MS,
      identity.data?.pubkey,
    ),
    queryFn: () => fetchArtifactPins(coordinate ?? "", repo ?? ""),
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
