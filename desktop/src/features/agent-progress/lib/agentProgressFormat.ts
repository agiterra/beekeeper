/**
 * Presentation helpers for the Agent Progress panel. Rendering only — every
 * fact these dress up was decided by `agentProgressFold.ts`, which in turn got
 * its coordination facts from `shared/coordination`.
 *
 * The vocabulary is the shared one. `Reachable` here is the same claim Project
 * Pulse spells `Provider-reachable`, `Unverified` the same as `Open · liveness
 * unverified`, `Closed` the same as `Closed` — the mapping is a table in
 * `sessionCoordinationFormat.ts`, not a convention two files remember apart.
 */
import {
  coordinationReportedStatus,
  coordinationStateCompactLabel,
  formatCoordinationAge,
} from "@/shared/coordination/sessionCoordinationFormat";
import type { SessionCoordinationState } from "@/shared/coordination/sessionCoordinationTypes";

import type {
  AgentProgressAggregate,
  AgentProgressLane,
} from "./agentProgressFold";

/** The coordination word — the row's only liveness claim. */
export function agentLaneCoordinationText(lane: AgentProgressLane): string {
  return coordinationStateCompactLabel(lane.coordination);
}

/**
 * The reported-status half of the row, on its own axis.
 *
 * A reachable row states the status plainly, because a live lease means the
 * provider is answering right now and its newest report is current. Every
 * other row says **last reported**, with the age, because the report is the
 * last thing heard rather than the current condition — which is exactly the
 * distinction the old freshness window erased.
 */
export function agentLaneReportedText(lane: AgentProgressLane): string {
  if (lane.reportedStatus === null) return "no status reported";
  const label = coordinationReportedStatus(lane.reportedStatus).label;
  if (lane.coordination === "provider_reachable") return label;
  const age =
    lane.reportedAgeSeconds === null
      ? null
      : formatCoordinationAge(lane.reportedAgeSeconds);
  return age === null
    ? `last reported ${label.toLowerCase()}`
    : `last reported ${label.toLowerCase()} ${age} ago`;
}

/** Why the row makes the claim it makes, spelled out for a hover. */
export function agentLaneCoordinationTitle(lane: AgentProgressLane): string {
  switch (lane.coordination) {
    case "provider_reachable":
      return "A current, authority-signed lease for this session's live generation has not expired. Reachability is proven by that lease, not by how recently the session reported a status.";
    case "open_unverified":
      return "No unexpired lease proves this session's provider is reachable. It may still be running: this is the absence of proof, not proof of absence.";
    case "closed":
      return "A human closed this session. Closure outranks every provider signal, including a live lease.";
  }
}

/** Tailwind classes for the coordination dot. Muted is the honest default. */
export function agentLaneDotClass(state: SessionCoordinationState): string {
  return state === "provider_reachable"
    ? "bg-emerald-500"
    : "bg-muted-foreground/50";
}

/**
 * The footer summary parts, zero counts omitted.
 *
 * These are durable sessions, never provider executions, so a session that
 * reconnected twice is one session. The execution count is disclosed
 * separately (see {@link agentProgressExecutionsText}) rather than folded into
 * the headline number.
 *
 * There is deliberately no token or cost total: nothing this surface reads
 * carries usage, and a number assembled from nothing would read as measured.
 */
export function agentProgressFooterParts(
  aggregate: Pick<
    AgentProgressAggregate,
    "reachable" | "unverified" | "closed"
  >,
): string[] {
  const parts: string[] = [];
  if (aggregate.reachable > 0) parts.push(`${aggregate.reachable} reachable`);
  if (aggregate.unverified > 0)
    parts.push(`${aggregate.unverified} unverified`);
  if (aggregate.closed > 0) parts.push(`${aggregate.closed} closed`);
  return parts;
}

/**
 * The whole footer line.
 *
 * `at least` is not decoration: when a query failed, was truncated, or handed
 * back evidence the fold refused to resolve, the number below is a floor over
 * what this read returned. Printing it bare would turn an incomplete read into
 * a census.
 */
export function agentProgressFooterText(
  aggregate: AgentProgressAggregate,
): string {
  const noun = aggregate.sessions === 1 ? "session" : "sessions";
  if (aggregate.sessions === 0) {
    return aggregate.atLeast
      ? "This read did not complete — no sessions in what it returned"
      : "No sessions in what this read returned";
  }
  const head = aggregate.atLeast
    ? `At least ${aggregate.sessions} ${noun}`
    : `${aggregate.sessions} ${noun}`;
  const parts = agentProgressFooterParts(aggregate);
  return parts.length > 0 ? `${head} · ${parts.join(" · ")}` : head;
}

/** The execution disclosure, shown only when it says something new. */
export function agentProgressExecutionsText(
  aggregate: AgentProgressAggregate,
): string | null {
  if (aggregate.executions <= aggregate.sessions) return null;
  return `${aggregate.executions} executions`;
}
