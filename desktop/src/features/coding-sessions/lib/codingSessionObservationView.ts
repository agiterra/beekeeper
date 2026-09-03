/**
 * What the observer's screen shows, from the Rust fold and nothing else.
 *
 * This module **decides nothing about the wire**. It groups the native fold's
 * rows by their author, orders what carries a phase word in §1e's declared
 * order, bounds each list the rail can hold, and turns absences into the words
 * a reader can act on. Newest-wins, dedupe, the collection bounds and the
 * disclosures are `buzz-core`'s, already applied before anything here runs.
 *
 * Three rules it exists to keep:
 *
 * 1. **Declared order, never arrival and never author time** (§8 I4). A phase
 *    word is a property of the loop; when it happened to be published is not.
 * 2. **Unknown ≠ empty ≠ zero** (§8 I9). A `null` duration renders as
 *    `not reported`, never `0s`; a section with nothing in it says which
 *    nothing it means.
 * 3. **Provenance never merges** (the 2026-09-02 addendum). An `observed` row
 *    and a `declared` row are two rows; observed sorts first, and both carry
 *    the word.
 */
import {
  CODING_SESSION_OBSERVATION_PHASE_ORDER,
  type CodingSessionObservationFindingRow,
  type CodingSessionObservationFold,
  type CodingSessionObservationGateRow,
  type CodingSessionObservationSource,
} from "./codingSessionObservationWire";
import { truncatePubkey } from "@/shared/lib/pubkey";

/** Rows one seat block will show per section before it truncates visibly. */
export const CODING_SESSION_OBSERVATION_SECTION_LIMIT = 50;

/** Summary lines a gate row shows before `Show all` (L5.2). */
export const CODING_SESSION_GATE_SUMMARY_LINE_LIMIT = 8;

/**
 * Lines a gate row keeps at all.
 *
 * `Show all` lifts the eight-line fold; it does not remove the bound. A tail
 * longer than this still says how many lines are not shown, so §8 I10 holds in
 * both states rather than only in the folded one.
 */
export const CODING_SESSION_GATE_SUMMARY_LINE_CEILING = 200;

/** The words the four sections use when they hold nothing. Frozen copy. */
export const CODING_SESSION_OBSERVATION_EMPTY = Object.freeze({
  checkpoints: "No checkpoint yet",
  gates: "No gate row yet",
  findings: "No finding recorded",
  phases: "No phase timing",
});

/**
 * The one line a surface prints where a number is absent.
 *
 * The Audit tab's existing `—` / `not reported` pair, in words, so a row that
 * has no number to show and a row whose number is zero can never look alike.
 */
export const CODING_SESSION_OBSERVATION_NOT_REPORTED = "not reported";

export type CodingSessionObservationCheckpointView = {
  readonly key: string;
  readonly eventId: string;
  readonly source: CodingSessionObservationSource;
  readonly phase: string;
  readonly testsWritten: number;
  readonly testsRed: number;
  readonly testsGreen: number;
  readonly lastCommand: string | null;
  readonly lastSummary: string | null;
  readonly note: string | null;
  /** True when this row's `assignmentRef` resolved to nothing supplied. */
  readonly assignmentUnresolved: boolean;
};

export type CodingSessionObservationGateView = {
  readonly key: string;
  readonly authorPubkey: string;
  readonly source: CodingSessionObservationSource;
  readonly gate: string;
  readonly outcome: "passed" | "failed" | "not-run";
  /** The command that produced the outcome, verbatim. Never shortened. */
  readonly command: string;
  /**
   * The tail, split into lines and bounded at
   * {@link CODING_SESSION_GATE_SUMMARY_LINE_CEILING}. Empty when the row
   * published no summary at all.
   */
  readonly summaryLines: readonly string[];
  /**
   * Lines past the ceiling, dropped here and disclosed. Distinct from the
   * eight-line fold, which the component opens and closes.
   */
  readonly hiddenSummaryLines: number;
  /** The author's own measurement, already worded; null when absent. */
  readonly duration: string | null;
  /** The newest event this row was read from — what a reader can go and find. */
  readonly sourceEventId: string;
  /** Older statements about this gate not listed; 0 when none. */
  readonly droppedEventIds: number;
  readonly assignmentUnresolved: boolean;
};

