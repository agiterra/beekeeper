/**
 * Who is waiting on whom, and since when.
 *
 * Live 2026-09-01, 12:36: `Assignment open 3m 3s · no report yet` existed in
 * exactly one place on the whole surface — the Route rail's road head — where
 * it never named the waiter, never gave a clock, and vanished entirely with
 * the rail below a ~1590 px window. "Who is blocked on whom" is the first
 * thing a person watching a team wants and the one thing the layout dropped
 * first.
 *
 * So the fact is derived here instead, once, from the same signed inputs
 * `deriveCodingSessionRoute` already takes — the projected transaction rows,
 * the resolved seat roster, and `nowMs`. Nothing here reads prose: an
 * assignment is open because no report references it, and a report is open
 * because no verdict does. Silence is never evidence.
 *
 * Every field is null when unknown, and the copy builders below print the
 * null cases as their own words rather than dropping a clause and leaving the
 * reader to guess which fact went missing.
 */
import {
  formatCodingSessionRouteDuration,
  type CodingSessionRouteParticipant,
  type CodingSessionRouteTransaction,
} from "./codingSessionRouteTypes";
import { CODING_SESSION_UNKNOWN_ACTOR } from "./codingSessionTurnByline";

/** One unanswered handoff: somebody waiting, somebody holding, and a clock. */
export type CodingSessionMissionOpenHold = {
  /**
   * Who is waiting for the answer — the party that signed the open fact.
   * Null when nothing resolves them to a name; never a pubkey.
   */
  waiterLabel: string | null;
  /**
   * Who owes the answer, named exactly as the roster names them, so this line
   * and the seat's own chip cannot read as two different people.
   *
   * Null when the counterparty resolves to no seat in this umbrella. The
   * caller must then place the hold on the *fact's* own row rather than on a
   * seat: a hold rendered against the wrong seat is worse than one rendered
   * against none.
   */
  holderLabel: string | null;
  /** Which fact is open: an assignment with no report, a report with no verdict. */
  holding: "assignment" | "report";
  /** How long it has been open, in ms; null when nothing dates it. */
  sinceMs: number | null;
  /** Unix seconds the open fact was signed; null when nothing dates it. */
  sinceAt: number | null;
  /** Signed event id of the open fact, for the caller's own disclosure. */
  sourceEventId: string | null;
};

/** What the section says when there is genuinely nothing open. */
export const CODING_SESSION_NO_OPEN_HOLDS = "No open assignments.";

/** The word for a party nothing in this mission claims. */
const UNRESOLVED_HOLDER = "holder not resolved";

/**
 * How many open holds the Team section will render (I10).
 *
 * A mission with two hundred unanswered assignments is a real mission, and
 * two hundred lines in a 360 px rail is not a disclosure — it is the same
 * silence by another route. Every other Inspector collection is capped with a
 * visible truncation row; this one is too.
 */
export const CODING_SESSION_OPEN_HOLDS_LIMIT = 50;

/** The capped list, and how many the cap dropped. Never a bare array. */
export type CodingSessionMissionOpenHolds = {
  holds: readonly CodingSessionMissionOpenHold[];
  /** How many open holds exist beyond {@link holds}; 0 when none were cut. */
  omitted: number;
};

/** The Team section's truncation notice, in the shape the section already uses. */
export function codingSessionOpenHoldsTruncation(
  omitted: number,
): string | null {
  return omitted === 0
    ? null
    : `Showing the ${CODING_SESSION_OPEN_HOLDS_LIMIT} oldest open handoffs · ` +
        `${omitted} more not shown.`;
}

/**
 * Derive every open hold in one mission.
 *
 * Oldest first, because the thing a person watching wants at the top is the
 * handoff that has been unanswered longest, not the newest one.
 */
