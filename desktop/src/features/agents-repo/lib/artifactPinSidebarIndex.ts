/**
 * The pinned artifacts of every project in the sidebar, in one hook.
 *
 * One query per coordinate, keyed exactly as the Artifacts tab keys its read
 * (`artifactPinsQueryKey`), so the sidebar and the tab share a cache: a pin
 * toggled in the tab moves the sidebar row on the same render, and opening the
 * tab from the sidebar finds its read already warm. The cold reads are
 * coalesced by the relay client into one `POST /query`; live updates ride one
 * REQ per ten coordinates (the relay's filter cap).
 *
 * Unlike the to-do index this also needs each project's **agents repository**,
 * because a pin names a path in a repository and the fold reports a pin for
 * any other as `otherRepo` rather than aiming it at the current one. That is
 * one more small, long-cached query per project (kind:30624), and a project
 * whose source has not loaded contributes no rows rather than rows folded
 * against a guess.
 */
import * as React from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { MAX_FILTERS_PER_REQ } from "@/shared/api/relayClientShared";
import { useStableMap } from "@/shared/hooks/useStableReference";

import {
  fetchProjectPackSource,
  projectPackSourceQueryKey,
} from "@/features/projects-container/lib/projectPackSource";

import type { PinRow } from "./artifactPinFold";
import {
  RELAY_TIMESTAMP_DRIFT_SECS,
  artifactPinOpsFilter,
  artifactPinsQueryKey,
  fetchArtifactPins,
} from "./artifactPinQueries";

const NO_PINS: readonly PinRow[] = [];

/**
 * Pinned artifacts by project coordinate, in sidebar order. `coordinates`
 * should be the addresses of real projects only (the local General
 * placeholder has none).
 */
export function usePinnedArtifactsIndex(
  coordinates: readonly string[],
): ReadonlyMap<string, readonly PinRow[]> {
  const queryClient = useQueryClient();
  const key = [...coordinates].sort().join(",");
  const stable = React.useMemo(() => (key === "" ? [] : key.split(",")), [key]);

  React.useEffect(() => {
    if (stable.length === 0) return;
    let disposed = false;
    const unsubscribes = new Set<() => Promise<void>>();
    const since = Math.floor(Date.now() / 1_000) - RELAY_TIMESTAMP_DRIFT_SECS;
    for (let at = 0; at < stable.length; at += MAX_FILTERS_PER_REQ) {
      const chunk = stable.slice(at, at + MAX_FILTERS_PER_REQ);
      void relayClient
        .subscribeLiveMany(
          chunk.map((coordinate) =>
            artifactPinOpsFilter(coordinate, { since, limit: 100 }),
          ),
          (event) => {
            const coordinate = event.tags.find((t) => t[0] === "a")?.[1];
            if (!coordinate) return;
            void queryClient.invalidateQueries({
              queryKey: artifactPinsQueryKey(coordinate),
            });
          },
        )
        .then((handle) => {
          if (!handle) return;
          if (disposed) void handle();
          else unsubscribes.add(handle);
        })
        .catch(() => {
          // The tab's poll is the fallback for a coordinate the user opens;
          // a sidebar row that lags is disclosed by the tab, not invented.
        });
    }
    return () => {
      disposed = true;
      for (const unsubscribe of unsubscribes) void unsubscribe();
      unsubscribes.clear();
    };
  }, [queryClient, stable]);

  const sources = useQueries({
    queries: stable.map((coordinate) => ({
      queryKey: projectPackSourceQueryKey(coordinate),
      queryFn: () => fetchProjectPackSource(coordinate),
      // The repository a project pins changes about never; the pin log is
      // what moves, and that has its own subscription.
      staleTime: 5 * 60_000,
    })),
  });
  const repos = stable.map((_, index) => sources[index]?.data?.repo ?? null);

  const results = useQueries({
    queries: stable.map((coordinate, index) => ({
      queryKey: artifactPinsQueryKey(coordinate),
      queryFn: () => fetchArtifactPins(coordinate, repos[index] ?? ""),
      enabled: repos[index] !== null,
      staleTime: 30_000,
    })),
  });

  // Only the pinned rows matter to a caller. The map is rebuilt from the
  // reads each render; `useStableMap` hands back the previous instance while
  // its entries are the same rows, so the groups' memoized children do not
  // recompute on unrelated renders. Each entry is the digest's own `pins`
  // filtered, cached per digest so the array identity moves only when a read
  // lands.
  const map = new Map<string, readonly PinRow[]>();
  stable.forEach((coordinate, index) => {
    const digest = results[index]?.data?.digest ?? null;
    map.set(coordinate, digest ? pinnedOf(digest) : NO_PINS);
  });
  return useStableMap(map);
}

const pinnedCache = new WeakMap<object, readonly PinRow[]>();

/** The pinned rows of a digest, one array per digest instance. */
function pinnedOf(digest: { pins: PinRow[] }): readonly PinRow[] {
  const cached = pinnedCache.get(digest);
  if (cached) return cached;
  const pinned = digest.pins.filter((row) => row.pinned);
  const value = pinned.length > 0 ? pinned : NO_PINS;
  pinnedCache.set(digest, value);
  return value;
}