export type CodingSessionObservationFindingView = {
  readonly key: string;
  readonly source: CodingSessionObservationSource;
  readonly findingId: string;
  readonly title: string;
  readonly disposition: string;
  readonly refCount: number;
  /** First 8 hex of the decision this finding waits on, or null. */
  readonly decisionShortRef: string | null;
  /** True when `decisionRef` names nothing this surface's fold resolved. */
  readonly decisionUnresolved: boolean;
  readonly sourceEventId: string;
  readonly droppedEventIds: number;
  readonly assignmentUnresolved: boolean;
};

export type CodingSessionObservationPhaseView = {
  readonly key: string;
  readonly eventId: string;
  readonly source: CodingSessionObservationSource;
  readonly phase: string;
  /** The author's own measured span, worded; null when it reported none. */
  readonly duration: string | null;
  /** True while the author says the phase has not ended. */
  readonly running: boolean;
  readonly assignmentUnresolved: boolean;
};

/** One author's block: everything this key signed, in four lists. */
export type CodingSessionObservationSeatBlock = {
  readonly key: string;
  readonly authorPubkey: string;
  /** Resolved display name, `You`, or the first 8 hex of the author. */
  readonly label: string;
  /**
   * True when every row in this block is `observed`.
   *
   * The block is then a **watcher's**, not a seat's: kind 44246 names the
   * mechanism that signed a row and has no field for the seat it watched, so
   * this surface says "watched by" rather than attributing the work.
   */
  readonly watcherOnly: boolean;
  readonly checkpoints: readonly CodingSessionObservationCheckpointView[];
  readonly gates: readonly CodingSessionObservationGateView[];
  readonly findings: readonly CodingSessionObservationFindingView[];
  readonly phases: readonly CodingSessionObservationPhaseView[];
  /** Per-section counts dropped by this surface's own bound; 0 when none. */
  readonly hidden: {
    readonly checkpoints: number;
    readonly gates: number;
    readonly findings: number;
    readonly phases: number;
  };
};

export type CodingSessionObservationTruncationNotice = {
  readonly id: string;
  readonly notice: string;
};

export type CodingSessionObservationView = {
  readonly seats: readonly CodingSessionObservationSeatBlock[];
  /** Every gate row across every author, observed first. One shared source. */
  readonly gates: readonly CodingSessionObservationGateView[];
  /** Pointers the fold could not resolve. Disclosed, never an exclusion. */
  readonly unresolved: readonly {
    readonly eventId: string;
    readonly shortEventId: string;
    readonly shortAssignmentRef: string;
  }[];
  /** Events the fold could not read at all, with the reason it gave. */
  readonly ignored: readonly {
    readonly eventId: string;
    readonly shortEventId: string;
    readonly reason: string;
  }[];
  readonly truncations: readonly CodingSessionObservationTruncationNotice[];
  /**
   * Rows that claimed `observed` without a provider instance behind them
   * (REVIEW-L5 F2). Each is rendered as `declared`; this is the disclosure.
   */
  readonly misclaimedObserved: readonly {
    readonly eventId: string;
    readonly shortEventId: string;
    readonly shortAuthor: string;
  }[];
  /**
   * Whether the fold verified provenance at all.
   *
   * `false` means no `observed` claim here was checked against the session's
   * provider instances — not that every one checked out.
   */
  readonly provenanceChecked: boolean;
  readonly disclosure: string;
};

/** Resolve an author key to the name a surface already shows for it. */
export type CodingSessionObservationLabelResolver = (
  pubkey: string,
) => string | null;

/**
 * Project one native fold onto the observer's screen.
 *
 * `resolveLabel` is the surface's own attribution, exactly as §1f's `{Who}`
 * defines it — this module never resolves an identity, because two surfaces
 * resolving it twice is how one of them ends up naming somebody else.
 */
