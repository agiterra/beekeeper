/**
 * The Packs tab's provenance answer, as one React Query.
 *
 * One query owns the whole thing — the four relay reads, the clock, and the
 * fold — because the answer is only sound if `now` is read *after* the last
 * read returned and the dispositions are computed from that one set of bytes.
 * Splitting the read from the model would let a render interleave a fresh
 * event set with a stale verdict, which is exactly the "upgraded on a stale
 * answer" failure the revision comparison already refuses.
 *
 * Two things keep the cached answer honest:
 *
 * - **The relay is in the key.** A proof is a fact about one relay's events,
 *   and the community switch remounts React without emptying the QueryClient.
 *   The relay-scoped key (plus the one-line reset in `resetCommunityState`)
 *   is what stops the previous community's `commissioned` from being served
 *   for the next community's rows.
 * - **Late proof invalidates the answer.** The 44221/44224/44226 a row needs
 *   often land *after* the 44223 that reported it — the Packs tab renders on
 *   a cold cache while replay is still arriving. Rather than poll, this hook
 *   listens on the coding-session observed-event bus the create-observation
 *   and trusted-ingress subscriptions already feed, and invalidates when a
 *   proof kind arrives in one of its own channels.
 *
 * No module-level state: React Query holds the cache and the bus subscription
 * is owned by the effect that made it.
 */

import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { subscribeToObservedCodingSessionEvents } from "@/features/coding-sessions/lib/codingSessionObservedEvents";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";

import {
  buildRolePackProvenance,
  type RolePackProvenanceResult,
  type RolePackProvenanceRow,
} from "./rolePackProvenance";
import {
  fetchRolePackProvenanceEvents,
  ROLE_PACK_PROVENANCE_QUERY_PREFIX,
  rolePackProvenanceQueryKey,
  type RolePackProvenanceFetcher,
} from "./rolePackProvenanceQuery";

/** How long one provenance answer stays fresh. */
const PROVENANCE_STALE_TIME_MS = 30_000;

const UNIT_SEPARATOR = "\u0000";

/**
 * The kinds whose arrival can change a verdict: the command, the receipt, the
 * genesis it binds to, and the report itself.
 */
const PROVENANCE_LIVE_KINDS = new Set<number>([
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_GENESIS,
]);

/** A stable array from an unstable one, without a new identity per render. */
function useStableSortedList(values: readonly (string | null)[]): string[] {
  const signature = [
    ...new Set(values.filter((value): value is string => Boolean(value))),
  ]
    .sort()
    .join(UNIT_SEPARATOR);
  return React.useMemo(
    () => (signature === "" ? [] : signature.split(UNIT_SEPARATOR)),
    [signature],
  );
}

function errorSentence(error: unknown): string | null {
  if (error === null || error === undefined) return null;
  return error instanceof Error ? error.message : String(error);
}

function channelOf(event: RelayEvent): string | null {
  for (const tag of event.tags) {
    if (tag[0] === "h" && typeof tag[1] === "string") return tag[1];
  }
  return null;
}

/** What the Packs tab knows about provenance right now. */
export type RolePackProvenanceQueryState = {
  /** Null until the first answer; never a partially-computed one. */
  result: RolePackProvenanceResult | null;
  isLoading: boolean;
  /** Readable, or null when the read succeeded. */
  error: string | null;
  refetch: () => void;
};

export function useRolePackProvenance(
  input: {
    /**
     * The active community's relay. Null while it is unresolved, and the query
     * stays disabled then — an answer with no relay to attribute it to could
     * only be cached under a key that lies about which events it read.
     */
    relayUrl: string | null;
    projectRef: string | null;
    channelIds: readonly string[];
    rows: readonly RolePackProvenanceRow[];
  },
  deps: { fetchEvents?: RolePackProvenanceFetcher } = {},
): RolePackProvenanceQueryState {
  const { relayUrl, projectRef, rows } = input;
  const fetchEvents = deps.fetchEvents;
  const channelIds = useStableSortedList(input.channelIds);
  const metadataEventIds = useStableSortedList(
    rows.map((row) => row.metadataEventId),
  );
  const enabled = relayUrl !== null && channelIds.length > 0 && rows.length > 0;
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: rolePackProvenanceQueryKey(
      relayUrl,
      projectRef,
      channelIds,
      metadataEventIds,
    ),
    queryFn: async (): Promise<RolePackProvenanceResult> => {
      const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
        channelIds,
        fetchEvents ? { fetchEvents } : {},
      );
      // Read once, after the last read returned: the clock the whole answer is
      // measured against.
      const now = Math.floor(Date.now() / 1_000);
      return buildRolePackProvenance({
        events,
        channelIds,
        now,
        sourceErrors,
        rows,
      });
    },
    enabled,
    staleTime: PROVENANCE_STALE_TIME_MS,
  });

  React.useEffect(() => {
    if (relayUrl === null || channelIds.length === 0) return;
    const scoped = new Set(channelIds);
    return subscribeToObservedCodingSessionEvents((events) => {
      const affectsThisProject = events.some((event) => {
        if (!PROVENANCE_LIVE_KINDS.has(event.kind)) return false;
        const channelId = channelOf(event);
        return channelId !== null && scoped.has(channelId);
      });
      if (!affectsThisProject) return;
      // A prefix match, not the exact key: the row set is part of the full key,
      // so the entry a newly arrived report belongs to is not the one currently
      // mounted until the rows change too.
      void queryClient.invalidateQueries({
        queryKey: [ROLE_PACK_PROVENANCE_QUERY_PREFIX, relayUrl, projectRef],
      });
    });
  }, [channelIds, projectRef, queryClient, relayUrl]);

  const queryRefetch = query.refetch;
  const refetch = React.useCallback(() => {
    void queryRefetch();
  }, [queryRefetch]);

  // A verdict is only as good as the read that produced it. The moment proof
  // changes (an invalidation above, a manual refetch, or a stale re-ask) the
  // previous answer stops being authoritative, and a read that failed leaves
  // nothing to stand on — so both windows report no result rather than the
  // old one. Rows fall back to "proof unavailable" with the read's own words;
  // a positive label never outlives the evidence it was drawn from.
  // `fetchStatus` rather than `isFetching`: a re-read that React Query has
  // paused (offline) is not fetching, yet has confirmed nothing either.
  const settled = query.fetchStatus === "idle" && !query.isError;
  return {
    result: settled ? (query.data ?? null) : null,
    isLoading: query.isLoading,
    error: errorSentence(query.error),
    refetch,
  };
}
