import { useRouter } from "@tanstack/react-router";
import * as React from "react";
import { toast } from "sonner";

import {
  CODING_SESSION_RESUME_STALL_MESSAGE,
  resolveCodingSessionResumeSettlement,
} from "@/features/coding-sessions/lib/codingSessionResumeSettle";
import { parseCodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { NEW_CODING_SESSION_STALL_MS } from "@/features/coding-sessions/lib/newCodingSessionModel";
import {
  type CodingSessionIngressClient,
  useCodingSessionLifecycleResolution,
} from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";

export type CodingSessionResumeSettleOptions = {
  channelId: string;
  providerAuthorityPubkey: string | null;
  /** Ingress transport seam; production passes nothing. */
  client?: CodingSessionIngressClient;
  /**
   * How a context-loss notice reaches the person. It defaults to a toast
   * because navigation remounts the workspace subtree — anything held in the
   * composer's own state would be destroyed by the very navigation that has
   * to report it.
   */
  notify?: (message: string) => void;
};

export type CodingSessionResumeSettleState = {
  /** Arm the wait for one published resume's receipt. */
  begin: (commandId: string) => void;
  /** Abandon the wait with a message — the publish itself was rejected. */
  fail: (message: string) => void;
  /** True from publish until the provider's receipt settles the command. */
  isPending: boolean;
  /** Refusal or stall copy, for the composer's existing error line. */
  error: string | null;
  /**
   * Render this next to the composer. It is `null` unless a resume is in
   * flight, so the relay subscription behind it exists only for the seconds
   * it is actually waited on (same shape as `useEndCodingSessionDialog`).
   */
  watcher: React.ReactNode;
};

type ResumeSettleOutcome =
  | { kind: "established" }
  | { kind: "failed"; message: string };

/**
 * Follow a published `session.resume` to the generation the provider minted.
 *
 * Provider-side resume works by minting generation N+1 — a new target, new
 * 44223 metadata, a new catalog record, and therefore a new `generationId`.
 * The workspace route pins the exact generation it addresses, so a screen that
 * publishes a resume and then only awaits the publish sits on the dead
 * generation N forever: its immutable metadata says `disconnected`, which is
 * why the banner never clears, and a second press earns a `STALE_GENERATION`
 * refusal that nobody was listening for.
 *
 * The receipt is the authority on what happened here, exactly as it is in the
 * create path (`useNewCodingSessionCreate`), and it is the only thing that
 * navigates.
 */
export function useCodingSessionResumeSettle({
  channelId,
  client,
  notify,
  providerAuthorityPubkey,
}: CodingSessionResumeSettleOptions): CodingSessionResumeSettleState {
  const [pendingCommandId, setPendingCommandId] = React.useState<string | null>(
    null,
  );
  const [error, setError] = React.useState<string | null>(null);

  const begin = React.useCallback((commandId: string) => {
    setError(null);
    setPendingCommandId(commandId);
  }, []);

  const fail = React.useCallback((message: string) => {
    setPendingCommandId(null);
    setError(message);
  }, []);

  const handleSettled = React.useCallback((outcome: ResumeSettleOutcome) => {
    setPendingCommandId(null);
    setError(outcome.kind === "failed" ? outcome.message : null);
  }, []);

  return {
    begin,
    error,
    fail,
    isPending: pendingCommandId !== null,
    watcher:
      pendingCommandId && providerAuthorityPubkey ? (
        <CodingSessionResumeSettleWatcher
          channelId={channelId}
          client={client}
          commandId={pendingCommandId}
          key={pendingCommandId}
          notify={notify}
          onSettled={handleSettled}
          providerAuthorityPubkey={providerAuthorityPubkey}
        />
      ) : null,
  };
}

/**
 * The in-flight half of a reconnect: one command's receipt, watched.
 *
 * A component rather than a plain hook so the trusted-ingress subscription is
 * mounted only while a resume is actually pending, and so a fresh command
 * always starts from a clean settled guard (it is keyed by `commandId`).
 */
function CodingSessionResumeSettleWatcher({
  channelId,
  client,
  commandId,
  notify,
  onSettled,
  providerAuthorityPubkey,
}: {
  channelId: string;
  client?: CodingSessionIngressClient;
  commandId: string;
  notify?: (message: string) => void;
  onSettled: (outcome: ResumeSettleOutcome) => void;
  providerAuthorityPubkey: string;
}) {
  // Every surface that renders a composer is inside the router; reading it
  // warn-free keeps this mountable in a bare unit render too.
  const router = useRouter({ warn: false });
  // Pinned, not config: a reconnect is addressed to the provider running this
  // session, which on a session someone else founded is not a provider this
  // machine is allowed to run. Reading the answer through the local allowlist
  // would neither subscribe for the refusal nor admit it, leaving the person
  // with a thirty-second silence in place of the provider's stated reason.
  const snapshot = useCodingSessionLifecycleResolution(
    channelId,
    commandId,
    providerAuthorityPubkey,
    client,
    "pinned",
  );
  const lifecycle = snapshot.lifecycle;
  const settlement = React.useMemo(
    () =>
      resolveCodingSessionResumeSettlement({
        channelId,
        lifecycle,
        providerAuthorityPubkey,
      }),
    [channelId, lifecycle, providerAuthorityPubkey],
  );
  // A duplicate receipt (relay replay, a reconnect's history refetch) resolves
  // to the same settlement; navigating twice would push a second history
  // entry for one reconnect.
  const settledRef = React.useRef(false);

  React.useEffect(() => {
    if (settledRef.current) return;
    if (settlement.kind === "pending") return;
    if (settlement.commandId !== commandId) return;
    settledRef.current = true;
    if (settlement.kind === "failed") {
      onSettled({ kind: "failed", message: settlement.message });
      return;
    }
    if (settlement.notice) {
      (notify ?? toast.warning)(settlement.notice);
    }
    void router?.navigate({
      to: "/coding-sessions/$channelId/$generationId",
      params: { channelId, generationId: settlement.generationId },
      // Carry the surface forward: a pop-out that reconnects lands on the new
      // generation as a pop-out rather than flipping into the app surface.
      search: (previous) => ({
        surface: parseCodingSessionSurface(previous.surface),
      }),
      // The dead generation is not somewhere Back should return to.
      replace: true,
    });
    onSettled({ kind: "established" });
  }, [channelId, commandId, notify, onSettled, router, settlement]);

  // Clock-free facts, clock-bound patience: the resolution never times out by
  // design, so the deadline lives here and hands the button back instead of
  // spinning against a provider that stopped answering.
  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      if (settledRef.current) return;
      settledRef.current = true;
      onSettled({
        kind: "failed",
        message: CODING_SESSION_RESUME_STALL_MESSAGE,
      });
    }, NEW_CODING_SESSION_STALL_MS);
    return () => window.clearTimeout(timer);
  }, [onSettled]);

  return null;
}
