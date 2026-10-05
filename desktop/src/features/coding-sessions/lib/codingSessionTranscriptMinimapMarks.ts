/**
 * The minimap's marks (SV-27): what happened inside each turn's window.
 *
 * A turn's window runs from its start to the next minimap turn's start; the
 * newest turn's window stays open. A signed fact — a gate row (kind 44246),
 * a handover record (kind 44247), an open ruling (DB8: a `decision.request`
 * on kind 44244 still waiting on a person) — lands on the turn whose window
 * holds its signed time. A fact signed before the first turn, or a turn
 * nothing dates, gets no mark: the mark says "this happened during this
 * turn", and a guess would make that false.
 *
 * Hues are DB12's state hues only: destructive for a failed turn, amber for a
 * waiting ruling. Gate and handover marks are glyphs, never colours, so
 * identity (the dash) and state (the mark) never share a hue.
 *
 * The rewind-point mark belongs to SV-29 (CHECKPOINTS slice S4c adds it here).
 */

export type CodingSessionMinimapGateOutcome = "passed" | "failed" | "not-run";

export type CodingSessionMinimapGateInput = {
  readonly gate: string;
  readonly outcome: CodingSessionMinimapGateOutcome;
  /** Signed time in ms; null when the fold carried none. */
  readonly signedAtMs: number | null;
};

export type CodingSessionMinimapHandoverInput = {
  readonly type: "checkpoint" | "continuation";
  readonly signedAtMs: number;
};

/**
 * One open `decision.request` (DB8) — never an open assignment or a report
 * awaiting a verdict. Build these with
 * {@link codingSessionMinimapOpenDecisionRulings}, not from open holds.
 */
export type CodingSessionMinimapRulingInput = {
  /** The request's signed `created_at`, in ms; null when nothing dates it. */
  readonly sinceAtMs: number | null;
};

/** A signed kind-44244 row, as much of it as the ruling selector reads. */
export type CodingSessionMinimapTransactionRow = {
  readonly sourceEventId: string;
  readonly type: string;
  /** Unix seconds from the signed event. */
  readonly createdAt: number;
};

/** One row of the Rust fold's `decisions[]`: open while `answeredBy` is null. */
export type CodingSessionMinimapDecisionState = {
  readonly requestId: string;
  readonly answeredBy: string | null;
};

/**
 * The waiting rulings the minimap marks: the open `decision.request` rows
 * (DB8), dated by each request's own signed `created_at`.
 *
 * Only a `decision.request` waits on a person. An open assignment is a seat
 * working and a report owed a verdict is not a ruling request, so neither
 * ever becomes an amber mark. Open-ness is the fold's word (`answeredBy`
 * null), never inferred here: a request the fold does not list is not
 * marked, because unknown is not "waiting".
 */
export function codingSessionMinimapOpenDecisionRulings(input: {
  transactions: readonly CodingSessionMinimapTransactionRow[];
  decisions: readonly CodingSessionMinimapDecisionState[];
}): CodingSessionMinimapRulingInput[] {
  const open = new Set(
    input.decisions
      .filter((decision) => decision.answeredBy === null)
      .map((decision) => decision.requestId),
  );
  return input.transactions
    .filter(
      (row) => row.type === "decision.request" && open.has(row.sourceEventId),
    )
    .map((row) => ({
      sinceAtMs: Number.isFinite(row.createdAt) ? row.createdAt * 1_000 : null,
    }));
}

/** The signed rows the waiting-ruling mark is built from (DB8). */
export type CodingSessionMinimapOpenDecisionSource = {
  transactions: readonly CodingSessionMinimapTransactionRow[];
  decisions: readonly CodingSessionMinimapDecisionState[];
};

/**
 * The surface ctx fields the waiting-ruling mark reads: the fold's
 * `decisions[]` (open while `answeredBy` is null) and `decisionRequests`,
 * the Mission transactions' signed `decision.request` rows that date them.
 * A ctx without `decisionRequests` reads as "requests not read".
 */
