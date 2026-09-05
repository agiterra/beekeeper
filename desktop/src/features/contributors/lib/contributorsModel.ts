/**
 * The Contributors tab's pure model: one row per agent that holds or held a
 * seat on the project the route names, built from the same {@link SeatRow}
 * shape the Packs tab uses — but with closed seats kept in, because this
 * surface's whole point is seat *history*, not the live shelf.
 *
 * Only two membership states have evidence tonight (ruling §1): a seat this
 * computer can currently see (`isSeatedNow`) reads `Seated · <status> ·
 * <age>`; anything else reads `Past contributor`. There is no third state —
 * no Eligible, no Offered — because no consent record exists on the wire.
 */
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";

import type { SeatRow } from "../../roles/lib/seatRows";

export type ContributorRow = {
  agentPubkey: string;
  /** The managed agent's name; `null` means show the pubkey's first 8 chars. */
  name: string | null;
  /** Distinct role slugs this agent held on this project, sorted. */
  roles: string[];
  seatCount: number;
  /** The freshest seat's status, open or closed. */
  lastStatus: CodingSessionStatus;
  lastAgeSeconds: number | null;
  /** True when any of this agent's seats on this project is still open. */
  isSeatedNow: boolean;
};

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Freshest (smallest) age first; an unknown age sorts last. */
function ageRank(seconds: number | null): number {
  return seconds ?? Number.POSITIVE_INFINITY;
}

/** Sorted seated-first, then by freshest last-seen age, then by name. */
function compareContributorRows(a: ContributorRow, b: ContributorRow): number {
  if (a.isSeatedNow !== b.isSeatedNow) return a.isSeatedNow ? -1 : 1;
  const ageA = ageRank(a.lastAgeSeconds);
  const ageB = ageRank(b.lastAgeSeconds);
  if (ageA !== ageB) return ageA < ageB ? -1 : 1;
  return compareStrings(a.name ?? a.agentPubkey, b.name ?? b.agentPubkey);
}

export type BuildContributorRowsInput = {
  /** Every seat on this project, open and closed — from `buildSeatRows({ includeClosed: true })`. */
  seats: readonly SeatRow[];
  /** Keys (`SeatRow.key`) of the seats that are still open. */
  openKeys: ReadonlySet<string>;
};

/**
 * Group seats by agent into one contributor row each. A seat with no agent
 * (`agentPubkey === null`) is a person's session, not a contributor, and is
 * excluded.
 */
export function buildContributorRows(
  input: BuildContributorRowsInput,
): ContributorRow[] {
  const { seats, openKeys } = input;

  type Accumulator = {
    agentPubkey: string;
    name: string | null;
    roles: Set<string>;
    seatCount: number;
    lastStatus: CodingSessionStatus;
    lastAgeSeconds: number | null;
    isSeatedNow: boolean;
  };

  const byAgent = new Map<string, Accumulator>();

  for (const seat of seats) {
    if (seat.agentPubkey === null) continue;
    const isOpen = openKeys.has(seat.key);
    const existing = byAgent.get(seat.agentPubkey);
    if (!existing) {
      byAgent.set(seat.agentPubkey, {
        agentPubkey: seat.agentPubkey,
        name: seat.agentName,
        roles: new Set(seat.role ? [seat.role] : []),
        seatCount: 1,
        lastStatus: seat.status,
        lastAgeSeconds: seat.ageSeconds,
        isSeatedNow: isOpen,
      });
      continue;
    }
    existing.seatCount += 1;
    if (seat.role) existing.roles.add(seat.role);
    if (existing.name === null && seat.agentName !== null) {
      existing.name = seat.agentName;
    }
    if (isOpen) existing.isSeatedNow = true;
    // Freshest observation wins the last-seen fields, regardless of open/closed
    // — an age is the age of an observation, never a duration of absence.
    if (ageRank(seat.ageSeconds) < ageRank(existing.lastAgeSeconds)) {
      existing.lastStatus = seat.status;
      existing.lastAgeSeconds = seat.ageSeconds;
    }
  }

  const rows = [...byAgent.values()].map((acc) => ({
    agentPubkey: acc.agentPubkey,
    name: acc.name,
    roles: [...acc.roles].sort(compareStrings),
    seatCount: acc.seatCount,
    lastStatus: acc.lastStatus,
    lastAgeSeconds: acc.lastAgeSeconds,
    isSeatedNow: acc.isSeatedNow,
  }));

  return rows.sort(compareContributorRows);
}
