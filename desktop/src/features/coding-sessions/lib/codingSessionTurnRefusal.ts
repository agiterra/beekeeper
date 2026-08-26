/**
 * What a sent turn does when the provider refuses it.
 *
 * A turn is not accepted merely because the relay took the 44220. The provider
 * decides who may steer an execution, and an operator it does not recognise
 * gets a signed `failed` receipt (`UNAUTHORIZED_OPERATOR`) rather than a turn.
 * Nothing else on the client reads that receipt, so without this the refusal is
 * published, verified, and dropped: the person's message simply vanishes.
 *
 * The provider's silences are deliberate and must not be dressed up as
 * failures. A turn addressed to a session some other provider owns is ignored
 * without a word, because every provider on the channel sees every command. So
 * the wait is short, and it expires in silence.
 *
 * What is *not* silence any more is everything else about a turn's fate. A
 * provider on the per-stage receipt contract signs `turn_queued` when it takes
 * the turn into its mailbox, `turn_started` when the turn begins,
 * `turn_dropped` when its queue overflowed, and `turn_refused` for a decision
 * about the sender or the target. Refused and dropped are different words for
 * different facts and this module keeps them apart: nobody was refused when a
 * queue filled up.
 */
import type { CodingSessionCommandRefusal } from "./codingSessionTrustedIngress";

/**
 * How long a sent turn is watched for a refusal.
 *
 * Shorter than the lifecycle stall deadline on purpose: a refusal is decided
 * before any agent work starts (it is a check on the operator, not on the
 * turn), so it arrives in the same breath as the publish or never.
 */
export const CODING_SESSION_TURN_REFUSAL_DEADLINE_MS = 20_000;

/**
 * How many sent turns are watched at once. Each watch holds a relay
 * subscription, and a person cannot meaningfully be waiting on more refusals
 * than this; the oldest watch is the one whose deadline is nearest anyway.
 */
export const MAX_WATCHED_CODING_SESSION_TURNS = 4;

/** Fallback when a refusal receipt carries no readable message. */
export const CODING_SESSION_TURN_REFUSED_MESSAGE =
  "The provider refused this turn.";

/** Fallback when a drop receipt carries no readable message. */
export const CODING_SESSION_TURN_DROPPED_MESSAGE =
  "The provider's turn queue was full, so this turn was dropped.";

/** One sent turn, held only until it is refused or the wait expires. */
export type WatchedCodingSessionTurn = {
  commandId: string;
  /**
   * The words the person actually typed, kept verbatim so a refusal can put
   * them back. The published text may differ (the umbrella composer strips a
   * routing `@handle`), and it is the draft — not the wire text — that belongs
   * back in the editor.
   */
  draft: string;
};

/**
 * Name the outcome for the composer's error line, in the provider's words.
 *
 * The provider's own `code` is shown alongside its sentence rather than
 * translated away. The codes are few and specific — `UNAUTHORIZED_OPERATOR`,
 * `UNKNOWN_TARGET`, `STALE_GENERATION`, `SESSION_CLOSED`, `QUEUE_FULL` — and a
 * person comparing what they see to what `bee sessions transcript` prints, or
 * quoting it to whoever runs the provider, needs the same word both places.
 */
export function formatCodingSessionTurnRefusal(
  refusal: CodingSessionCommandRefusal,
): string {
  const dropped = refusal.outcome === "dropped";
  const label = dropped ? "Turn dropped" : "Turn refused";
  const message =
    refusal.message.trim() ||
    (dropped
      ? CODING_SESSION_TURN_DROPPED_MESSAGE
      : CODING_SESSION_TURN_REFUSED_MESSAGE);
  const code = refusal.code.trim();
  return code ? `${label} (${code}): ${message}` : `${label}: ${message}`;
}

/** Add one sent turn to the bounded watch set, dropping the oldest first. */
export function watchCodingSessionTurn(
  watched: readonly WatchedCodingSessionTurn[],
  turn: WatchedCodingSessionTurn,
  max: number = MAX_WATCHED_CODING_SESSION_TURNS,
): WatchedCodingSessionTurn[] {
  const next = [
    ...watched.filter((entry) => entry.commandId !== turn.commandId),
    turn,
  ];
  return next.slice(Math.max(0, next.length - Math.max(1, max)));
}

/** Drop a settled or expired watch. */
export function forgetCodingSessionTurn(
  watched: readonly WatchedCodingSessionTurn[],
  commandId: string,
): WatchedCodingSessionTurn[] {
  return watched.filter((entry) => entry.commandId !== commandId);
}

/**
 * Put refused words back in the editor without overwriting newer ones.
 *
 * Sending clears the editor, so a refusal that arrives while the person is
 * already typing again must not choose between the two drafts — it keeps both,
 * refused text first, since that is the one they wrote first.
 */
export function restoreCodingSessionDraft(
  current: string,
  refused: string,
): string {
  if (refused.trim().length === 0) return current;
  if (current.trim().length === 0) return refused;
  // A replayed refusal is the same fact, not a second copy of the message.
  if (current.includes(refused)) return current;
  return `${refused}\n\n${current}`;
}