export type CodingSessionMinimapDecisionReads = {
  readonly decisions: readonly CodingSessionMinimapDecisionState[] | null;
  readonly decisionRequests?:
    | readonly CodingSessionMinimapTransactionRow[]
    | null;
};

/**
 * Where the waiting-ruling mark reads its open `decision.request` rows.
 *
 * Deliberately not `ctx.openRulings`: those are open *holds* — an open
 * assignment is a seat working, a report owed a verdict is not a ruling
 * request — and drawing them amber said "waiting on a ruling" over a team
 * that was simply working. Open-ness is the fold's `decisions[]`
 * (`answeredBy` null); the signed request rows date each one. Null when
 * either is unread: a missing amber mark over an unread source is "not
 * known", never "none".
 */
export function codingSessionMinimapOpenDecisionSource(
  ctx: CodingSessionMinimapDecisionReads | null,
): CodingSessionMinimapOpenDecisionSource | null {
  if (ctx === null) return null;
  const { decisions } = ctx;
  const transactions = ctx.decisionRequests ?? null;
  if (decisions === null || transactions === null) return null;
  return { transactions, decisions };
}

/**
 * The card's disclosure for the ruling source, or null when it was read in
 * full. Unread fold → "not read". Fold read but the request rows that date
 * an open request not carried → says how many open requests the minimap
 * cannot place, so an absent amber mark never reads as "none waiting".
 */
export function codingSessionMinimapRulingNote(
  ctx: CodingSessionMinimapDecisionReads | null,
): string | null {
  if (ctx === null || ctx.decisions === null) {
    return "Open ruling requests are not read in this view";
  }
  const transactions = ctx.decisionRequests ?? null;
  const open = ctx.decisions.filter((decision) => decision.answeredBy === null);
  const dated =
    transactions === null
      ? new Set<string>()
      : new Set(
          transactions
            .filter(
              (row) =>
                row.type === "decision.request" &&
                Number.isFinite(row.createdAt),
            )
            .map((row) => row.sourceEventId),
        );
  const undated = open.filter((decision) => !dated.has(decision.requestId));
  if (undated.length === 0) return null;
  return undated.length === 1
    ? "1 open ruling request is not marked: its signed request was not read in this view"
    : `${undated.length} open ruling requests are not marked: their signed requests were not read in this view`;
}

export type CodingSessionMinimapTurnWindowInput = {
  readonly id: string;
  readonly startedAtMs: number | null;
  readonly failed: boolean;
};

/** Every gate row inside one turn's window, newest last. */
export type CodingSessionMinimapGateMark = {
  readonly passed: number;
  readonly failed: number;
  readonly notRun: number;
  /** ✗ when any failed, ✓ when any passed and none failed, else null. */
  readonly glyph: "pass" | "fail" | null;
  readonly gates: readonly {
    gate: string;
    outcome: CodingSessionMinimapGateOutcome;
  }[];
};

export type CodingSessionMinimapMarks = {
  readonly failed: boolean;
  readonly gate: CodingSessionMinimapGateMark | null;
  readonly handovers: number;
  readonly waitingRulings: number;
};

export const CODING_SESSION_MINIMAP_NO_MARKS: CodingSessionMinimapMarks =
  Object.freeze({ failed: false, gate: null, handovers: 0, waitingRulings: 0 });

type Window = { id: string; start: number; end: number };

/** The dated turns' windows, in start order. */
function turnWindows(
  turns: readonly CodingSessionMinimapTurnWindowInput[],
): Window[] {
  const dated = turns
    .filter(
      (
        turn,
      ): turn is CodingSessionMinimapTurnWindowInput & {
        startedAtMs: number;
      } => turn.startedAtMs !== null && Number.isFinite(turn.startedAtMs),
    )
    .map((turn, index) => ({ turn, index }))
    .sort(
      (left, right) =>
        left.turn.startedAtMs - right.turn.startedAtMs ||
        left.index - right.index,
    )
    .map(({ turn }) => turn);
  return dated.map((turn, index) => ({
    id: turn.id,
    start: turn.startedAtMs,
    end: dated[index + 1]?.startedAtMs ?? Number.POSITIVE_INFINITY,
  }));
}

