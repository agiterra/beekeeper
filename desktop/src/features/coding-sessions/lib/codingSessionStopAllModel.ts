/**
 * "Stop all": which seats of an umbrella a single click would stop, and who
 * is allowed to click it.
 *
 * Item 87(e), found live 2026-08-28: a team launch seated a lead, the lead
 * hired two more seats, and the person who started it had no control anywhere
 * that stopped them. The per-execution stop lives in each execution's own
 * composer, which is exactly the surface you cannot reach when what you want
 * is "all of it, now".
 *
 * Pure on purpose. The authority question ("may this viewer stop other
 * people's agents?") and the scope question ("which executions are still
 * live?") are both answerable from signed facts, so they are answered here and
 * proved in `node:test` rather than inferred inside a click handler.
 */
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import type { EndCodingSessionRequest } from "./endCodingSessionModel";

/** One execution of the umbrella, as this decision needs to see it. */
export type CodingSessionStopAllExecution = {
  /** How the confirm and the transcript name this seat. */
  label: string;
  /** The derived workspace status; `ended` executions have nothing to stop. */
  status: { kind: string };
  /**
   * Whether W1 calls this seat live.
   *
   * **This MUST come from `deriveCodingSessionStreamPresence` — the same map
   * the roster chips read — and from nowhere else.** L4.3, seen live
   * 2026-09-01: the header printed `Stop all (2)` with an aria label reading
   * `Stop 2 live seats` over one working seat and one idle one. The count was
   * right and the word was the lie. Stoppable and live are two different
   * questions, and a surface that answers the second from the first's number
   * is inventing liveness. Re-deriving it here from `status` would rebuild the
   * same fault one layer down, so the caller hands it over instead.
   */
  live: boolean;
  /** Null when no receipt has minted a target for it yet. */
  target: CodingSessionCommandTarget | null;
  /** Null when nothing trusted signs for this execution. */
  providerAuthorityPubkey: string | null;
};

export type CodingSessionStopAllModel =
  | {
      kind: "available";
      /** How many seats a confirm would stop. Always > 0. */
      seatCount: number;
      /**
       * How many of those {@link seatCount} seats W1 calls live — never more,
       * and sometimes 0. The split exists so no caller can print the word
       * `live` over a number it did not get from the presence map.
       */
      liveCount: number;
      /** `Stop all (2 seats)`. The caller renders this, never its own count. */
      buttonLabel: string;
      /** The control's title and accessible name, split and consequence both. */
      sentence: string;
      confirmTitle: string;
      confirmDescription: string;
      request: EndCodingSessionRequest;
    }
  | {
      kind: "hidden";
      /**
       * Why there is no control. Never rendered as a disabled button — a
       * viewer who is not the founder is not being refused, the control is
       * simply not theirs — but the sentence exists so a caller that *does*
       * want to explain (a tooltip, a test) is not left inventing one.
       */
      reason: string;
    };

/**
 * The confirm's own sentence.
 *
 * Deliberately not "seats can be resumed". A stopped execution is not
 * resumable in this app — the composer offers Reconnect for `disconnected`
 * only (`CodingSessionComposerSurface.tsx`) — and the existing single-stop
 * confirm has said so since §2 item 42. A bulk control that promised
 * otherwise would be the more comfortable of two sentences and the false one.
 */
export function codingSessionStopAllDescription(seatCount: number): string {
  return (
    `${seatCount === 1 ? "This seat" : "These seats"} will be stopped for ` +
    "everyone. The session stays open — its transcript, its people, and its " +
    `goal are untouched — but a stopped seat cannot be resumed: to carry the ` +
    "work on, add a provider to the session."
  );
}

/**
 * The confirm's title: the count, so nobody stops more than they meant to.
 *
 * `Stop N seats?`, not `Stop N live seats?`. The number has always been the
 * seats this control can stop, which includes idle ones; borrowing the seat
 * word `live` for it was the same lie the header's aria label told (L4.3).
 *
 * **Mission only** (REVIEW-L4 F6). This dialog is mounted from the workspace,
 * which is both lenses, and L4.3 is a Mission ruling — so Conversation keeps
 * its literal `Stop N live seats?`. The masked `outerHTML` baseline cannot
 * see this: the dialog is closed in the dump, which is exactly why the rule
 * has to be applied by hand rather than trusted to the diff.
 */
export function codingSessionStopAllTitle(
  seatCount: number,
  mission = true,
): string {
  const noun = seatCount === 1 ? "seat" : "seats";
  return mission
    ? `Stop ${seatCount} ${noun}?`
    : `Stop ${seatCount} live ${noun}?`;
}

