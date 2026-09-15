/**
 * The Roles page's four headline counts, as one pure function over the two
 * models the page already built. No React, no fetching, no second source: a
 * number here is a set size taken from the same rows the cards below draw,
 * so the strip can never disagree with them.
 *
 * Two rules keep the counts honest. Agents and sessions are counted by
 * *identity* (pubkey, seat key) across every list they can appear in, because
 * one agent holds one role and one seat is filed under both a role and a
 * project — adding the lists would double-count. And a session is "open" only
 * while its own status word says it could still be running; the three words
 * that mean it is over are excluded rather than inferred from age.
 */
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";

import type { RolePackSnapshots } from "./rolePackSnapshots";
import type { RolesView } from "./rolesViewModel";
import type { SeatRow } from "./seatRows";

/** Statuses that mean the session is over. Everything else is still open. */
const FINISHED_STATUSES: ReadonlySet<CodingSessionStatus> = new Set([
  "completed",
  "stopped",
  "failed",
]);

/** The same execution-status boundary for summary counts and role activity. */
export function roleSeatIsOpen(status: CodingSessionStatus): boolean {
  return !FINISHED_STATUSES.has(status);
}

export type RolesPageSummary = {
  /** Role cards on the page. */
  roles: number;
  /** Distinct agents across every card, local and shared together. */
  agents: number;
  /** Of those, the ones this computer manages a record for. */
  agentsLocal: number;
  /**
   * Of those, the ones only seen in this project (`isManagedHere === false`).
   * An agent seen as shared anywhere is counted here, so the two never
   * overstate what this computer set up: `agentsLocal + agentsShared` is
   * always `agents`.
   */
  agentsShared: number;
  /**
   * Distinct identities that held a session here but are not this project's
   * agents on this computer (`RoleRow.nonProjectAgents`). Never in `agents`.
   */
  nonProjectAgents: number;
  /** Distinct seats, in any list, whose status is not a finished one. */
  openSessions: number;
  /** Reports this project's channels carried. */
  reports: number;
  /** Of those, the ones whose sender could not be confirmed. */
  unconfirmed: number;
  /** Of those, the ones the provenance fold contradicted. */
  disputed: number;
};

export function rolesPageSummary(
  view: RolesView,
  snapshots: RolePackSnapshots,
): RolesPageSummary {
  const agents = new Set<string>();
  const shared = new Set<string>();
  const nonProject = new Set<string>();
  const openSeats = new Set<string>();
  const countSeat = (seat: SeatRow) => {
    if (roleSeatIsOpen(seat.status)) openSeats.add(seat.key);
  };

  for (const role of view.roles) {
    for (const agent of role.agents) {
      agents.add(agent.pubkey);
      if (agent.isManagedHere === false) shared.add(agent.pubkey);
    }
    for (const agent of role.nonProjectAgents ?? [])
      nonProject.add(agent.pubkey);
    for (const seat of role.seats) countSeat(seat);
  }
  for (const row of view.byProject) {
    for (const seat of row.seats) countSeat(seat);
  }
  for (const seat of view.unplaced) countSeat(seat);

  let unconfirmed = 0;
  let disputed = 0;
  for (const row of snapshots.reported) {
    if (row.provenance === "proof-unavailable") unconfirmed += 1;
    else if (row.provenance === "disputed") disputed += 1;
  }

  return {
    roles: view.roles.length,
    agents: agents.size,
    agentsLocal: agents.size - shared.size,
    agentsShared: shared.size,
    nonProjectAgents: [...nonProject].filter((key) => !agents.has(key)).length,
    openSessions: openSeats.size,
    reports: snapshots.reported.length,
    unconfirmed,
    disputed,
  };
}
