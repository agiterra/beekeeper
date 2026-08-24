import * as React from "react";

import type { CodingSessionCommandRefusal } from "@/features/coding-sessions/lib/codingSessionTrustedIngress";
import {
  CODING_SESSION_TURN_REFUSAL_DEADLINE_MS,
  forgetCodingSessionTurn,
  formatCodingSessionTurnRefusal,
  type WatchedCodingSessionTurn,
  watchCodingSessionTurn,
} from "@/features/coding-sessions/lib/codingSessionTurnRefusal";
import {
  type CodingSessionIngressClient,
  useCodingSessionLifecycleResolution,
} from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";

export type CodingSessionTurnRefusalOptions = {
  channelId: string;
  providerAuthorityPubkey: string | null;
  /**
   * Put the refused words back where the person can edit and resend them. The
   * composer clears its editor on send, so without this a refusal costs them
   * the message they wrote. `commandId` identifies the turn that was refused,
   * so the composer can also retire the optimistic row it is still showing for
   * it — a refused turn has no echo coming.
   */
  restoreDraft: (text: string, commandId: string) => void;
  /** Ingress transport seam; production passes nothing. */
  client?: CodingSessionIngressClient;
};

export type CodingSessionTurnRefusalState = {
  /** Watch one published turn for the provider's refusal. */
  watch: (turn: WatchedCodingSessionTurn) => void;
  /** Refusal copy for the composer's existing error line. */
  error: string | null;
  /**
   * Render this next to the composer. It is `null` while no sent turn is being
   * watched, so the relay subscriptions behind it exist only for the seconds a
   * refusal could still arrive — and, as with the resume watcher, so a
   * composer rendered in a bare unit test mounts no ingress at all.
   */
  watcher: React.ReactNode;
};

/**
 * Hear the provider say no to a turn.
 *
 * The composer publishes a signed 44220 and, today, treats the relay's
 * acceptance as the end of the story. The provider disagrees: an operator who
 * is neither the session founder nor a granted operator is refused with a
 * `failed` 44224 keyed to that exact command id. Watching for it is the whole
 * difference between "the agent is thinking" and "you were not allowed to say
 * that" — a distinction the multi-member case makes constantly.
 *
 * Success and irrelevance are both silent by design (see
 * `codingSessionTurnRefusal.ts`), so this waits briefly and then stops
 * watching without a word rather than inventing an outcome.
 */
export function useCodingSessionTurnRefusal({
  channelId,
  client,
  providerAuthorityPubkey,
  restoreDraft,
}: CodingSessionTurnRefusalOptions): CodingSessionTurnRefusalState {
  const [watched, setWatched] = React.useState<
    readonly WatchedCodingSessionTurn[]
  >([]);
  const [error, setError] = React.useState<string | null>(null);

  const watch = React.useCallback((turn: WatchedCodingSessionTurn) => {
    // A new turn is the person's answer to the last refusal; clear it.
    setError(null);
    setWatched((current) => watchCodingSessionTurn(current, turn));
  }, []);

  const handleRefused = React.useCallback(
    (turn: WatchedCodingSessionTurn, refusal: CodingSessionCommandRefusal) => {
      setWatched((current) => forgetCodingSessionTurn(current, turn.commandId));
      setError(formatCodingSessionTurnRefusal(refusal));
      restoreDraft(turn.draft, turn.commandId);
    },
    [restoreDraft],
  );

  const handleExpired = React.useCallback((commandId: string) => {
    setWatched((current) => forgetCodingSessionTurn(current, commandId));
  }, []);

  return {
    error,
    watch,
    watcher:
      providerAuthorityPubkey && watched.length > 0
        ? watched.map((turn) => (
            <CodingSessionTurnRefusalWatcher
              channelId={channelId}
              client={client}
              key={turn.commandId}
              onExpired={handleExpired}
              onRefused={handleRefused}
              providerAuthorityPubkey={providerAuthorityPubkey}
              turn={turn}
            />
          ))
        : null,
  };
}

/**
 * The in-flight half of one sent turn: its command id, watched for a refusal.
 *
 * A component rather than a plain hook for the same two reasons the resume
 * watcher is one — the trusted-ingress subscription is mounted only while a
 * turn could still be refused, and being keyed by `commandId` gives every turn
 * a clean settled guard of its own.
 */
function CodingSessionTurnRefusalWatcher({
  channelId,
  client,
  onExpired,
  onRefused,
  providerAuthorityPubkey,
  turn,
}: {
  channelId: string;
  client?: CodingSessionIngressClient;
  onExpired: (commandId: string) => void;
  onRefused: (
    turn: WatchedCodingSessionTurn,
    refusal: CodingSessionCommandRefusal,
  ) => void;
  providerAuthorityPubkey: string;
  turn: WatchedCodingSessionTurn;
}) {
  // Pinned to the provider this turn was addressed to. The refusal a member
  // most needs to hear — "you are not this session's founder" — is signed by a
  // provider that member's machine does not run, so the local allowlist is
  // exactly the wrong question to ask about it.
  const snapshot = useCodingSessionLifecycleResolution(
    channelId,
    turn.commandId,
    providerAuthorityPubkey,
    client,
    "pinned",
  );
  const refusal = snapshot.turnRefusal;
  // A replayed receipt (relay refetch, reconnect backfill) is the same
  // refusal; restoring the draft twice would duplicate the person's words.
  const settledRef = React.useRef(false);

  React.useEffect(() => {
    if (settledRef.current || !refusal) return;
    settledRef.current = true;
    onRefused(turn, refusal);
  }, [onRefused, refusal, turn]);

  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      if (settledRef.current) return;
      settledRef.current = true;
      // Silence here means the turn ran — the transcript says so. Stop
      // listening; say nothing.
      onExpired(turn.commandId);
    }, CODING_SESSION_TURN_REFUSAL_DEADLINE_MS);
    return () => window.clearTimeout(timer);
  }, [onExpired, turn.commandId]);

  return null;
}