/** `Stop all (2 seats)` — the control's own label, count and noun together. */
export function codingSessionStopAllButtonLabel(seatCount: number): string {
  return `Stop all (${seatCount} ${seatCount === 1 ? "seat" : "seats"})`;
}

/**
 * The control's title and accessible name: what it stops, how much of that is
 * live, and what stopping costs.
 *
 * `Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.` The
 * liveness half is only ever the caller's W1 count; when it is zero the
 * sentence says `none live` rather than quietly dropping the clause, because a
 * missing clause reads as "all of them".
 *
 * A single seat states its own word and stops there — `Stop 1 seat — live.` —
 * per the L4.3 copy table. The unrecoverability warning rides the plural form
 * only; the single-seat control keeps the confirm dialog's own sentence
 * ({@link codingSessionStopAllDescription}) for that.
 */
export function codingSessionStopAllSentence(input: {
  seatCount: number;
  liveCount: number;
}): string {
  const live = Math.min(input.liveCount, input.seatCount);
  if (input.seatCount === 1) {
    return `Stop 1 seat — ${live === 1 ? "live" : "idle"}.`;
  }
  const idle = input.seatCount - live;
  const split =
    live === 0
      ? "none live"
      : idle === 0
        ? `${live} live`
        : `${live} live, ${idle} idle`;
  return (
    `Stop ${input.seatCount} seats — ${split}. ` +
    "A stopped seat cannot be resumed."
  );
}

/**
 * What the umbrella header should offer this viewer.
 *
 * Founder-only, and hidden rather than disabled for anyone else: stopping
 * somebody else's agents is authority, and a greyed-out control invites the
 * click that teaches you that you do not have it. An execution with no target
 * or no provider authority is dropped from the fan-out rather than counted —
 * a confirm that says "3 seats" and stops two is the same class of lie as a
 * roster that lists four and creates one.
 */
export function buildCodingSessionStopAll(input: {
  channelId: string;
  founderPubkey: string | null;
  currentUserPubkey: string | null;
  executions: readonly CodingSessionStopAllExecution[];
  /**
   * Is the surface asking the Mission lens? Defaults to Conversation's
   * pre-L4 strings, so any caller that has not adopted the flag is unchanged
   * (F6).
   */
  mission?: boolean;
}): CodingSessionStopAllModel {
  const mission = input.mission ?? false;
  const founder = input.founderPubkey?.toLowerCase() ?? null;
  const viewer = input.currentUserPubkey?.toLowerCase() ?? null;
  if (founder === null) {
    return {
      kind: "hidden",
      reason:
        "This session's founder has not resolved yet, so nothing here can " +
        "claim the authority to stop its seats.",
    };
  }
  if (viewer === null || viewer !== founder) {
    return {
      kind: "hidden",
      reason:
        "Only the person who founded this session can stop its seats. Stop " +
        "your own execution from its composer.",
    };
  }
  const stoppable = input.executions.filter(
    (execution) =>
      execution.status.kind !== "ended" &&
      execution.target !== null &&
      execution.providerAuthorityPubkey !== null,
  );
  if (stoppable.length === 0) {
    return {
      kind: "hidden",
      // Not "no seat is live" — an idle seat is stoppable, and saying `live`
      // over the stoppable set is the same borrowed word L4.3 removed above.
      // Mission-gated for the same reason the title is (F6).
      reason: mission
        ? "Every seat in this session has ended or cannot be addressed, so " +
          "there is nothing to stop."
        : "No seat in this session is live, so there is nothing to stop.",
    };
  }
  // The liveness split, taken from the caller's W1 map and never re-derived
  // from `status`: an ended seat is out of both counts because it is out of
  // `stoppable`, and every other seat says `live` only if the map did.
  const liveCount = stoppable.filter((execution) => execution.live).length;
  return {
    kind: "available",
    seatCount: stoppable.length,
    liveCount,
    buttonLabel: codingSessionStopAllButtonLabel(stoppable.length),
    sentence: codingSessionStopAllSentence({
      seatCount: stoppable.length,
      liveCount,
    }),
    confirmTitle: codingSessionStopAllTitle(stoppable.length, mission),
    confirmDescription: codingSessionStopAllDescription(stoppable.length),
    request: {
      label: stoppable.map((execution) => execution.label).join(", "),
      channelId: input.channelId,
      stops: stoppable.map((execution) => ({
        // Narrowed by the filter above; the nulls never reach here.
        target: execution.target as CodingSessionCommandTarget,
        providerAuthorityPubkey: execution.providerAuthorityPubkey as string,
      })),
      confirm: {
        title: codingSessionStopAllTitle(stoppable.length, mission),
        description: codingSessionStopAllDescription(stoppable.length),
        action: `Stop ${stoppable.length === 1 ? "the seat" : "all seats"}`,
      },
    },
  };
}