export function deriveCodingSessionObservationView(input: {
  fold: CodingSessionObservationFold | null;
  resolveLabel: CodingSessionObservationLabelResolver;
  /** Decision request ids this surface's fold holds, for §1f resolution. */
  knownDecisionRefs?: readonly string[];
}): CodingSessionObservationView {
  const fold = input.fold;
  if (fold === null) {
    return Object.freeze({
      seats: Object.freeze([]),
      gates: Object.freeze([]),
      unresolved: Object.freeze([]),
      ignored: Object.freeze([]),
      truncations: Object.freeze([]),
      misclaimedObserved: Object.freeze([]),
      provenanceChecked: false,
      disclosure: "",
    });
  }
  const decisions = new Set(input.knownDecisionRefs ?? []);
  const unresolvedEvents = new Set(fold.unresolved.map((row) => row.eventId));

  const authors: string[] = [];
  const remember = (pubkey: string) => {
    if (!authors.includes(pubkey)) authors.push(pubkey);
  };
  for (const row of fold.checkpoints) remember(row.authorPubkey);
  for (const row of fold.gates) remember(row.authorPubkey);
  for (const row of fold.findings) remember(row.authorPubkey);
  for (const row of fold.phases) remember(row.authorPubkey);

  const allGates: CodingSessionObservationGateView[] = [];
  const seats = authors.map((authorPubkey) => {
    const checkpoints = fold.checkpoints
      .filter((row) => row.authorPubkey === authorPubkey)
      .map((row) => ({
        key: row.eventId,
        eventId: row.eventId,
        source: row.source,
        phase: row.phase,
        testsWritten: row.testsWritten,
        testsRed: row.testsRed,
        testsGreen: row.testsGreen,
        lastCommand: row.lastCommand,
        lastSummary: row.lastSummary,
        note: row.note,
        assignmentUnresolved: unresolvedEvents.has(row.eventId),
      }))
      .sort((left, right) => phaseRank(left.phase) - phaseRank(right.phase));
    const gates = fold.gates
      .filter((row) => row.authorPubkey === authorPubkey)
      .map(gateView)
      .sort(observedFirst);
    const findings = fold.findings
      .filter((row) => row.authorPubkey === authorPubkey)
      .map((row) => findingView(row, decisions, unresolvedEvents))
      .sort(observedFirst);
    const phases = fold.phases
      .filter((row) => row.authorPubkey === authorPubkey)
      .map((row) => ({
        key: row.eventId,
        eventId: row.eventId,
        source: row.source,
        phase: row.phase,
        duration: describeDuration(row.durationMs),
        running: row.endedAtMs === null,
        assignmentUnresolved: unresolvedEvents.has(row.eventId),
      }))
      .sort((left, right) => phaseRank(left.phase) - phaseRank(right.phase));
    allGates.push(...gates);
    const everyRowObserved = [
      ...checkpoints,
      ...gates,
      ...findings,
      ...phases,
    ].every((row) => row.source === "observed");
    return Object.freeze({
      key: authorPubkey,
      authorPubkey,
      // The canonical truncation, not §1f's eight-hex `{Who}`. A truncated
      // prefix is forgeable by vanity grinding, and the repo's own guard
      // (`desktop/scripts/check-pubkey-truncation.mjs`) exists to stop that
      // form fragmenting again — a block header naming an author is an
      // identity display, whatever the wake line's copy table says about a
      // sentence.
      label: input.resolveLabel(authorPubkey) ?? truncatePubkey(authorPubkey),
      watcherOnly: everyRowObserved,
      checkpoints: bound(checkpoints),
      gates: bound(gates),
      findings: bound(findings),
      phases: bound(phases),
      hidden: Object.freeze({
        checkpoints: hiddenCount(checkpoints),
        gates: hiddenCount(gates),
        findings: hiddenCount(findings),
        phases: hiddenCount(phases),
      }),
    });
  });

  return Object.freeze({
    seats: Object.freeze(seats),
    gates: Object.freeze([...allGates].sort(observedFirst)),
    unresolved: Object.freeze(
      fold.unresolved.map((row) =>
        Object.freeze({
          eventId: row.eventId,
          shortEventId: row.eventId.slice(0, 8),
          shortAssignmentRef: row.assignmentRef.slice(0, 8),
        }),
      ),
    ),
    ignored: Object.freeze(
      fold.ignored.map((row) =>
        Object.freeze({
          eventId: row.eventId,
          shortEventId: row.eventId.slice(0, 8),
          reason: row.reason,
        }),
      ),
    ),
    truncations: truncationNotices(fold, seats),
    misclaimedObserved: Object.freeze(
      fold.misclaimedObserved.map((row) =>
        Object.freeze({
          eventId: row.eventId,
          shortEventId: row.eventId.slice(0, 8),
          shortAuthor: truncatePubkey(row.authorPubkey),
        }),
      ),
    ),
    provenanceChecked: fold.provenanceChecked,
    disclosure: fold.disclosure,
  });
}

