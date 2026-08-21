/**
 * The words every surface uses for session coordination.
 *
 * The fold decides the facts; this file decides how they are spelled, once.
 * Project Pulse and Agent Progress speak in different registers — a card with
 * room for a sentence, a dense row with room for a word — but they must be
 * saying the same thing, so both registers live here side by side and the
 * equivalence is a table rather than a convention two files remember
 * separately.
 */

import type {
  CoordinatedGeneration,
  CoordinatedSession,
  SessionCoordinationState,
} from "./sessionCoordinationTypes.ts";

/**
 * The one coordination vocabulary, in both registers.
 *
 * `full` is what Project Pulse's session cards say; `compact` is what a dense
 * lane row says. They map 1:1 on purpose: a reader who learns "Reachable" on
 * one screen and "Provider-reachable" on the other must not have to wonder
 * whether they are the same claim.
 */
export const SESSION_COORDINATION_LABELS: Record<
  SessionCoordinationState,
  { compact: string; full: string }
> = {
  provider_reachable: { compact: "Reachable", full: "Provider-reachable" },
  open_unverified: {
    compact: "Unverified",
    full: "Open · liveness unverified",
  },
  closed: { compact: "Closed", full: "Closed" },
};

/** The dense-row word for a coordination state. */
export function coordinationStateCompactLabel(
  state: SessionCoordinationState,
): string {
  return SESSION_COORDINATION_LABELS[state].compact;
}

/** The full sentence-register phrase for a coordination state. */
export function coordinationStateFullLabel(
  state: SessionCoordinationState,
): string {
  return SESSION_COORDINATION_LABELS[state].full;
}

/** The provider's reported status, kept separate from coordination. */
export type CoordinationReportedStatus = {
  kind: "working" | "idle" | "ended" | "unknown";
  label: string;
};

/**
 * Map the 44223 wire status to presentation words shared by every coordination
 * surface. This is history, not liveness: callers still need an independent
 * {@link SessionCoordinationState} before claiming reachability.
 */
export function coordinationReportedStatus(
  status: string | null | undefined,
): CoordinationReportedStatus {
  if (status === null || status === undefined) {
    return { kind: "unknown", label: "Status unknown" };
  }
  switch (status) {
    case "starting":
    case "running":
    case "waiting_for_input":
      return { kind: "working", label: "Working" };
    case "idle":
      return { kind: "idle", label: "Idle" };
    case "completed":
      return { kind: "ended", label: "Completed" };
    case "stopped":
    case "interrupted":
      return { kind: "ended", label: "Stopped" };
    case "failed":
      return { kind: "unknown", label: "Needs attention" };
    case "disconnected":
      return { kind: "unknown", label: "Disconnected" };
    case "unknown":
      return { kind: "unknown", label: "Status unknown" };
  }
  return { kind: "unknown", label: "Status unknown" };
}

/**
 * Compact age for a duration in seconds — `4m`, `3h`, `2d`, `just now`.
 *
 * A negative age means the observation's clock ran ahead of ours. Saying
 * "in 4 minutes" about an observation is worse than admitting the skew.
 */
export function formatCoordinationAge(seconds: number): string {
  if (!Number.isFinite(seconds)) return "unknown";
  if (seconds < 0) return "just now";
  if (seconds < 60) return "just now";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`;
  return `${Math.floor(seconds / 86_400)}d`;
}

/** Current generation with the newest observation, then the newest fallback. */
export function coordinationDisplayGeneration(
  session: CoordinatedSession,
): CoordinatedGeneration | null {
  const current = session.generations.filter(
    (generation) => generation.current,
  );
  const candidates = current.length > 0 ? current : session.generations;
  return (
    [...candidates].sort((left, right) => {
      if (left.statusAt === null) return right.statusAt === null ? 0 : 1;
      if (right.statusAt === null) return -1;
      return right.statusAt - left.statusAt;
    })[0] ?? null
  );
}
