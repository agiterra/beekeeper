import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_SESSION } from "@/shared/constants/kinds";

import {
  rosterFromAnnounce,
  type RemoteTerminalRosterEntry,
} from "./useProjectTerminals";

/** The parsed head of one session's kind:30623 announce. */
export type ShellSessionAnnounce = {
  status: string | null;
  title: string | null;
  /** Invited members from the announce's arity-4 `p` tags. */
  roster: RemoteTerminalRosterEntry[];
  announcedAt: number;
};

function parseAnnounce(
  events: readonly RelayEvent[],
  sessionId: string,
): ShellSessionAnnounce | null {
  // Addressable head: the relay returns the latest, but never trust ordering.
  let head: RelayEvent | null = null;
  for (const event of events) {
    if (event.kind !== KIND_SHELL_SESSION) continue;
    if (event.tags.find((t) => t[0] === "d")?.[1] !== sessionId) continue;
    if (!head || event.created_at > head.created_at) head = event;
  }
  if (!head) return null;
  const chosen = head;
  const tagValue = (name: string) =>
    chosen.tags.find((t) => t[0] === name && typeof t[1] === "string")?.[1] ??
    null;
  return {
    status: tagValue("status"),
    title: tagValue("title"),
    roster: rosterFromAnnounce(chosen),
    announcedAt: chosen.created_at,
  };
}

const announceQueryKey = (ownerPubkey: string, sessionId: string) => [
  "shell-observe",
  "announce",
  ownerPubkey.toLowerCase(),
  sessionId,
];

/**
 * The live kind:30623 announce for one shared terminal — how an observer
 * learns (and keeps learning) its own role. Fetched directly by owner +
 * session id so roles survive deep links (no project list in scope), and
 * kept fresh (live subscription + short staleTime + refetch on focus) so a
 * revocation or role change lands quickly.
 */
export function useShellSessionAnnounce(
  ownerPubkey: string,
  sessionId: string,
) {
  const queryClient = useQueryClient();

  React.useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_SHELL_SESSION],
          authors: [ownerPubkey],
          "#d": [sessionId],
          since: Math.floor(Date.now() / 1_000),
          limit: 10,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: announceQueryKey(ownerPubkey, sessionId),
          });
        },
      )
      .then((handle) => {
        if (disposed) handle?.();
        else unsubscribe = handle ?? null;
      })
      .catch(() => {
        // Poll/refocus fallback still runs.
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [ownerPubkey, sessionId, queryClient]);

  return useQuery({
    queryKey: announceQueryKey(ownerPubkey, sessionId),
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    queryFn: async () => {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_SHELL_SESSION],
        authors: [ownerPubkey],
        "#d": [sessionId],
        limit: 10,
      });
      return parseAnnounce(events, sessionId);
    },
  });
}