export function deriveCodingSessionMissionOpenHolds(input: {
  participants: readonly CodingSessionRouteParticipant[];
  transactions: readonly CodingSessionRouteTransaction[];
  founderPubkey?: string | null;
  /** Label for the founder's own party; the surface supplies `You` or a name. */
  founderLabel?: string;
  nowMs: number;
}): CodingSessionMissionOpenHolds {
  const answeredAssignments = new Set<string>();
  const answeredReports = new Set<string>();
  for (const row of input.transactions) {
    if (row.type === "report" && row.parentEventId !== null) {
      answeredAssignments.add(row.parentEventId);
    }
    if (row.type === "disposition" && row.parentEventId !== null) {
      answeredReports.add(row.parentEventId);
    }
  }

  const seatLabels = new Map<string, string>();
  for (const participant of input.participants) {
    if (participant.actorPubkey === null) continue;
    seatLabels.set(participant.actorPubkey.toLowerCase(), participant.label);
  }

  const founder =
    input.founderPubkey === null || input.founderPubkey === undefined
      ? null
      : input.founderPubkey.toLowerCase();

  const holds: CodingSessionMissionOpenHold[] = [];
  for (const row of input.transactions) {
    const holding =
      row.type === "assignment" &&
      !answeredAssignments.has(row.meta.sourceEventId)
        ? "assignment"
        : row.type === "report" && !answeredReports.has(row.meta.sourceEventId)
          ? "report"
          : null;
    if (holding === null) continue;

    const sinceAt = Number.isFinite(row.createdAt) ? row.createdAt : null;
    holds.push({
      // The waiter needs no seat — the founder assigns work and holds none —
      // so their own row label stands when the roster has nothing better.
      waiterLabel: resolveWaiterLabel({
        pubkey: row.actor.pubkey,
        label: row.actor.label,
        founder,
        founderLabel: input.founderLabel,
        seatLabels,
      }),
      // The holder is resolved exactly as the waiter is, founder included.
      // REVIEW-L4 F2: resolving the holder from `seatLabels` alone printed
      // `Bob · Builder waits on holder not resolved` over the single most
      // common hold in any mission — a seat filed a report and the *founder*
      // owes the verdict. The founder holds no seat, and the honest answer for
      // the person reading the screen is `you`.
      holderLabel:
        row.counterparty === null
          ? null
          : resolveWaiterLabel({
              pubkey: row.counterparty.pubkey,
              label: row.counterparty.label,
              founder,
              founderLabel: input.founderLabel,
              seatLabels,
            }),
      holding,
      sinceMs: sinceAt === null ? null : input.nowMs - sinceAt * 1_000,
      sinceAt,
      sourceEventId: row.meta.sourceEventId,
    });
  }

  const ordered = holds.sort(byOldestFirst);
  return {
    holds: ordered.slice(0, CODING_SESSION_OPEN_HOLDS_LIMIT),
    omitted: Math.max(0, ordered.length - CODING_SESSION_OPEN_HOLDS_LIMIT),
  };
}

/**
 * Line 1 of the Observer table: `Keystone waits on Ira · Verifier`.
 *
 * A hold whose waiter nothing resolves reads `Waiting on {holder}` rather than
 * inventing a subject — an unattributed sentence is better than one attributed
 * to the wrong person.
 */
export function codingSessionOpenHoldWaitLine(
  hold: CodingSessionMissionOpenHold,
): string {
  const holder = hold.holderLabel ?? UNRESOLVED_HOLDER;
  return hold.waiterLabel === null
    ? `Waiting on ${holder}`
    : `${hold.waiterLabel} waits on ${holder}`;
}

/**
 * Line 2 of the Observer table:
 * `Assignment open 3m 3s · since 12:33 PM · no report yet`.
 *
 * Takes the road head's own shape as well as a hold's, so the Route rail's
 * head and the Inspector's roster line are literally one string builder and
 * cannot drift. `null` when the subject holds nothing at all.
 *
 * A clause whose fact is missing is dropped whole rather than printed empty:
 * `formatCodingSessionRouteDuration` refuses anything under a second, so a
 * hold this client cannot date never reads `0s` — §9.4.4's rule, because a
 * distance nobody measured is not a distance of zero.
 */
export function codingSessionOpenHoldStatusLine(input: {
  holding: "assignment" | "report" | null;
  sinceMs: number | null;
  sinceAt: number | null;
}): string | null {
  if (input.holding === null) return null;
  const head = input.holding === "assignment" ? "Assignment" : "Report";
  const tail =
    input.holding === "assignment" ? "no report yet" : "no verdict yet";
  const duration =
    input.sinceMs === null
      ? null
      : formatCodingSessionRouteDuration(input.sinceMs);
  const clock = input.sinceAt === null ? null : openHoldClock(input.sinceAt);
  const since = clock === null ? "" : ` · since ${clock}`;
  return `${head} open${duration === null ? "" : ` ${duration}`}${since} · ${tail}`;
}

/**
 * A 12-hour clock label from unix seconds.
 *
 * The exact `toLocaleTimeString` call the Route rail's `Now` rule and the
 * stream's own row times already make, so two clocks on one screen cannot
 * disagree about the same moment. Null when the seconds do not name one.
 */
function openHoldClock(atSeconds: number): string | null {
  const date = new Date(atSeconds * 1_000);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : null;
}

/** A hold nothing dates sorts last: it can claim no place in the order. */
function byOldestFirst(
  left: CodingSessionMissionOpenHold,
  right: CodingSessionMissionOpenHold,
): number {
  if (left.sinceAt === null) return right.sinceAt === null ? 0 : 1;
  if (right.sinceAt === null) return -1;
  return left.sinceAt - right.sinceAt;
}

/**
 * The waiter's name: the roster's word when this party holds a seat, the row's
 * own word otherwise, and null when even that is the unknown-actor sentinel —
 * `unknown actor waits on …` is a sentence that says nothing twice.
 */
function resolveWaiterLabel(input: {
  pubkey: string;
  label: string;
  founder: string | null;
  founderLabel: string | undefined;
  seatLabels: ReadonlyMap<string, string>;
}): string | null {
  const key = input.pubkey.toLowerCase();
  const seat = input.seatLabels.get(key);
  if (seat !== undefined) return seat;
  if (input.founder !== null && key === input.founder && input.founderLabel) {
    return input.founderLabel;
  }
  return input.label === CODING_SESSION_UNKNOWN_ACTOR ? null : input.label;
}
