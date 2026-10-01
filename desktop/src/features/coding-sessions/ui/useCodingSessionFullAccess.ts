import { useQuery, useQueryClient } from "@tanstack/react-query";
import type * as React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { useCodingSessionResumeSettle } from "@/features/coding-sessions/hooks/useCodingSessionResumeSettle";
import { codingSessionFullAccessFailure } from "@/features/coding-sessions/lib/codingSessionFullAccess";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionRestart,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { buildCodingSessionResumeInput } from "@/features/coding-sessions/lib/codingSessionResumeSeat";
import { codingSessionResumeSeatDeps } from "@/features/coding-sessions/lib/codingSessionResumeSeatDeps";
import { publishSeatedCodingSessionResume } from "@/features/coding-sessions/lib/codingSessionSeatedCreate";
import type { CodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  getCodingSessionFullAccess,
  setCodingSessionFullAccess,
} from "@/shared/api/tauriCodingSessionFullAccess";

/** The execution facts a full-access change reads and restarts from. */
export type CodingSessionFullAccessRecord = Pick<
  CodingSessionCatalogRecord,
  | "agentRef"
  | "commandTarget"
  | "projectRef"
  | "providerAuthorityPubkey"
  | "role"
>;

/** One local session's grant, and the control that changes it. */
export type CodingSessionFullAccess = {
  /** The host's last answer. Never set from the click — always re-read. */
  granted: boolean;
  /** The value being put in force, or null when nothing is changing. */
  pending: boolean | null;
  error: string | null;
  toggle: () => void;
  /**
   * The restart's receipt watcher. Mount it somewhere that stays mounted
   * while the header does; it renders no DOM and is null when idle.
   */
  watcher: React.ReactNode;
};

/**
 * Read and change one execution's full-access grant on this computer.
 *
 * Returns `null` — show nothing — unless the host answered for this exact
 * session: no provider or command target yet, a read in flight, a read that
 * failed, or a provider that is not this computer's (`null` from the host).
 *
 * A change is two steps: write the grant, then restart the execution so its
 * next generation starts under it (`session.restart`, which the provider
 * refuses with `SESSION_BUSY` while a turn is open). A seated execution
 * restages its seat under the restart's command id exactly as the
 * project-agent restart does; an unseated one publishes the restart alone
 * (`publishSeatedCodingSessionResume` with no actor is just the publish). If
 * the restart does not take — refused, rejected, or unanswered — the grant
 * is put back, so the badge never says one thing while the running agent
 * does another, and the provider's own words are shown.
 */
export function useCodingSessionFullAccess({
  channelId,
  record,
}: {
  channelId: string;
  record: CodingSessionFullAccessRecord | null;
}): CodingSessionFullAccess | null {
  const providerPubkey = record?.providerAuthorityPubkey ?? null;
  const target = record?.commandTarget ?? null;
  const sessionId = target?.sessionId ?? null;
  const queryClient = useQueryClient();
  const read = useQuery({
    queryKey: ["coding-session-full-access", providerPubkey, sessionId],
    enabled: providerPubkey !== null && sessionId !== null,
    staleTime: 30_000,
    queryFn: () => {
      if (!providerPubkey || !sessionId) return null;
      return getCodingSessionFullAccess({ providerPubkey, sessionId });
    },
  });
  const settle = useCodingSessionResumeSettle({
    channelId,
    providerAuthorityPubkey: providerPubkey,
  });
  const [change, setChange] = useState<{
    granted: boolean;
    commandId: string | null;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const recordRef = useRef(record);
  recordRef.current = record;

  const refetch = useCallback(
    () =>
      queryClient.invalidateQueries({
        queryKey: ["coding-session-full-access", providerPubkey, sessionId],
      }),
    [providerPubkey, queryClient, sessionId],
  );

  const failChange = useCallback(
    async (granted: boolean, cause: string) => {
      let revertError: string | null = null;
      if (providerPubkey && sessionId) {
        try {
          await setCodingSessionFullAccess({
            providerPubkey,
            sessionId,
            granted: !granted,
          });
        } catch (revert) {
          revertError = errorText(revert);
        }
      }
      const message = codingSessionFullAccessFailure({
        granted,
        cause,
        revertError,
      });
      setChange(null);
      setError(message);
      toast.error(message);
      void refetch();
    },
    [providerPubkey, refetch, sessionId],
  );

  // The restart's receipt settles the change: established keeps the grant,
  // anything else puts it back.
  const { error: settleError, isPending: settlePending } = settle;
  useEffect(() => {
    if (!change?.commandId || settlePending) return;
    if (settleError) {
      // Still pending to the reader, but no longer armed: the revert runs
      // exactly once however often this effect re-runs while it awaits.
      setChange({ granted: change.granted, commandId: null });
      void failChange(change.granted, settleError);
      return;
    }
    setChange(null);
    void refetch();
  }, [change, failChange, refetch, settleError, settlePending]);

  const { begin, fail } = settle;
  const granted = read.data ?? null;
  const toggle = useCallback(() => {
    const current = recordRef.current;
    const currentTarget = current?.commandTarget ?? null;
    if (
      granted === null ||
      change !== null ||
      !current ||
      !currentTarget ||
      !providerPubkey
    ) {
      return;
    }
    const next = !granted;
    setError(null);
    setChange({ granted: next, commandId: null });
    void (async () => {
      try {
        await setCodingSessionFullAccess({
          providerPubkey,
          sessionId: currentTarget.sessionId,
          granted: next,
        });
      } catch (writeError) {
        // Nothing was written, so there is nothing to put back.
        const message = errorText(writeError);
        setChange(null);
        setError(message);
        toast.error(message);
        void refetch();
        return;
      }
      void refetch();
      const commandId = createCodingSessionLifecycleCommandId();
      // Armed before it is recorded, so the settle effect can never see this
      // command id with no wait behind it and read that as success.
      begin(commandId);
      setChange({ granted: next, commandId });
      try {
        await publishSeatedCodingSessionResume(
          buildCodingSessionResumeInput({
            commandId,
            seat: {
              actorPubkey: current.agentRef,
              role: current.role,
              projectRef: current.projectRef,
              sessionId: currentTarget.sessionId,
            },
            deps: codingSessionResumeSeatDeps,
            publish: () =>
              publishCodingSessionRestart({
                channelId,
                commandId,
                target: currentTarget,
                providerAuthorityPubkey: providerPubkey,
              }),
          }),
        );
      } catch (publishError) {
        // Routed through the settle state so the effect above reverts once.
        fail(errorText(publishError));
      }
    })();
  }, [begin, change, channelId, fail, granted, providerPubkey, refetch]);

  if (granted === null) return null;
  return {
    granted,
    pending: change?.granted ?? null,
    error,
    toggle,
    watcher: settle.watcher,
  };
}

function errorText(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return "the change did not complete";
}
