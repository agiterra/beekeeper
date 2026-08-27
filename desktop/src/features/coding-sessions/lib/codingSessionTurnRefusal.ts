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
 * the turn into its mailbox, `turn_degraded` when a steer it cannot perform
 * becomes a boundary delivery, `turn_started` when the turn begins,
 * `turn_dropped` when it will never run it (`QUEUE_FULL`, or
 * `NO_LIVE_EXECUTION` for a session with nothing running), and `turn_refused`
 * for a decision about the sender or the target. Refused and dropped are
 * different words for different facts and this module keeps them apart:
 * nobody was refused when a queue filled up. Degraded is neither — the turn
 * still runs — so it never reaches this module's error line at all.
 */
import { MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET } from "./codingSessionPendingTurns";
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
 * How many sent turns are watched at once.
 *
 * Exactly the number of pending rows one execution can hold, because a watch
 * is now the only thing that can retire one. When the composer still queued
 * drafts locally a person could not have more than a few turns outstanding,
 * and an evicted watch cost nothing: the row expired at the pending TTL. A
 * turn the provider has signed for is exempt from that TTL *and* from the
 * refusal deadline, so a row whose watch was evicted has nothing left that can
 * end it — its `turn_dropped` arrives to a closed subscription and the row
 * sits above the composer forever. Each watch holds a relay subscription; that
 * is the cost of not lying about a message that was really sent.
 */
export const MAX_WATCHED_CODING_SESSION_TURNS =
  MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET;

/** Fallback when a refusal receipt carries no readable message. */
export const CODING_SESSION_TURN_REFUSED_MESSAGE =
  "The provider refused this turn.";

/**
 * Fallback when a drop receipt carries no readable message.
 *
 * Deliberately does not name a cause. A drop used to mean one thing (the
 * queue was full) and now means several — `QUEUE_FULL`, or `NO_LIVE_EXECUTION`
 * when the session has no running execution to deliver into — so guessing the
 * reason in the fallback would put a specific wrong sentence on screen. The
 * provider's own `code` is shown beside this either way.
 */
export const CODING_SESSION_TURN_DROPPED_MESSAGE =
  "The provider dropped this turn without running it.";

/** One sent turn, held only until it is refused or the wait expires. */
export type WatchedCodingSessionTurn = {
  commandId: string;
  /**
   * The generation this turn was addressed to.
   *
   * Kept so a refusal can be answered honestly: a `NO_LIVE_EXECUTION` or
   * `STALE_GENERATION` receipt only offers a resend when the execution has
   * since resumed into a *newer* generation, and that comparison needs the
   * number the command actually named. Optional because a watch re-armed from
   * a row recorded before this field existed has no such number, and guessing
   * one would put a fabricated generation on the wire.
   */
  generation?: number;
  /**
   * `buildCodingSessionExecutionKey` of the execution this turn was sent to —
   * the identity that survives a generation bump.
   *
   * The composer instance is reused when the umbrella switches which
   * participant it is addressing, so a refusal from one execution can still be
   * in hand while the editor points at another. Re-addressing must never cross
   * that line: these words were written for the seat they were sent to.
   */
  executionKey?: string;
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
 * translated away. The known codes are specific — `UNAUTHORIZED_OPERATOR`,
 * `UNKNOWN_TARGET`, `STALE_GENERATION`, `SESSION_CLOSED`, `QUEUE_FULL`,
 * `NO_LIVE_EXECUTION` — but the set is open, and an unfamiliar one is shown
 * verbatim rather than swallowed: a person comparing what they see to what
 * `bee sessions transcript` prints, or quoting it to whoever runs the
 * provider, needs the same word both places.
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

/**
 * The two receipt codes that mean the words were never run and could still
 * run somewhere else.
 *
 * Generation fencing is why they exist. A turn is addressed to an exact
 * `(driver, instance, session, generation)`; when the provider comes back, the
 * session it resumed is generation N+1 and the replayed command addresses N,
 * which nothing owns any more. The provider answers durably — `turn_dropped`
 * with `NO_LIVE_EXECUTION`, or `turn_refused` with `STALE_GENERATION` — and
 * stops there. It cannot re-address the turn itself: nothing but the sender
 * can decide that these words still apply to the session that came back.
 *
 * Every other code is a decision about the sender (`UNAUTHORIZED_OPERATOR`),
 * the target (`UNKNOWN_TARGET`, `SESSION_CLOSED`), or the provider's own
 * mailbox (`QUEUE_FULL`). Re-sending the same words to a newer generation
 * would not change any of those answers, so offering it would be a lie about
 * what happened.
 */
export const CODING_SESSION_READDRESSABLE_REFUSAL_CODES = [
  "NO_LIVE_EXECUTION",
  "STALE_GENERATION",
] as const;

/** True for a refusal whose only remedy is a fresh command to a live generation. */
export function isCodingSessionReaddressableRefusal(
  refusal: CodingSessionCommandRefusal,
): boolean {
  const code = refusal.code.trim().toUpperCase();
  return (
    CODING_SESSION_READDRESSABLE_REFUSAL_CODES as readonly string[]
  ).includes(code);
}

/** The exact words on the re-addressing control (crew plan ruling R1). */
export const CODING_SESSION_READDRESS_LABEL = "Resend to the resumed execution";

/**
 * Whether an owed turn can be re-addressed right now, and to what.
 *
 * `offer` names the generation the fresh command will be addressed to. It is
 * always the execution's *current* generation — the one this composer is
 * already pointed at — never the one that refused, and never a number this
 * module invents.
 */
export type CodingSessionReaddressOffer =
  | { kind: "offer"; generation: number; label: string }
  | { kind: "unavailable"; reason: string };

/**
 * Decide what to offer after a `NO_LIVE_EXECUTION` / `STALE_GENERATION`
 * receipt, answering the three questions ruling R1 named.
 *
 * *Which generation it resolves to:* the current one, and only when it is
 * strictly newer than the generation that refused. A resend into the same
 * generation would be answered by the same refusal, so the offer is withheld
 * and the reason says a resume is what is missing.
 *
 * *Who resumed it:* deliberately not claimed. This client sees a generation
 * number move; it does not see who moved it, and a sentence naming a person
 * would be a guess. The words back in the editor plus the new number are the
 * whole of what is known.
 *
 * *What happens when the session is closed:* nothing is offered. A stopped
 * execution has no live generation and cannot gain one, so the honest answer
 * is that these words need a different execution, not another attempt at this
 * one.
 */
export function resolveCodingSessionReaddress({
  currentGeneration,
  isEnded,
  refusedGeneration,
}: {
  /** The generation this composer is addressing now. */
  currentGeneration: number;
  /** The execution has stopped for good (`lifecycleStatus === "stopped"`). */
  isEnded: boolean;
  /** The generation the refused command named, when this client still knows it. */
  refusedGeneration: number | undefined;
}): CodingSessionReaddressOffer {
  if (isEnded) {
    return {
      kind: "unavailable",
      reason:
        "This execution has ended, so there is no generation to resend into. Your words are back in the editor — send them to another execution.",
    };
  }
  if (refusedGeneration === undefined) {
    return {
      kind: "unavailable",
      reason:
        "This client no longer knows which generation refused this turn, so it will not guess one. Your words are back in the editor.",
    };
  }
  if (currentGeneration <= refusedGeneration) {
    return {
      kind: "unavailable",
      reason:
        "Nothing has resumed this execution yet, so there is no newer generation to send to. Reconnect it, then send these words again.",
    };
  }
  return {
    kind: "offer",
    generation: currentGeneration,
    label: CODING_SESSION_READDRESS_LABEL,
  };
}