/** Observed before declared; otherwise the fold's own first-seen order. */
function observedFirst(
  left: { source: CodingSessionObservationSource },
  right: { source: CodingSessionObservationSource },
): number {
  return rankSource(left.source) - rankSource(right.source);
}

function rankSource(source: CodingSessionObservationSource): number {
  return source === "observed" ? 0 : 1;
}

/**
 * A phase word's position in §1e's declared order.
 *
 * A checkpoint's phase is a closed token and always ranks. A *phase timing*'s
 * name is the author's own free text, so one that is not a declared word sorts
 * after every one that is, in the fold's supplied order — placed honestly
 * rather than dropped or guessed at.
 */
function phaseRank(phase: string): number {
  const index = CODING_SESSION_OBSERVATION_PHASE_ORDER.indexOf(
    phase as (typeof CODING_SESSION_OBSERVATION_PHASE_ORDER)[number],
  );
  return index === -1 ? CODING_SESSION_OBSERVATION_PHASE_ORDER.length : index;
}

function gateView(
  row: CodingSessionObservationGateRow,
): CodingSessionObservationGateView {
  const lines =
    row.summary === null
      ? []
      : row.summary.split("\n").filter((line) => line.trim().length > 0);
  const newest = row.eventIds.at(-1) ?? "";
  return Object.freeze({
    key: `${row.authorPubkey}:${row.source}:${row.gate}`,
    authorPubkey: row.authorPubkey,
    source: row.source,
    gate: row.gate,
    outcome: row.outcome,
    command: row.command,
    summaryLines: Object.freeze(
      lines.slice(0, CODING_SESSION_GATE_SUMMARY_LINE_CEILING),
    ),
    hiddenSummaryLines: Math.max(
      0,
      lines.length - CODING_SESSION_GATE_SUMMARY_LINE_CEILING,
    ),
    duration: describeDuration(row.durationMs),
    sourceEventId: newest,
    droppedEventIds: row.droppedEventIds,
    assignmentUnresolved: false,
  });
}

function findingView(
  row: CodingSessionObservationFindingRow,
  decisions: ReadonlySet<string>,
  unresolvedEvents: ReadonlySet<string>,
): CodingSessionObservationFindingView {
  const newest = row.eventIds.at(-1) ?? "";
  return Object.freeze({
    key: `${row.authorPubkey}:${row.source}:${row.findingId}`,
    source: row.source,
    findingId: row.findingId,
    title: row.title,
    disposition: row.disposition,
    refCount: row.refs.length,
    decisionShortRef:
      row.decisionRef === null ? null : row.decisionRef.slice(0, 8),
    // §1f: a pointer this surface's fold does not hold is *unresolved*, which
    // is a different fact from absent, and never an exclusion.
    decisionUnresolved:
      row.decisionRef !== null && !decisions.has(row.decisionRef),
    sourceEventId: newest,
    droppedEventIds: row.droppedEventIds,
    assignmentUnresolved: unresolvedEvents.has(newest),
  });
}

