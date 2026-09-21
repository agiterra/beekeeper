/**
 * Declared work in Project Pulse: one pure projection over what was read.
 *
 * A declaration is either a **plan** the digest already folded (kind 44240,
 * `type: "plan"`, still active) or an **assignment** the canonical team fold
 * canonically included (kind 44244, projected natively by
 * `pulse_declared_work`). This module joins the two into one newest-first list
 * and writes nothing it was not handed.
 *
 * What it must never do, and what its tests pin:
 *
 * - **Fetch.** It takes loaded pages and a digest. A row never triggers a read
 *   on hover, scroll or disclosure.
 * - **Infer.** A missing branch, base or declared path is rendered as missing.
 *   `null` stays `null` so the surface can say "not reported".
 * - **Compare scopes.** `fileOwnership` is carried verbatim and is never
 *   matched against another declaration's paths, another branch, or a reported
 *   changed file. A project can hold several repositories and none of these
 *   strings carries a repository identity, so an overlap here would be a guess
 *   wearing a badge (`docs/WORK_COORDINATION_VISIBILITY_SPEC.md` § Scope
 *   comparisons).
 * - **Settle anything.** Settlement comes from the fold's own
 *   `settlement.settled` and nothing else. A closed session is *evidence*:
 *   closing an execution settles no assignment, so an unsettled assignment in
 *   a closed session stays in `current`.
 * - **Say "no declared work" over a read that failed.** A page error, an
 *   unreadable session, or a capped scan each keeps that sentence off the
 *   screen and puts a limitation on it instead.
 *
 * Contract: `docs/DECLARED_WORK_PULSE_IMPL.md` §5.
 */
import type {
  PulseDeclaredAssignment,
  PulseDeclaredDisposition,
  PulseDeclaredReport,
  PulseDeclaredWorkResponse,
  PulseDeclaredWorkSession,
} from "./pulseDeclaredWorkWire";
import { truncatePubkey } from "@/shared/lib/pubkey";

import { formatPulseAge } from "./pulseFormat";
import type { ProjectPulseDigest, PulseDigestEntry } from "./pulseFoldTypes";
import {
  PULSE_DECLARED_WORK_MAX_PAGES,
  PULSE_DECLARED_WORK_PAGE_SIZE,
} from "./pulseMissionSessionRead";

/** One page of the scan that could not be read at all. */
export type PulseDeclaredWorkPageError = {
  readonly pageIndex: number;
  readonly message: string;
};

/**
 * One page as the projection reads it: what came back, and what was asked for.
 *
 * Both halves are required. A session whose 44244 read failed is **absent**
 * from `response.sessions` (the gather refuses to send records it could not
 * read completely), so without the asked-for keys the difference between "not
 * reached yet" and "reached and unreadable" disappears — and the first offers
 * a "Show older sessions" control that would never surface it.
 */
export type PulseDeclaredWorkModelPage = {
  readonly response: PulseDeclaredWorkResponse;
  /** The session keys this page asked about, in the order it asked. */
  readonly sessionKeys: readonly string[];
};

/** Everything the projection consumes. Nothing here is fetched. */
export type PulseDeclaredWorkInput = {
  /** The folded digest, for its active plans. `null` before the first fold. */
  readonly digest: ProjectPulseDigest | null;
  /** The pages that came back, in load order, with what each one asked for. */
  readonly pages: readonly PulseDeclaredWorkModelPage[];
  /** The pages that did not. Their sessions are unscanned, not empty. */
  readonly pageErrors?: readonly PulseDeclaredWorkPageError[];
  /** How many sessions the digest proved visible, before any page was read. */
  readonly visibleSessionCount: number;
  /** How many pages came back. A page that failed is in `pageErrors`. */
  readonly loadedPageCount: number;
  readonly maxPages?: number;
  readonly pageSize?: number;
  /** Seconds since the epoch, read once — every age below is measured to it. */
  readonly nowSeconds: number;
  readonly viewerPubkey?: string | null;
};

/** The closed label vocabulary for an assignment's evidence lines. */
export type PulseDeclaredWorkEvidenceLabel =
  | "Report submitted"
  | "Disposition"
  | "Settled"
  | "Mission completed"
  | "Mission blocked"
  | "Session closed";

