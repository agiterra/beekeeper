/**
 * The pinned to-do lists of every project in the sidebar, in one hook.
 *
 * One query per coordinate, keyed exactly as the To-Do tab keys its read
 * (`projectTodosQueryKey`), so the sidebar and the tab share a cache: a pin
 * toggled in the tab moves the sidebar row on the same render, and opening
 * a tab from the sidebar finds its read already warm. The cold reads are
 * coalesced by the relay client into one `POST /query`; live updates ride
 * one REQ per ten coordinates (the relay's filter cap).
 *
 * Personal lists need no client-side filtering here: the relay withholds
 * every other member's, so the only personal pins a reader ever sees are
 * their own.
 */
import * as React from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { MAX_FILTERS_PER_REQ } from "@/shared/api/relayClientShared";
import { useStableMap } from "@/shared/hooks/useStableReference";

import type { TodoList } from "./todoFold";
import {
  RELAY_TIMESTAMP_DRIFT_SECS,
  fetchProjectTodos,
  projectTodosQueryKey,
  todoOpsFilter,
} from "./todoQueries";

const NO_LISTS: readonly TodoList[] = [];

/**
 * Pinned, unarchived lists by project coordinate. `coordinates` should be
 * the addresses of real projects only (the local General placeholder has
 * none).
 */
export function usePinnedTodoListsIndex(
  coordinates: readonly string[],
): ReadonlyMap<string, readonly TodoList[]> {
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
            todoOpsFilter(coordinate, { since, limit: 100 }),
          ),
          (event) => {
            const coordinate = event.tags.find((t) => t[0] === "a")?.[1];
            if (!coordinate) return;
            void queryClient.invalidateQueries({
              queryKey: projectTodosQueryKey(coordinate),
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

  const results = useQueries({
    queries: stable.map((coordinate) => ({
      queryKey: projectTodosQueryKey(coordinate),
      queryFn: () => fetchProjectTodos(coordinate),
      staleTime: 30_000,
    })),
  });

  // Only the pinned rows matter to a caller. The map is rebuilt from the
  // reads each render; `useStableMap` hands back the previous instance
  // while its entries are the same lists, so the groups' memoized children
  // do not recompute on unrelated renders. Each entry is the digest's own
  // `lists` filtered, cached per digest so the array identity moves only
  // when a read lands.
  const map = new Map<string, readonly TodoList[]>();
  stable.forEach((coordinate, index) => {
    const digest = results[index]?.data?.digest ?? null;
    map.set(coordinate, digest ? pinnedOf(digest) : NO_LISTS);
  });
  return useStableMap(map);
}

const pinnedCache = new WeakMap<object, readonly TodoList[]>();

/** The pinned, unarchived lists of a digest, one array per digest instance. */
function pinnedOf(digest: { lists: TodoList[] }): readonly TodoList[] {
  const cached = pinnedCache.get(digest);
  if (cached) return cached;
  const pinned = digest.lists.filter((list) => list.pinned && !list.archived);
  const value = pinned.length > 0 ? pinned : NO_LISTS;
  pinnedCache.set(digest, value);
  return value;
}
