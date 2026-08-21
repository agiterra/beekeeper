/**
 * Presentation helpers for Project Pulse. Rendering only — every fact these
 * functions dress up was already decided by `pulseFold.ts`.
 *
 * Ages are computed at paint time, never folded into the digest: a humanized
 * age baked into a JSON field is stale the moment it is written, and Slice 2's
 * relay-signed kind 39011 would then carry a sentence that ages while the
 * digest sits in a client cache.
 */
import {
  coordinationDisplayGeneration,
  coordinationReportedStatus,
  coordinationStateFullLabel,
  formatCoordinationAge,
} from "@/shared/coordination/sessionCoordinationFormat";

import {
  PULSE_COMMIT_NOT_CHECKED,
  type ProjectPulseDigest,
  type PulseCoordinationState,
  type PulseDigestEntry,
  type PulseDigestGeneration,
  type PulseDigestSession,
} from "./pulseFold.ts";

/** Pulse's group-heading register, derived from the shared coordination words. */
export function pulseSessionGroupHeading(
  state: PulseCoordinationState,
): string {
  const label = coordinationStateFullLabel(state);
  if (state === "provider_reachable") return `${label} sessions`;
  if (state === "closed") return `${label}/history`;
  return label;
}

/**
 * Current generation with the newest observation, then the newest fallback.
 *
 * Shared with Agent Progress: which generation a row *speaks for* is part of
 * the coordination answer, not a Pulse styling choice.
 */
export const pulseSessionDisplayGeneration: (
  session: PulseDigestSession,
) => PulseDigestGeneration | null = coordinationDisplayGeneration;

/** Compact age for a duration in seconds — `4m`, `3h`, `2d`, `just now`. */
export const formatPulseAge: (seconds: number) => string =
  formatCoordinationAge;

/** `observed 4m ago` — the age of the session's newest 44223 observation. */
export function formatObservedAge(session: PulseDigestSession): string {
  if (session.observedAgeSeconds === null) return "observation time unknown";
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
  const generation = pulseSessionDisplayGeneration(session);
  if (!generation) return PULSE_COMMIT_NOT_CHECKED;
  if (
    generation.commitConfirmation === PULSE_COMMIT_NOT_CHECKED ||
    generation.verifiedAt === null
  ) {
    return generation.commitConfirmation;
  }
  const age = formatPulseAge(nowSeconds - generation.verifiedAt);
  return `${generation.commitConfirmation} · ${age} ago`;
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
  const status = pulseSessionDisplayGeneration(session)?.status ?? undefined;
  return coordinationReportedStatus(status).label;
}

/** Reachability wording appears only when the chosen generation proves it. */
export function formatProviderReachableLabel(
  session: PulseDigestSession,
): string {
  const generation = pulseSessionDisplayGeneration(session);
  const reachable =
    session.coordinationState === "provider_reachable" &&
    generation?.reachability === "provider_reachable";
  return `${pulseSessionStatusLabel(session)} · ${reachable ? "provider reachable · " : ""}${formatObservedAge(session)}`;
}

/** Open or closed rows report observation recency without inferring liveness. */
export function formatUnverifiedObservationLabel(
  session: PulseDigestSession,
): string {
  const age = session.observedAgeSeconds;
  return age === null
    ? `${pulseSessionStatusLabel(session)} · observation time unknown`
    : `${pulseSessionStatusLabel(session)} · last observed ${formatPulseAge(age)} ago`;
}

/**
 * Session umbrellas grouped by independent lifecycle and reachability facts.
 *
 * A valid lease can prove provider reachability for one generation. An open
 * umbrella without that proof remains coordination-relevant, and durable
 * closure is the only fact that moves the umbrella into history.
 */
export function groupPulseSessions(digest: ProjectPulseDigest): {
  providerReachable: PulseDigestSession[];
  openUnverified: PulseDigestSession[];
  closed: PulseDigestSession[];
} {
  return {
    providerReachable: digest.sessions.filter(
      (session) => session.coordinationState === "provider_reachable",
    ),
    openUnverified: digest.sessions.filter(
      (session) => session.coordinationState === "open_unverified",
    ),
    closed: digest.sessions.filter(
      (session) => session.coordinationState === "closed",
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
 * Does this umbrella belong to the selected branch chip?
 *
 * Matches on **any** generation's branch, not just the display generation's.
 * The chip list is enumerated over every generation (`pulseDigestBranches`), so
 * matching the display branch alone would offer a chip that yields zero cards
 * whenever the branch belongs only to a superseded generation — a control that
 * lies about what it will show. This is the rule the shipped fold already
 * stated: filter executions first, then collapse, so a chip never hides the
 * execution the reader asked for.
 *
 * Both the visible rows and `countPulseBranchRows` call this one predicate;
 * that is what keeps a chip's count and its rows from drifting apart.
 */
export function pulseSessionMatchesBranch(
  session: PulseDigestSession,
  selected: string | null | undefined,
): boolean {
  return session.generations.some((generation) =>
    matchesBranchFilter(generation.branch, selected),
  );
}

/**
 * How many rows a branch chip would show: sessions plus **active** entries.
 *
 * Superseded entries are deliberately excluded — they sit behind a disclosure
 * whose own count is computed from the same filtered list, and a chip count
 * that included them would promise rows the screen does not show until asked.
 */
export function countPulseBranchRows(
  digest: ProjectPulseDigest,
  selected: string | null | undefined,
): number {
  const sessions = digest.sessions.filter((session) =>
    pulseSessionMatchesBranch(session, selected),
  ).length;
  const entries = digest.entries.filter(
    (entry) => entry.active && matchesBranchFilter(entry.branch, selected),
  ).length;
  return sessions + entries;
}

/** `1 entry` / `3 entries` / `no entries` — a count that reads as a sentence. */
export function formatPulseEntryCount(count: number): string {
  if (count === 0) return "no entries";
  return `${count} ${count === 1 ? "entry" : "entries"}`;
}

/** `1 session` / `12 sessions` / `no sessions` — umbrellas, not generations. */
export function formatPulseSessionCount(count: number): string {
  if (count === 0) return "no sessions";
  return `${count} ${count === 1 ? "session" : "sessions"}`;
}

/** `3 executions` — callers count distinct execution identities. */
export function formatPulseExecutionCount(count: number): string {
  return `${count} ${count === 1 ? "execution" : "executions"}`;
}

/** Whether a `Closed` chip would only repeat the selected status label. */
export function pulseSessionClosedIsRestated(
  session: PulseDigestSession,
): boolean {
  const status = pulseSessionDisplayGeneration(session)?.status ?? undefined;
  return (
    session.lifecycle === "closed" &&
    coordinationReportedStatus(status).label === "Ended"
  );
}