/** One evidence line: a label the fold established, and what it says. */
export type PulseDeclaredWorkEvidence = {
  readonly label: PulseDeclaredWorkEvidenceLabel;
  readonly detail: string;
  readonly eventId: string | null;
};

/** The session an assignment row belongs to, as the projection saw it. */
export type PulseDeclaredWorkRowSession = {
  readonly sessionKey: string;
  readonly sessionRef: string;
  /**
   * The umbrella's immutable genesis event id, carried verbatim.
   *
   * Not decoration: the session ref is author-chosen and the genesis record is
   * what actually names the channel and the founder this row's 44244 set was
   * proven under. The disclosure block shows both, so a reader can check the
   * row against the record rather than against a pointer.
   */
  readonly genesisRef: string;
  readonly channelId: string;
  readonly name: string | null;
  readonly lifecycle: "open" | "closed";
  readonly latestObservationAt: number | null;
  readonly founderPubkey: string;
};

/** One row of the section: a posted plan, or a canonically included assignment. */
export type PulseDeclaredWorkRow =
  | {
      readonly kind: "plan";
      readonly label: "Plan posted";
      readonly entry: PulseDigestEntry;
      readonly createdAt: number;
      readonly dedupeKey: string;
    }
  | {
      readonly kind: "assignment";
      readonly label: "Assigned";
      readonly session: PulseDeclaredWorkRowSession;
      readonly assignment: PulseDeclaredAssignment;
      readonly evidence: readonly PulseDeclaredWorkEvidence[];
      /** The assigned actor: the participant responsible for the work. */
      readonly responsible: { readonly pubkey: string; readonly role: string };
      /** The assigning author, kept inspectable rather than folded away. */
      readonly assignedBy: string;
      readonly createdAt: number;
      readonly dedupeKey: string;
    };

/** How far the scan got, and how it says so. */
export type PulseDeclaredWorkScan = {
  readonly visibleSessions: number;
  readonly scannedSessions: number;
  readonly unreadableSessions: number;
  /**
   * Sessions a page asked about and did not get back: their records could not
   * be read, so the gather did not send them.
   *
   * Kept apart from the sessions no page has reached yet. Folding the two
   * would put a failed read behind a "Show older sessions" control that will
   * never show it, and would let the count of unread sessions shrink on a
   * retry that read nothing.
   */
  readonly droppedSessions: number;
  readonly morePages: boolean;
  readonly capped: boolean;
  readonly sentence: string;
};

/** What the section renders. It is handed this and folds nothing further. */
export type PulseDeclaredWorkModel = {
  readonly current: readonly PulseDeclaredWorkRow[];
  readonly settled: readonly PulseDeclaredWorkRow[];
  readonly scan: PulseDeclaredWorkScan;
  readonly limitations: readonly string[];
  /**
   * Whether this read covered its stated scope: every visible session was
   * reached, its records were readable, no page failed, and the cap was not
   * hit.
   *
   * Separate from {@link PulseDeclaredWorkModel.noDeclaredWork} because a
   * complete read that found only settled work is still complete — reporting
   * it as incomplete would put "the read is incomplete" over a section that
   * is showing everything there is.
   */
  readonly readIsComplete: boolean;
  /**
   * Whether "No declared work" is an answer this read has earned.
   *
   * True only for a successful, complete-within-its-stated-scope empty read:
   * every attempted page came back, no session's records were unreadable, the
   * scan was not capped, and nothing was declared.
   */
  readonly noDeclaredWork: boolean;
};

/** `4m ago`, or `just now` — never `just now ago`. */
function agePhrase(nowSeconds: number, at: number): string {
  const age = formatPulseAge(Math.max(0, nowSeconds - at));
  return age === "just now" ? "just now" : `${age} ago`;
}

/**
 * A commit short enough to sit in a sentence: the first eight characters.
 *
 * Exported so the section renders a base sha and a head sha the same width —
 * two conventions for the same kind of identifier is how a reader starts
 * believing they name different kinds of thing.
 */
export function shortSha(sha: string): string {
  return sha.length > 8 ? sha.slice(0, 8) : sha;
}