/** The window holding `at`, by binary search; null before the first. */
function windowAt(windows: readonly Window[], at: number): Window | null {
  let low = 0;
  let high = windows.length - 1;
  let found: Window | null = null;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const candidate = windows[middle];
    if (candidate === undefined) break;
    if (candidate.start <= at) {
      found = candidate;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found !== null && at < found.end ? found : null;
}

type MutableMarks = {
  failed: boolean;
  gates: { gate: string; outcome: CodingSessionMinimapGateOutcome }[];
  handovers: number;
  waitingRulings: number;
};

/** Each turn's marks, keyed by the turn's id. Every turn has an entry. */
export function deriveCodingSessionMinimapMarks(input: {
  turns: readonly CodingSessionMinimapTurnWindowInput[];
  gates?: readonly CodingSessionMinimapGateInput[];
  handovers?: readonly CodingSessionMinimapHandoverInput[];
  rulings?: readonly CodingSessionMinimapRulingInput[];
}): ReadonlyMap<string, CodingSessionMinimapMarks> {
  const marks = new Map<string, MutableMarks>();
  for (const turn of input.turns) {
    marks.set(turn.id, {
      failed: turn.failed,
      gates: [],
      handovers: 0,
      waitingRulings: 0,
    });
  }
  const windows = turnWindows(input.turns);
  const place = (at: number | null): MutableMarks | null => {
    if (at === null || !Number.isFinite(at)) return null;
    const window = windowAt(windows, at);
    return window === null ? null : (marks.get(window.id) ?? null);
  };
  const gates = [...(input.gates ?? [])].sort(
    (left, right) => (left.signedAtMs ?? 0) - (right.signedAtMs ?? 0),
  );
  for (const gate of gates) {
    place(gate.signedAtMs)?.gates.push({
      gate: gate.gate,
      outcome: gate.outcome,
    });
  }
  for (const handover of input.handovers ?? []) {
    const target = place(handover.signedAtMs);
    if (target) target.handovers += 1;
  }
  for (const ruling of input.rulings ?? []) {
    const target = place(ruling.sinceAtMs);
    if (target) target.waitingRulings += 1;
  }
  const result = new Map<string, CodingSessionMinimapMarks>();
  for (const [id, mutable] of marks) {
    result.set(id, {
      failed: mutable.failed,
      gate: summarizeGates(mutable.gates),
      handovers: mutable.handovers,
      waitingRulings: mutable.waitingRulings,
    });
  }
  return result;
}

function summarizeGates(
  gates: readonly { gate: string; outcome: CodingSessionMinimapGateOutcome }[],
): CodingSessionMinimapGateMark | null {
  if (gates.length === 0) return null;
  const count = (outcome: CodingSessionMinimapGateOutcome) =>
    gates.filter((gate) => gate.outcome === outcome).length;
  const passed = count("passed");
  const failed = count("failed");
  return {
    passed,
    failed,
    notRun: count("not-run"),
    glyph: failed > 0 ? "fail" : passed > 0 ? "pass" : null,
    gates,
  };
}

/** The card's gate line for one turn's window. */
export function codingSessionMinimapGateLine(
  gate: CodingSessionMinimapGateMark | null,
): string {
  if (gate === null) return "No gate row signed during this turn";
  const parts = [
    gate.passed > 0 ? `${gate.passed} passed` : null,
    gate.failed > 0 ? `${gate.failed} failed` : null,
    gate.notRun > 0 ? `${gate.notRun} not run` : null,
  ].filter((part): part is string => part !== null);
  return `Gates: ${parts.join(", ")}`;
}
