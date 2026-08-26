import * as React from "react";

import {
  heldPendingCodingSessionTurns,
  markPendingCodingSessionTurnDegraded,
  markPendingCodingSessionTurnQueued,
} from "@/features/coding-sessions/lib/codingSessionPendingTurns";
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
  /**
   * `buildCodingSessionTargetKey` of the execution this composer sends to.
   *
   * Given, this hook adopts the turns that execution's provider is still
   * holding when it mounts — see the re-arm effect below. Omitted, it watches
   * only the turns sent through it, which is all a surface with no optimistic
   * rows of its own needs.
   */
  targetKey?: string;
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
 * Irrelevance stays silent by design (see `codingSessionTurnRefusal.ts`), so
 * this waits briefly and then stops watching without a word rather than
 * inventing an outcome. Success is no longer silent where the provider
 * publishes per-stage receipts: a `turn_queued` is passed to the optimistic
 * row so it can stop implying the turn started.
 */
export function useCodingSessionTurnRefusal({
  channelId,
  client,
  providerAuthorityPubkey,
  restoreDraft,
  targetKey,
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

  // Adopt what the provider is still holding for this execution.
  //
  // A watch is a component and a held row is not: the row is exempt from the
  // pending TTL *and* from the refusal deadline, precisely because a queued
  // turn can wait behind an hour of work. Unmount the composer in that hour
  // — switch execution, close the panel, remount on a community switch — and
  // the `turn_dropped` that finally answers it arrives with nothing
  // listening, leaving a row that says "queued by the provider" forever with
  // the person's words trapped inside it.
  //
  // Once per scope, not on every store change: `watch` adds turns as they are
  // sent, and re-adding one a settled watcher has just retired would
  // resurrect it.
  const rearmedScope = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (targetKey === undefined) return;
    const scope = `${channelId}|${targetKey}`;
    if (rearmedScope.current === scope) return;
    rearmedScope.current = scope;
    const held = heldPendingCodingSessionTurns(channelId, targetKey);
    if (held.length === 0) return;
    setWatched((current) =>
      held.reduce(
        (watching, turn) =>
          watchCodingSessionTurn(watching, {
            commandId: turn.commandId,
            // The wire text is the fallback: a row recorded before this field
            // existed still gives the person something back.
            draft: turn.draft ?? turn.text,
          }),
        [...current],
      ),
    );
  }, [channelId, targetKey]);

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
  const stage = snapshot.turnProgress?.stage;
  const queued = stage === "queued";
  const degraded = stage === "degraded";
  const started = stage === "started";
  // Queued and degraded are the two stages that leave the turn in the
  // provider's hands. While it is there, this turn's fate is still open.
  const held = queued || degraded;

  // The same subscription answers a second question the person can see: the
  // provider signed for this turn and parked it behind work already running.
  // The optimistic row stops implying it is being worked on and says so.
  React.useEffect(() => {
    if (!queued) return;
    markPendingCodingSessionTurnQueued(channelId, turn.commandId);
  }, [channelId, queued, turn.commandId]);
  // And a third: the steer this turn asked for could not happen, so it will
  // arrive at the next turn boundary instead. Not a refusal — the words stay
  // sent — but not what was asked for either, so the row must not read as an
  // ordinary queue.
  React.useEffect(() => {
    if (!degraded) return;
    markPendingCodingSessionTurnDegraded(channelId, turn.commandId);
  }, [channelId, degraded, turn.commandId]);
  // A replayed receipt (relay refetch, reconnect backfill) is the same
  // refusal; restoring the draft twice would duplicate the person's words.
  const settledRef = React.useRef(false);

  React.useEffect(() => {
    if (settledRef.current || !refusal) return;
    settledRef.current = true;
    onRefused(turn, refusal);
  }, [onRefused, refusal, turn]);

  // A turn that has begun is the transcript's business from here: its echo
  // settles the optimistic row, and no further receipt is coming for it.
  React.useEffect(() => {
    if (settledRef.current || !started) return;
    settledRef.current = true;
    onExpired(turn.commandId);
  }, [onExpired, started, turn.commandId]);

  React.useEffect(() => {
    // A turn the provider signed for can legitimately sit in its mailbox for
    // longer than this deadline — behind a turn that runs for an hour. Twenty
    // seconds of silence is only evidence the turn ran when nobody has said
    // otherwise; once `turn_queued` says it has not, expiring the watch would
    // throw away the `turn_dropped` that may still be coming.
    if (held) return;
    const timer = window.setTimeout(() => {
      if (settledRef.current) return;
      settledRef.current = true;
      // Silence here means the turn ran — the transcript says so. Stop
      // listening; say nothing.
      onExpired(turn.commandId);
    }, CODING_SESSION_TURN_REFUSAL_DEADLINE_MS);
    return () => window.clearTimeout(timer);
  }, [held, onExpired, turn.commandId]);

  return null;
}
