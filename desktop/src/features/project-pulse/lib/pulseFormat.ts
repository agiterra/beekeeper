/**
 * Presentation helpers for Project Pulse. Rendering only — every fact these
 * functions dress up was already decided by `pulseFold.ts`.
 *
 * Ages are computed at paint time, never folded into the digest: a humanized
 * age baked into a JSON field is stale the moment it is written, and Slice 2's
 * relay-signed kind 39011 would then carry a sentence that ages while the
 * digest sits in a client cache.
 */
import { codingSessionWireWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";

import {
  PULSE_COMMIT_NOT_CHECKED,
  type ProjectPulseDigest,
  type PulseDigestEntry,
  type PulseDigestSession,
} from "./pulseFold.ts";

/** Compact age for a duration in seconds — `4m`, `3h`, `2d`, `just now`. */
export function formatPulseAge(seconds: number): string {
  if (!Number.isFinite(seconds)) return "unknown";
  // A negative age means the observation's clock ran ahead of ours. Saying
  // "in 4 minutes" about an observation is worse than admitting the skew.
  if (seconds < 0) return "just now";
  if (seconds < 60) return "just now";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`;
  return `${Math.floor(seconds / 86_400)}d`;
}

/** `observed 4m ago` — the age of the session's newest 44223 observation. */
export function formatObservedAge(session: PulseDigestSession): string {
  return `observed ${formatPulseAge(session.observedAgeSeconds)} ago`;
}

/**
 * The commit-confirmation line: the fold's fixed tri-state string, with the
 * `verifiedAt` age appended to the two checked outcomes and nothing appended
 * to `Commit not checked` (there is no verification instant to age).
 *
 * Never says "relay reachable" or "relay unreachable": the underlying fact is
 * that the relay's advertised refs contained this exact commit at
 * `verifiedAt`, which says nothing about whether the session is connected.
 */
export function formatCommitConfirmation(
  session: PulseDigestSession,
  nowSeconds: number,
): string {
  if (
    session.commitConfirmation === PULSE_COMMIT_NOT_CHECKED ||
    session.verifiedAt === null
  ) {
    return session.commitConfirmation;
  }
  const age = formatPulseAge(nowSeconds - session.verifiedAt);
  return `${session.commitConfirmation} · ${age} ago`;
}

/** How a nullable observation renders. Unknown is never false. */
export function formatDirtyObservation(dirty: boolean | null): string {
  if (dirty === null) return "Worktree not observed";
  return dirty ? "Worktree dirty" : "Worktree clean";
}

/** The observed commit, shortened, or an honest absence. */
export function formatObservedCommit(commit: string | null): string {
  return commit === null ? "Commit unknown" : commit.slice(0, 12);
}

/**
 * The header status label for a session, from the one shared wire→label
 * mapping the coding-session workspace header uses. A second derivation here
 * would let the same session read `Working` on its shelf row and something
 * else on the Pulse card two cards below it.
 */
export function pulseSessionStatusLabel(session: PulseDigestSession): string {
  return codingSessionWireWorkspaceStatus(session.status as CodingSessionStatus)
    .label;
}

/** `Disconnected · last observed 3h ago` — the Last seen group's row label. */
export function formatLastSeenLabel(session: PulseDigestSession): string {
  const age = formatPulseAge(session.observedAgeSeconds);
  return `${pulseSessionStatusLabel(session)} · last observed ${age} ago`;
}

/** Sessions split into Active work and Last seen, per the §5.4 definition. */
export function groupPulseSessions(digest: ProjectPulseDigest): {
  activeWork: PulseDigestSession[];
  lastSeen: PulseDigestSession[];
} {
  return {
    activeWork: digest.sessions.filter(
      (session) => session.activity === "active",
    ),
    lastSeen: digest.sessions.filter(
      (session) => session.activity !== "active",
    ),
  };
}

/** Entries split into the active claims and the superseded history. */
export function groupPulseEntries(digest: ProjectPulseDigest): {
  active: PulseDigestEntry[];
  superseded: PulseDigestEntry[];
} {
  return {
    active: digest.entries.filter((entry) => entry.active),
    superseded: digest.entries.filter((entry) => !entry.active),
  };
}

/** Sentence-cased entry kind, e.g. `plan` → `Plan`. */
export function formatPulseEntryType(type: PulseDigestEntry["type"]): string {
  return `${type.slice(0, 1).toUpperCase()}${type.slice(1)}`;
}

/** Branch chips over a digest; `null` is the real "no branch" group. */
export function branchChipLabel(branch: string | null): string {
  return branch ?? "no branch";
}

/** Does this row belong to the selected branch chip? `null` selects nothing. */
export function matchesBranchFilter(
  branch: string | null,
  selected: string | null | undefined,
): boolean {
  if (selected === undefined) return true;
  return branch === selected;
}
