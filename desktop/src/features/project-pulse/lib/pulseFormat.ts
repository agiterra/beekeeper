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

/**
 * One umbrella session and every execution of it this read observed.
 *
 * A provider restarts a session as a new *generation* — a new `targetKey` — but
 * the umbrella `sessionRef` (and therefore the name a person gave it) stays the
 * same. The fold keeps one row per execution, which is right for a digest and
 * wrong for a screen: three restarts of `dedupe_test` on one branch at one
 * commit render as three full-height cards, and the sessions that are actually
 * distinct get pushed off the fold.
 *
 * So the grouping is presentation-only: the digest still carries every
 * execution, and this type just says which of them a card leads with and which
 * sit behind its disclosure. Nothing is dropped.
 */
export type PulseSessionExecutions = {
  /** The umbrella: the session's `sessionRef`, or its own key when it has none. */
  key: string;
  /** The newest execution — the observation the card shows. */
  latest: PulseDigestSession;
  /** Older executions of the same umbrella, newest first. */
  older: PulseDigestSession[];
  /** Executions of this umbrella, including {@link latest}. */
  count: number;
};

/**
 * Collapse executions onto their umbrella session, preserving digest order.
 *
 * The fold sorts sessions by `(statusAt desc, targetKey)`, so the first
 * execution of a key in that order is its newest one and becomes `latest`.
 * Groups appear in the order their newest execution does, which keeps the
 * screen's session order the digest's order.
 *
 * A session with no `sessionRef` is its own umbrella: two unrelated sessions
 * must never be merged because both lack the field that would tell them apart.
 */
export function groupPulseSessionExecutions(
  sessions: readonly PulseDigestSession[],
): PulseSessionExecutions[] {
  const groups: PulseSessionExecutions[] = [];
  const byKey = new Map<string, PulseSessionExecutions>();
  for (const session of sessions) {
    const key = session.sessionRef ?? session.targetKey;
    const existing = byKey.get(key);
    if (existing) {
      existing.older.push(session);
      existing.count += 1;
      continue;
    }
    const group: PulseSessionExecutions = {
      key,
      latest: session,
      older: [],
      count: 1,
    };
    byKey.set(key, group);
    groups.push(group);
  }
  return groups;
}

/**
 * Umbrella sessions split into Active work and Last seen, per the §5.4
 * definition applied to each umbrella's newest execution.
 *
 * The newest execution decides, so an umbrella whose current run is live is
 * Active work even when its earlier runs aged out — the older runs travel with
 * it rather than seeding a duplicate row under Last seen.
 */
