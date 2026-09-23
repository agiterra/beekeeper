import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { channelsQueryKey } from "@/features/channels/hooks";
import { useProjectsQuery, projectsQueryKey } from "@/features/projects/hooks";
import {
  projectContainersQueryKey,
  useProjectContainersQuery,
} from "@/features/projects-container/hooks";
import { relayClient } from "@/shared/api/relayClient";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import {
  createTrailingDebounce,
  type TrailingDebounce,
} from "@/shared/lib/trailingDebounce";

import {
  isWatchedProjectEvent,
  liveProjectFilters,
} from "./liveProjectFilters";

/**
 * A burst, not a beat: deleting a project publishes every channel, every
 * workflow, every terminal, the repositories and then the head, and each one
 * can land on this subscription. One refetch for the whole act.
 */
const PROJECT_INVALIDATE_DEBOUNCE_MS = 500;

/**
 * Keep this client's project list current with everybody else's.
 *
 * Mounted once, in `AppShell`, beside the other app-wide live subscription.
 * A hook rather than a module singleton on purpose: the community switch
 * remounts the whole subtree by key, so the effect's own cleanup closes the
 * REQ and nothing new is owed to `resetCommunityState()`.
 *
 * On any watched event it **invalidates rather than patching**. The two read
 * models already apply owner-signed tombstone thresholds
 * (`buildProjectReadModels`, `isProjectContainerDeleted`), and re-deriving
 * through them keeps one code path deciding what is deleted instead of two
 * that can disagree.
 *
 * `channelsQueryKey` goes with them because a cascade delete takes the
 * project's channels too, and those otherwise wait up to 60 s for their own
 * poll — long enough to watch a project vanish and its channels linger.
 */
export function useLiveProjectUpdates(): void {
  const queryClient = useQueryClient();
  const containersQuery = useProjectContainersQuery();
  const projectsQuery = useProjectsQuery();

  // Every coordinate this client knows, from both lists. The union matters:
  // the sidebar and the projects screen are fed by different queries, and a
  // project can be in one before the other.
  //
  // Sorted, then passed through `useStableArrayShallow`: both queries hand
  // back a fresh array on every refetch even when the contents are
  // identical, and without a content-equality cache that identity churn
  // would tear down and rebuild the REQ on each poll of an unrelated query.
  const watched = useStableArrayShallow(
    React.useMemo(() => {
      const all = new Set<string>();
      for (const container of containersQuery.data ?? []) {
        if (container.address) all.add(container.address);
      }
      for (const project of projectsQuery.data ?? []) {
        if (project.projectAddress) all.add(project.projectAddress);
      }
      return [...all].sort();
    }, [containersQuery.data, projectsQuery.data]),
  );

  const invalidateRef = React.useRef<TrailingDebounce | null>(null);
  if (invalidateRef.current === null) {
    invalidateRef.current = createTrailingDebounce(() => {
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
    }, PROJECT_INVALIDATE_DEBOUNCE_MS);
  }

  React.useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => Promise<void>) | null = null;

    const watchedSet = new Set(watched);
    void relayClient
      .subscribeLiveMany(liveProjectFilters(watched), (event) => {
        if (cancelled) return;
        if (!isWatchedProjectEvent(event, watchedSet)) return;
        invalidateRef.current?.trigger();
      })
      .then((dispose) => {
        if (cancelled) void dispose();
        else unsubscribe = dispose;
      })
      .catch((error: unknown) => {
        // A subscription that cannot start is not worth a toast: the lists
        // still refresh on reconnect, exactly as they did before this hook.
        console.warn("[liveProjectUpdates] subscribe failed:", error);
      });

    return () => {
      cancelled = true;
      void unsubscribe?.();
    };
  }, [watched]);
}