/** The decision word, exactly as the closed disposition vocabulary spells it. */
function decisionWord(decision: PulseDeclaredDisposition["decision"]): string {
  switch (decision) {
    case "approve":
      return "Approved";
    case "approve-with-notes":
      return "Approved with notes";
    case "changes-requested":
      return "Changes requested";
    case "reject":
      return "Rejected";
    case "blocked":
      return "Blocked";
  }
}

/**
 * One report's evidence line.
 *
 * A report is evidence that a report was submitted — never that the work is
 * done. The sentence says who submitted what and when, and discloses an author
 * the fold found holding no seat for the role they reported into.
 */
function reportEvidence(
  report: PulseDeclaredReport,
  assignment: PulseDeclaredAssignment,
  nowSeconds: number,
): PulseDeclaredWorkEvidence {
  const parts = [agePhrase(nowSeconds, report.createdAt)];
  if (report.summary !== "") parts.push(report.summary);
  if (report.headSha !== null && report.headSha !== "") {
    parts.push(`head ${shortSha(report.headSha)}`);
  }
  if (report.authorUnseated) {
    parts.push(`author holds no seat for ${assignment.assigneeRole}`);
  }
  return {
    label: "Report submitted",
    detail: parts.join(" · "),
    eventId: report.eventId,
  };
}

/** Every evidence line for one assignment, oldest fact first. */
function assignmentEvidence(
  assignment: PulseDeclaredAssignment,
  session: PulseDeclaredWorkSession,
  nowSeconds: number,
): PulseDeclaredWorkEvidence[] {
  const evidence: PulseDeclaredWorkEvidence[] = [];
  for (const report of assignment.reports) {
    evidence.push(reportEvidence(report, assignment, nowSeconds));
  }
  for (const disposition of assignment.dispositions) {
    evidence.push({
      label: "Disposition",
      // The app's one compact display form. A truncated pubkey is a
      // recognition aid and never an identity proof, so it is not hand-rolled
      // here into a second shape a reader would have to learn.
      detail: `${decisionWord(disposition.decision)} by ${truncatePubkey(disposition.authorPubkey)} · ${agePhrase(nowSeconds, disposition.createdAt)}`,
      eventId: disposition.eventId,
    });
  }
  if (assignment.settlement.settled) {
    evidence.push({
      label: "Settled",
      // Lane 210: which rule settled it is the fold's answer, carried
      // verbatim. Saying "the acknowledgement is on the wire" over an
      // approval that asked for nothing — and got no receipt, correctly —
      // would be the surface asserting a record that does not exist.
      detail:
        assignment.settlement.settledBy === "approving_disposition_without_ask"
          ? "An approving disposition that asks the assignee for nothing. No acknowledgement is owed, and none was published."
          : "An approving disposition and the assignee's acknowledgement are both on the wire.",
      eventId:
        assignment.settlement.dispositionEventId ??
        assignment.settlement.acknowledgementEventId,
    });
  }
  if (session.terminal !== null) {
    evidence.push({
      label:
        session.terminal.type === "mission.completed"
          ? "Mission completed"
          : "Mission blocked",
      detail: `This session's canonical terminal was recorded ${agePhrase(nowSeconds, session.terminal.at)}.`,
      eventId: session.terminal.eventId,
    });
  }
  // Closing an execution settles nothing. The closed lifecycle is disclosed as
  // evidence beside an assignment that is still unresolved, and never as a
  // reason to move it out of `current`.
  if (session.lifecycle === "closed" && !assignment.settlement.settled) {
    evidence.push({
      label: "Session closed",
      detail:
        "This session is closed. Closing settles nothing, so this assignment is still unresolved.",
      eventId: null,
    });
  }
  return evidence;
}

/** The row session shape, copied rather than referenced into the response. */
function rowSession(
  session: PulseDeclaredWorkSession,
): PulseDeclaredWorkRowSession {
  return {
    sessionKey: session.sessionKey,
    sessionRef: session.sessionRef,
    genesisRef: session.genesisRef,
    channelId: session.channelId,
    name: session.name,
    lifecycle: session.lifecycle,
    latestObservationAt: session.latestObservationAt,
    founderPubkey: session.founderPubkey,
  };
}

/** A session by name when it has one, else by its ref — never by a guess. */
function sessionLabel(session: PulseDeclaredWorkSession): string {
  return session.name ?? `session ${session.sessionRef}`;
}

function byteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** Newest first; a tie is broken on the dedupe key so two paints agree. */
function newestFirst(
  left: PulseDeclaredWorkRow,
  right: PulseDeclaredWorkRow,
): number {
  if (left.createdAt !== right.createdAt)
    return right.createdAt - left.createdAt;
  return byteOrder(left.dedupeKey, right.dedupeKey);
}

/** `N older sessions not read yet`, singular when there is one. */
function olderSessions(count: number): string {
  return count === 1
    ? "1 older session not read yet"
    : `${count} older sessions not read yet`;
}

/**
 * The scan sentence: what was read, out of what, and — in its own clause each
 * — what could not be read, what was unreadable, and what is simply not
 * reached yet.
 *
 * Three different failures, three different clauses, on purpose. "Not read
 * yet" has a control behind it; "could not be read" does not; "records could
 * not be read" means the session was reached and its 44244 set would not
 * fold. Collapsing any pair of them tells a reader to press a button that
 * cannot help, or hides a failure inside a number that looks like progress.
 */
function scanSentence(input: {
  visibleSessions: number;
  scannedSessions: number;
  droppedSessions: number;
  unreadableSessions: number;
  remaining: number;
  capped: boolean;
  scanLimit: number;
}): string {
  if (input.visibleSessions === 0) {
    return "No sessions are visible in this project's channels to scan.";
  }
  const clauses: string[] = [];
  if (input.capped) {
    clauses.push(`the read stops at ${input.scanLimit} sessions`);
  } else if (input.remaining > 0) {
    clauses.push(olderSessions(input.remaining));
  }
  if (input.droppedSessions > 0) {
    clauses.push(`${input.droppedSessions} could not be read`);
  }
  if (input.unreadableSessions > 0) {
    clauses.push(
      input.unreadableSessions === 1
        ? "1 session's records could not be read"
        : `${input.unreadableSessions} sessions' records could not be read`,
    );
  }
  if (clauses.length === 0) {
    return input.visibleSessions === 1
      ? "Scanned the 1 visible session."
      : `Scanned all ${input.scannedSessions} visible sessions.`;
  }
  const noun = input.visibleSessions === 1 ? "session" : "sessions";
  const base = `Scanned ${input.scannedSessions} of ${input.visibleSessions} visible ${noun}, newest first`;
  return `${base}; ${clauses.join("; ")}.`;
}

/**
 * Project the loaded pages and the digest into one declared-work model.
 *
 * Pure: the same input gives the same rows, the same sentence and the same
 * limitations, and nothing outside `input.pages` and `input.digest.entries`
 * can reach the output — a row for another project's session cannot appear
 * because no other source is read.
 */
