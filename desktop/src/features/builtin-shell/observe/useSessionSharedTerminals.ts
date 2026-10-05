import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import { KIND_SHELL_SESSION } from "@/shared/constants/kinds";
import { phaseJitteredPeriodMs } from "@/shared/lib/pollSchedule";
import { useFocusedRefetchInterval } from "@/shared/lib/useDocumentVisible";

import {
  type RemoteTerminal,
  remoteTerminalsFromEvents,
  TERMINALS_REFETCH_INTERVAL_MS,
} from "./useProjectTerminals";

/**
 * The query key carries the community's relay URL: the QueryClient outlives
 * a community switch, and one community's terminals must never be shown
 * under another's until a refetch replaces them.
 */
export const sessionSharedTerminalsQueryKey = (
  relayUrl: string,
  projectAddress: string,
  sessionRef: string,
) => [
  "coding-session",
  "shared-terminals",
  relayUrl,
  projectAddress,
  sessionRef,
];

/**
 * How many of the project's announces one read asks for. A page that comes
 * back full may have cut off older announces, this session's among them, so
 * the read reports `truncated` and the surfaces say only these were checked.
 */
export const SESSION_SHARED_TERMINALS_READ_LIMIT = 100;

/** Whether a read of `received` announces under `limit` may be cut off. */
export function sessionSharedTerminalsReadTruncated(
  received: number,
  limit: number = SESSION_SHARED_TERMINALS_READ_LIMIT,
): boolean {
  return received >= limit;
}

/**
 * The open shared terminals of one coding session (SV-25, DB11): the
 * project's kind:30623 announces whose `session` tag names `sessionRef`.
 *
 * Relays index single-letter tags only, so the read is the project's
 * announces (`#a`) filtered here by the `session` tag. Every member's are
 * returned, the viewer's own included — a terminal the viewer shares from
 * another of their computers is still not on this one; the drawer drops the
 * ones this computer runs itself.
 *
 * One explicit `POST /query` (`fetchEventsBatch`), live-invalidated by the
 * project's announce stream, with the project terminals' 30 s backstop. The
 * read is bounded to the project's newest
 * `SESSION_SHARED_TERMINALS_READ_LIMIT` announces; when the page comes back
 * full, `truncated` says so rather than letting an empty list read as "none
 * shared".
 */
export function useSessionSharedTerminals(
  relayUrl: string,
  projectAddress: string | null,
  sessionRef: string,
): {
  terminals: RemoteTerminal[];
  state: "no-project" | "loading" | "ready" | "error";
  /** The project's announce page came back full: older ones were not read. */
  truncated: boolean;
  myPubkey: string | null;
} {
  const identity = useIdentityQuery();
  const queryClient = useQueryClient();
  const myPubkey = identity.data?.pubkey?.toLowerCase() ?? null;

  React.useEffect(() => {
    if (projectAddress === null) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_SHELL_SESSION],
          "#a": [projectAddress],
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: sessionSharedTerminalsQueryKey(
              relayUrl,
              projectAddress,
              sessionRef,
            ),
          });
        },
      )
      .then((handle) => {
        if (disposed) handle?.();
        else unsubscribe = handle ?? null;
      })
      .catch(() => {
        // The poll below still runs.
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [projectAddress, queryClient, relayUrl, sessionRef]);

  const refetchInterval = useFocusedRefetchInterval(
    phaseJitteredPeriodMs(
      "session-shared-terminals",
      TERMINALS_REFETCH_INTERVAL_MS,
      myPubkey ?? "",
    ),
  );
  const query = useQuery({
    queryKey: sessionSharedTerminalsQueryKey(
      relayUrl,
      projectAddress ?? "",
      sessionRef,
    ),
    enabled: projectAddress !== null,
    refetchInterval,
    queryFn: async () => {
      const address = projectAddress ?? "";
      const events = await relayClient.fetchEventsBatch([
        {
          kinds: [KIND_SHELL_SESSION],
          "#a": [address],
          limit: SESSION_SHARED_TERMINALS_READ_LIMIT,
        },
      ]);
      return {
        terminals: remoteTerminalsFromEvents(events, address, null).filter(
          (terminal) => terminal.sessionRef === sessionRef,
        ),
        truncated: sessionSharedTerminalsReadTruncated(events.length),
      };
    },
  });

  const terminals = query.data?.terminals ?? EMPTY;
  const truncated = query.data?.truncated ?? false;
  const state =
    projectAddress === null
      ? "no-project"
      : query.data !== undefined
        ? "ready"
        : query.isError
          ? "error"
          : "loading";
  return { terminals, state, truncated, myPubkey };
}

const EMPTY: RemoteTerminal[] = [];
