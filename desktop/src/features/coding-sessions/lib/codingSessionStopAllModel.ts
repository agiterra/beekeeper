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
  /** Null when no receipt has minted a target for it yet. */
  target: CodingSessionCommandTarget | null;
  /** Null when nothing trusted signs for this execution. */
  providerAuthorityPubkey: string | null;
};

export type CodingSessionStopAllModel =
  | {
      kind: "available";
      /** How many seats a confirm would stop. Always > 0. */
      liveCount: number;
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
export function codingSessionStopAllDescription(liveCount: number): string {
  return (
    `${liveCount === 1 ? "This seat" : "These seats"} will be stopped for ` +
    "everyone. The session stays open — its transcript, its people, and its " +
    `goal are untouched — but a stopped seat cannot be resumed: to carry the ` +
    "work on, add a provider to the session."
  );
}

/** The confirm's title: the count, so nobody stops more than they meant to. */
export function codingSessionStopAllTitle(liveCount: number): string {
  return `Stop ${liveCount} live ${liveCount === 1 ? "seat" : "seats"}?`;
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
}): CodingSessionStopAllModel {
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
      reason: "No seat in this session is live, so there is nothing to stop.",
    };
  }
  return {
    kind: "available",
    liveCount: stoppable.length,
    confirmTitle: codingSessionStopAllTitle(stoppable.length),
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
        title: codingSessionStopAllTitle(stoppable.length),
        description: codingSessionStopAllDescription(stoppable.length),
        action: `Stop ${stoppable.length === 1 ? "the seat" : "all seats"}`,
      },
    },
  };
}