export function projectPulseDeclaredWork(
  input: PulseDeclaredWorkInput,
): PulseDeclaredWorkModel {
  const pageSize = input.pageSize ?? PULSE_DECLARED_WORK_PAGE_SIZE;
  const maxPages = input.maxPages ?? PULSE_DECLARED_WORK_MAX_PAGES;
  const pageErrors = input.pageErrors ?? [];
  const limitations: string[] = [];

  // Plans first: already folded, already deduped by the digest's supersession
  // rule. A superseded plan is `active: false` and stays in the existing
  // superseded disclosure rather than appearing here a second time.
  const rows = new Map<string, PulseDeclaredWorkRow>();
  for (const entry of input.digest?.entries ?? []) {
    if (entry.type !== "plan" || !entry.active) continue;
    const dedupeKey = `plan:${entry.eventId}`;
    rows.set(dedupeKey, {
      kind: "plan",
      label: "Plan posted",
      entry,
      createdAt: entry.createdAt,
      dedupeKey,
    });
  }

  const scannedSessionKeys = new Set<string>();
  const askedSessionKeys = new Set<string>();
  const unreadableScopes = new Set<string>();
  let unreadableSessions = 0;

  for (const loaded of input.pages) {
    const page = loaded.response;
    for (const key of loaded.sessionKeys) askedSessionKeys.add(key);
    for (const session of page.sessions) {
      scannedSessionKeys.add(session.sessionKey);
      if (session.unreadable !== null) {
        unreadableSessions += 1;
        unreadableScopes.add(`declared:${session.sessionKey}`);
        limitations.push(
          `The records of ${sessionLabel(session)} could not be read: ${session.unreadable}`,
        );
        continue;
      }
      if (session.excludedCount > 0) {
        limitations.push(
          session.excludedCount === 1
            ? `1 record in ${sessionLabel(session)} was excluded by the canonical fold and is not shown.`
            : `${session.excludedCount} records in ${sessionLabel(session)} were excluded by the canonical fold and are not shown.`,
        );
      }
      for (const assignment of session.assignments) {
        // Channel, session and source event. Community and project are already
        // fixed by the query key this page was read under.
        const dedupeKey = `${session.channelId}:${session.sessionRef}:${assignment.sourceEventId}`;
        const existing = rows.get(dedupeKey);
        if (existing && existing.createdAt >= assignment.createdAt) continue;
        rows.set(dedupeKey, {
          kind: "assignment",
          label: "Assigned",
          session: rowSession(session),
          assignment,
          evidence: assignmentEvidence(assignment, session, input.nowSeconds),
          responsible: {
            pubkey: assignment.assigneeActor,
            role: assignment.assigneeRole,
          },
          assignedBy: assignment.assignerPubkey,
          createdAt: assignment.createdAt,
          dedupeKey,
        });
      }
    }
    for (const error of page.errors) {
      // An unreadable session already carries its own sentence above; the
      // command mirrors it into `errors` and printing both says it twice.
      if (unreadableScopes.has(error.scope)) continue;
      limitations.push(`${error.scope}: ${error.message}`);
    }
  }

  for (const pageError of pageErrors) {
    limitations.push(
      `Page ${pageError.pageIndex + 1} of the session scan could not be read: ${pageError.message}`,
    );
  }

  const scanLimit = maxPages * pageSize;
  const scannedSessions = scannedSessionKeys.size;
  // Asked for and not returned: the gather refuses to send a session whose
  // records it could not read completely, so the difference is exactly the
  // sessions whose read failed. Its cause is already in `limitations` — this
  // is the count the scan sentence needs to keep it out of "not read yet".
  let droppedSessions = 0;
  for (const key of askedSessionKeys) {
    if (!scannedSessionKeys.has(key)) droppedSessions += 1;
  }
  const remaining = Math.max(
    0,
    input.visibleSessionCount - scannedSessions - droppedSessions,
  );
  const attemptedPages = Math.max(0, input.loadedPageCount);
  const morePages =
    attemptedPages * pageSize < input.visibleSessionCount &&
    attemptedPages < maxPages;
  const capped =
    attemptedPages >= maxPages &&
    attemptedPages * pageSize < input.visibleSessionCount;
  if (capped) {
    limitations.push(
      `This scan reads at most ${scanLimit} sessions; older sessions were not read.`,
    );
  }

  // Every visible session reached, every one of them readable, no page lost,
  // and the cap not hit. `remaining` carries the pages nobody has asked for
  // yet — a read that has not asked cannot claim to have covered its scope,
  // which is the completeness the spec forbids advertising early.
  const readIsComplete =
    pageErrors.length === 0 &&
    unreadableSessions === 0 &&
    droppedSessions === 0 &&
    remaining === 0 &&
    !capped;

  const ordered = [...rows.values()].sort(newestFirst);
  const current = ordered.filter(
    (row) => row.kind === "plan" || !row.assignment.settlement.settled,
  );
  const settled = ordered.filter(
    (row) => row.kind === "assignment" && row.assignment.settlement.settled,
  );

  return {
    current,
    settled,
    scan: {
      visibleSessions: input.visibleSessionCount,
      scannedSessions,
      unreadableSessions,
      morePages,
      capped,
      droppedSessions,
      sentence: scanSentence({
        visibleSessions: input.visibleSessionCount,
        scannedSessions,
        droppedSessions,
        unreadableSessions,
        remaining,
        capped,
        scanLimit,
      }),
    },
    limitations,
    readIsComplete,
    // A complete read that found nothing. Both halves are needed: an empty
    // list over an incomplete read is "we did not look", and a complete read
    // that found only settled work is not empty at all.
    noDeclaredWork:
      readIsComplete && current.length === 0 && settled.length === 0,
  };
}