/**
 * The author's own measured span, in words — or `null`.
 *
 * `null` renders as `not reported`, never as `0s`: a span nobody measured is
 * not a span of no time. A measured zero is `0s`, and that is a different
 * fact, so it prints.
 */
export function describeDuration(durationMs: number | null): string | null {
  if (durationMs === null) return null;
  const seconds = Math.round(durationMs / 1_000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return rest === 0 ? `${minutes}m` : `${minutes}m ${rest}s`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes === 0 ? `${hours}h` : `${hours}h ${restMinutes}m`;
}

function bound<T>(rows: readonly T[]): readonly T[] {
  return Object.freeze(rows.slice(0, CODING_SESSION_OBSERVATION_SECTION_LIMIT));
}

function hiddenCount(rows: readonly unknown[]): number {
  return Math.max(0, rows.length - CODING_SESSION_OBSERVATION_SECTION_LIMIT);
}

/**
 * Every bound that actually bit, in words.
 *
 * Two layers, and they are different facts: `buzz-core`'s own 512-entry
 * ceiling on each collection, and this rail's 50 rows per seat per section.
 * Both are disclosed rather than silently applied (§8 I10).
 */
function truncationNotices(
  fold: CodingSessionObservationFold,
  seats: readonly CodingSessionObservationSeatBlock[],
): readonly CodingSessionObservationTruncationNotice[] {
  const notices: CodingSessionObservationTruncationNotice[] = [];
  const fromFold: [string, number, string][] = [
    ["checkpoints", fold.truncated.checkpoints, "checkpoints"],
    ["gates", fold.truncated.gates, "gate rows"],
    ["findings", fold.truncated.findings, "findings"],
    ["phases", fold.truncated.phases, "phase timings"],
    ["unresolved", fold.truncated.unresolved, "unresolved pointers"],
    ["ignored", fold.truncated.ignored, "unreadable events"],
  ];
  for (const [id, dropped, noun] of fromFold) {
    if (dropped > 0) {
      notices.push({
        id: `fold-${id}`,
        notice: `${dropped} more ${noun} are on the wire and not in this fold.`,
      });
    }
  }
  // REVIEW-L5 F1: newest-wins is right, but the statement it replaced has to
  // be a fact a reader can see. A `failed` row replaced by a `passed` one must
  // never read like a gate that had only ever passed.
  if (fold.truncated.displacedGates > 0) {
    notices.push({
      id: "fold-displaced-gates",
      notice: `${fold.truncated.displacedGates} earlier gate statement${
        fold.truncated.displacedGates === 1 ? " was" : "s were"
      } replaced by a later one from the same author.`,
    });
  }
  if (fold.truncated.displacedFindings > 0) {
    notices.push({
      id: "fold-displaced-findings",
      notice: `${fold.truncated.displacedFindings} earlier finding${
        fold.truncated.displacedFindings === 1 ? " was" : "s were"
      } replaced by a later disposition from the same author.`,
    });
  }
  if (fold.truncated.misclaimedObserved > 0) {
    notices.push({
      id: "fold-misclaimed-observed",
      notice: `${fold.truncated.misclaimedObserved} more row${
        fold.truncated.misclaimedObserved === 1 ? "" : "s"
      } claimed to be observed and are not listed here.`,
    });
  }
  if (fold.truncated.entryEventIds > 0) {
    notices.push({
      id: "fold-entry-event-ids",
      notice: `${fold.truncated.entryEventIds} older statements are on the wire and not listed against their row.`,
    });
  }
  for (const seat of seats) {
    const hidden: [string, number, string][] = [
      ["checkpoints", seat.hidden.checkpoints, "checkpoints"],
      ["gates", seat.hidden.gates, "gate rows"],
      ["findings", seat.hidden.findings, "findings"],
      ["phases", seat.hidden.phases, "phase timings"],
    ];
    for (const [id, dropped, noun] of hidden) {
      if (dropped > 0) {
        notices.push({
          id: `seat-${seat.key}-${id}`,
          notice: `${seat.label}: ${dropped} more ${noun} not shown here.`,
        });
      }
    }
  }
  return Object.freeze(notices);
}
