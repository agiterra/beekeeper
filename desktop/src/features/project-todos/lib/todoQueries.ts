/**
 * Reading a project's to-do ops: one cold read of the whole op log, kept
 * fresh by a live `#a` subscription that invalidates it and a jittered poll
 * that catches what the subscription misses (a kind:5 on an op, a reconnect).
 *
 * The pattern is Project Pulse's (`project-pulse/lib/pulseQueries.ts`). What
 * differs is honesty about the cold read: a to-do list must see every
 * surviving `item.add`, not just recent claims, so the read pages by `until`
 * until a short page and reports `truncated` when it gave up rather than
 * presenting a list that silently lost history.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_TODO_OP } from "@/shared/constants/kinds";
import { phaseJitteredPeriodMs } from "@/shared/lib/pollSchedule";

import { type ProjectTodoDigest, foldProjectTodos } from "./todoFold";

/** The relay pages at 1000; ask for the cap so a list needs as few round trips as possible. */
export const TODO_PAGE_LIMIT = 1_000;
/** After this many full pages the read stops and says so. */
export const TODO_MAX_PAGES = 10;
/** Poll fallback period; the live subscription is the primary signal. */
export const TODO_REFETCH_INTERVAL_MS = 60_000;
/** The relay's ingest window; a peer may legally stamp this far in the past. */
export const RELAY_TIMESTAMP_DRIFT_SECS = 900;

export function projectTodosQueryKey(coordinate: string) {
  return ["project-todos", coordinate] as const;
}

export type ProjectTodosRead = {
  /** Every op event read, deduplicated by id. */
  events: RelayEvent[];
  digest: ProjectTodoDigest;
  /** The read hit `TODO_MAX_PAGES` full pages and stopped; older ops are missing. */
  truncated: boolean;
  /** The greatest `created_at` seen per `(listId, itemId)` target (`""` = the list). */
  latestByTarget: Record<string, number>;
  /**
   * Ops written before the contract carried `td-vis` (a build older than
   * this one). They are among the digest's `ignored`; counted apart so the
   * notice can say "older format" rather than "malformed".
   */
  legacy: number;
};

/** The op filter for one coordinate. */
export function todoOpsFilter(
  coordinate: string,
  extra: Partial<RelaySubscriptionFilter> = {},
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_PROJECT_TODO_OP],
    "#a": [coordinate],
    limit: TODO_PAGE_LIMIT,
    ...extra,
  };
}

/** Key for `latestByTarget`. */
export function todoTargetKey(listId: string, itemId: string | null): string {
  return `${listId}/${itemId ?? ""}`;
}

/** Fold a set of op events into the read the UI consumes. */
export function readFromEvents(
  coordinate: string,
  events: RelayEvent[],
  truncated: boolean,
): ProjectTodosRead {
  const byId = new Map<string, RelayEvent>();
  for (const event of events) byId.set(event.id, event);
  const unique = [...byId.values()];
  const latestByTarget: Record<string, number> = {};
  let legacy = 0;
  for (const event of unique) {
    if (!event.tags.some((t) => t[0] === "td-vis")) legacy++;
    const listId = event.tags.find((t) => t[0] === "td-list")?.[1];
    if (!listId) continue;
    const itemId = event.tags.find((t) => t[0] === "td-item")?.[1] ?? null;
    for (const key of [
      todoTargetKey(listId, itemId),
      todoTargetKey(listId, null),
    ]) {
      latestByTarget[key] = Math.max(
        latestByTarget[key] ?? 0,
        event.created_at,
      );
    }
  }
  return {
    events: unique,
    digest: foldProjectTodos(coordinate, unique),
    truncated,
    latestByTarget,
    legacy,
  };
}

/**
 * Read every op for `coordinate`, oldest page last. Stops after
 * `TODO_MAX_PAGES` full pages and reports it.
 */
export async function fetchProjectTodos(
  coordinate: string,
): Promise<ProjectTodosRead> {
  const events: RelayEvent[] = [];
  let until: number | undefined;
  let truncated = false;
  for (let page = 0; ; page++) {
    if (page >= TODO_MAX_PAGES) {
      truncated = true;
      break;
    }
    const batch = await relayClient.fetchEventsCoalesced(
      todoOpsFilter(coordinate, until === undefined ? {} : { until }),
    );
    events.push(...batch);
    if (batch.length < TODO_PAGE_LIMIT) break;
    // `until` is inclusive and second-granular; dedupe by id handles the
    // overlap, and a whole page inside one second cannot advance the cursor,
    // which the truncation cap turns into a disclosed stop, not a spin.
    const oldest = Math.min(...batch.map((event) => event.created_at));
    if (until !== undefined && oldest >= until) {
      truncated = true;
      break;
    }
    until = oldest;
  }
  return readFromEvents(coordinate, events, truncated);
}

export type ProjectTodosState =
  | { kind: "loading"; read: ProjectTodosRead | null }
  | { kind: "ready"; read: ProjectTodosRead; refreshing: boolean }
  | { kind: "error"; read: ProjectTodosRead | null; message: string };

/**
 * A project's to-do lists, live. Pass `null` for a project with no coordinate
 * (the local General placeholder): nothing is read.
 */
export function useProjectTodos(coordinate: string | null): ProjectTodosState {
  const queryClient = useQueryClient();
  const key = React.useMemo(
    () => projectTodosQueryKey(coordinate ?? "none"),
    [coordinate],
  );

  React.useEffect(() => {
    if (coordinate === null) return;
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        todoOpsFilter(coordinate, {
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
  }, [coordinate, key, queryClient]);

  const identity = useIdentityQuery();
  const query = useQuery({
    queryKey: key,
    enabled: coordinate !== null,
    refetchInterval: phaseJitteredPeriodMs(
      `project-todos:${coordinate ?? "none"}`,
      TODO_REFETCH_INTERVAL_MS,
      identity.data?.pubkey,
    ),
    queryFn: () => fetchProjectTodos(coordinate ?? ""),
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