export function groupPulseSessions(sessions: readonly PulseDigestSession[]): {
  activeWork: PulseSessionExecutions[];
  lastSeen: PulseSessionExecutions[];
} {
  const groups = groupPulseSessionExecutions(sessions);
  return {
    activeWork: groups.filter((group) => group.latest.activity === "active"),
    lastSeen: groups.filter((group) => group.latest.activity !== "active"),
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

/**
 * Salience order for the active list: the entries that can stop somebody else
 * from working come first.
 *
 * "Salience tracks consequence" (`VISION_ACTIVITY.md`): a standing *do not
 * touch* buried as row 3 of 3, in the same grey as a plan, is a wait-signal
 * the reader has to hunt for. Within a rank the digest's own newest-first
 * order is preserved, so this re-weights the list without inventing a
 * recency claim the fold did not make.
 */
export const PULSE_ENTRY_CONSEQUENCE_RANK: Record<
  PulseDigestEntry["type"],
  number
> = {
  blocker: 0,
  handoff: 1,
  plan: 2,
  milestone: 3,
  note: 4,
};

/** Entries reordered by consequence; input order breaks ties (stable). */
export function sortPulseEntriesByConsequence(
  entries: readonly PulseDigestEntry[],
): PulseDigestEntry[] {
  return [...entries].sort(
    (left, right) =>
      PULSE_ENTRY_CONSEQUENCE_RANK[left.type] -
      PULSE_ENTRY_CONSEQUENCE_RANK[right.type],
  );
}

/**
 * How old the read itself is — `read just now`, `read 4m ago`.
 *
 * Separate from every other age on the screen because it answers a different
 * question: not "how old is this claim" but "how old is this *answer*". A
 * cached digest painted under a screen that looks current is the failure this
 * line exists to prevent.
 */
export function formatPulseReadAge(secondsSinceRead: number): string {
  const age = formatPulseAge(secondsSinceRead);
  return age === "just now" ? "read just now" : `read ${age} ago`;
}

/**
 * `“First pass at the wire contract.” (Plan · 1h ago)` — an entry named by
 * what it says rather than by its event id.
 *
 * Shared, not duplicated: the entry rows and the error cards both have to
 * refer to an entry the reader can find on this screen, and two spellings of
 * the same reference would let one surface drift into printing a hash.
 */
export function pulseEntryReference(
  entry: PulseDigestEntry,
  nowSeconds: number,
): string {
  return `“${quotePulseEntryText(entry.text)}” (${formatPulseEntryType(
    entry.type,
  )} · ${formatPulseAge(nowSeconds - entry.createdAt)} ago)`;
}

/** Shorten a quoted entry for a one-line reference; the full text stays in a `title`. */
export function quotePulseEntryText(text: string, maxLength = 56): string {
  const collapsed = text.replace(/\s+/g, " ").trim();
  return collapsed.length <= maxLength
    ? collapsed
    : `${collapsed.slice(0, maxLength - 1).trimEnd()}…`;
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

/**
 * How many rows a branch chip would show: session cards plus **active** entries.
 *
 * Sessions are counted as *umbrella* rows, not as executions, because that is
 * what the screen renders — a chip promising 19 rows over 12 cards is a count
 * that outran the thing it counts.
 *
 * Superseded entries are deliberately excluded — they sit behind a disclosure
 * whose own count is computed from the same filtered list, and a chip count
 * that included them would promise rows the screen does not show until asked.
 */
export function countPulseBranchRows(
  digest: ProjectPulseDigest,
  selected: string | null | undefined,
): number {
  const sessions = groupPulseSessionExecutions(
    digest.sessions.filter((session) =>
      matchesBranchFilter(session.branch, selected),
    ),
  ).length;
  const entries = digest.entries.filter(
    (entry) => entry.active && matchesBranchFilter(entry.branch, selected),
  ).length;
  return sessions + entries;
}

/**
 * `1 entry` / `3 entries` / `no entries` — a count that reads as a sentence.
 *
 * Used by the at-a-glance summary under the header, whose whole job is to let
 * a reader see that there *is* an entry (and how many sessions sit under it)
 * without scrolling a long session list.
 */
export function formatPulseEntryCount(count: number): string {
  if (count === 0) return "no entries";
  return `${count} ${count === 1 ? "entry" : "entries"}`;
}

/** `1 session` / `12 sessions` / `no sessions` — umbrellas, not executions. */
export function formatPulseSessionCount(count: number): string {
  if (count === 0) return "no sessions";
  return `${count} ${count === 1 ? "session" : "sessions"}`;
}

/**
 * `3 executions` — the umbrella's disclosure label.
 *
 * Counts the whole umbrella, including the execution the card already shows,
 * so the number answers "how many times has this session run?" rather than
 * "how many rows are hidden?".
 */
export function formatPulseExecutionCount(count: number): string {
  return `${count} ${count === 1 ? "execution" : "executions"}`;
}

/**
 * Does the `Closed` chip repeat what the status label already said?
 *
 * A session whose wire status is `stopped` renders `Ended · last observed 30m
 * ago`; putting a `Closed` chip next to it is two words for one fact. Every
 * other status — `disconnected`, `failed`, an unknown one — carries information
 * the closure does not, so the chip stays.
 */
export function pulseSessionClosedIsRestated(
  session: PulseDigestSession,
): boolean {
  return (
    session.closed &&
    codingSessionWireWorkspaceStatus(session.status as CodingSessionStatus)
      .kind === "ended"
  );
}
