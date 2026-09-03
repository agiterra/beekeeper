/**
 * The decision queue and the waiting-on-a-person state, split out of
 * `codingSessionMissionInspectorModel.ts` so neither file passes 1,000 lines.
 *
 * Everything here reads the Rust fold's own `decisions[]` and
 * `waitingOnDecision` verbatim; the only local judgement is the liveness
 * qualifier, which is a fact about executions the fold has never seen.
 */
import { truncatePubkey } from "@/shared/lib/pubkey";
import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";

/**
 * One row of the Rust fold's own `decisions[]`, verbatim.
 *
 * `heldOn` is exactly `"founder"` or a 64-hex actor pubkey — the wire's two
 * spellings, and the only two this surface renders. `answeredBy` and
 * `answerId` are both null exactly while the question stands open, so an
 * unanswered request can never be mistaken for a settled one.
 */
export type CodingSessionMissionDecisionInput = {
  requestId: string;
  heldOn: string;
  blocks: readonly string[];
  answeredBy: string | null;
  answerId: string | null;
};

/** The fold's `waitingOnDecision`, verbatim. TypeScript never computes it. */
export type CodingSessionMissionWaitingOnDecisionInput = {
  requestId: string;
  heldOn: string;
};

/** How many decision rows the queue shows before it says what it dropped. */
export const MAX_CODING_SESSION_MISSION_DECISION_ROWS = 50;

/** One decision, in the words a queue row prints. */
export type CodingSessionMissionDecisionModel = {
  requestId: string;
  /** First eight hex of the request id — the id form §1f and §1g freeze. */
  shortId: string;
  /** The signed question, when the fold included the request row. */
  question: string | null;
  state: "open" | "answered";
  /** `Open · held on …` or `Answered by …` — a word, never a colour (I9). */
  stateWord: string;
  /**
   * What the request holds up, always said in words.
   *
   * `blocks: []` is a real answer from the fold, not a gap, so it reads
   * `holds up no assignment yet` rather than rendering nothing — which is the
   * exact shape of live run 2's founder-held request (finding 16).
   */
  blocksWord: string;
  /** The assignments named by `blocks`, short-form, bounded. */
  blocks: readonly string[];
  /** Signed `created_at` of the request in ms, or null when it is not folded. */
  askedAtMs: number | null;
  /**
   * The class this ruling covers, from the answer's signed `condition`.
   *
   * Three values, and they are three different facts (I9, REVIEW-L7 F10):
   *
   * - a **string** — the ruling named this class;
   * - `null` — nothing named a class: the row is open, or the answer named
   *   none;
   * - `"unknown"` — the answer exists but its row fell outside
   *   {@link CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT}, so this surface
   *   has not read it. It is **not** "named no class", and a renderer must
   *   not print it as one.
   *
   * Live run 2, 11:33 (finding 21): the same question was asked twice because
   * the first answer had been given about one commit. A queue row that shows
   * the class is where the second asker finds out they need not ask.
   *
   * Quoted, never parsed: no state on this surface is derived from it.
   */
  condition: string | null | "unknown";
};

/**
 * The mission's waiting-on-a-person fact, in the rail's own words.
 *
 * The **fact** is the Rust fold's (`waitingOnDecision`); the **qualifier** is
 * this surface's, because only this surface knows whether the lead has an open
 * turn. With the lead working, a mission is waiting *and* running and the rail
 * says both; with no open lead turn, waiting is the whole of the state.
 */
export type CodingSessionMissionWaitingModel = {
  requestId: string;
  shortId: string;
  /** `Waiting on the founder` / `Waiting on {Who}` (§1g), without the stamp. */
  line: string;
  askedAtMs: number | null;
  /**
   * Where the waiting fact goes.
   *
   * `state-line` — a running mission with no open lead turn is *only* waiting.
   * `beside` — the lead is working, so the mission is waiting **and** running
   * and the rail says both.
   * `appended` — the mission has a terminal or conflicted state, which the
   * waiting fact may never erase (F4): `Mission completed · waiting on …`.
   */
  placement: "state-line" | "beside" | "appended";
};

/**
 * How long ago a request was asked, in one token, or null.
 *
 * Null under a minute and null for a stamp in the future: §1g forbids `0m`,
 * and a negative age is a clock disagreement, not a duration. `nowMs` is
 * supplied by the caller so the value is a render-time read of the clock the
 * surface already has — never a timer, never a subscription (I1).
 */
export function codingSessionMissionAskedRelative(
  askedAtMs: number | null,
  nowMs: number,
): string | null {
  if (askedAtMs === null || !Number.isFinite(askedAtMs)) return null;
  const elapsed = nowMs - askedAtMs;
  if (!Number.isFinite(elapsed) || elapsed < 60_000) return null;
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/**
 * `you`, `the founder`, a resolved name, or the canonical truncation.
 *
 * `you` comes first (F12): a ruling held on the person reading the rail is the
 * one row they can act on, and it used to read as eight characters of their own
 * key. The fallback is `truncatePubkey`, the repository's single display form —
 * a hand-rolled `slice(0, 8)` here was a `check-pubkey-truncation` failure the
 * lane's own gate list never ran (F1b/F11).
 */
function heldOnLabel(
  heldOn: string,
  input: CodingSessionMissionInspectorInput,
): string {
  const viewer = input.currentUserPubkey?.trim().toLowerCase() ?? null;
  const founder = input.founderPubkey?.trim().toLowerCase() ?? null;
  const party = heldOn === "founder" ? founder : heldOn.trim().toLowerCase();
  if (viewer !== null && party !== null && viewer === party) return "you";
  if (heldOn === "founder") return "the founder";
  if (founder !== null && founder === heldOn.trim().toLowerCase()) {
    return "the founder";
  }
  const resolved = input.resolveActorLabel?.(heldOn)?.trim();
  return resolved && resolved.length > 0 ? resolved : truncatePubkey(heldOn);
}

/**
 * How much of a signed question one queue row prints.
 *
 * The stream projection already bounds a summary at 280 characters, but the
 * queue must not depend on somebody else's bound: a rail row is scanned, and a
 * 400-character question turns the queue into a wall (F13).
 */
export const MAX_CODING_SESSION_DECISION_QUESTION_CHARS = 200;

function clampQuestion(value: string): string {
  const collapsed = value.trim().replace(/\s+/g, " ");
  return collapsed.length <= MAX_CODING_SESSION_DECISION_QUESTION_CHARS
    ? collapsed
    : `${collapsed.slice(0, MAX_CODING_SESSION_DECISION_QUESTION_CHARS - 1)}…`;
}

/**
 * How much of a signed condition one queue row prints.
 *
 * The same 200-character bound a question gets, and for the same reason: a
 * rail row is scanned. The whole text is one row away in the Mission stream,
 * which renders the signed record itself.
 */
function clampCondition(value: string | null | undefined): string | null {
  if (typeof value !== "string") return null;
  const collapsed = value.trim().replace(/\s+/g, " ");
  if (collapsed.length === 0) return null;
  return collapsed.length <= MAX_CODING_SESSION_DECISION_QUESTION_CHARS
    ? collapsed
    : `${collapsed.slice(0, MAX_CODING_SESSION_DECISION_QUESTION_CHARS - 1)}…`;
}

/**
 * The class an answered row's ruling covered, or why this surface cannot say.
 *
 * `"unknown"` exactly when the answer's own row is not in the bounded window
 * this model was given. Collapsing that to `null` would tell a reader the
 * founder named no class when the truth is that nobody here looked — the
 * unknown-≠-empty rule, in the one place this lane could break it.
 */
function answerCondition(
  answers: ReadonlyMap<string, string | null>,
  answerId: string | null,
): string | null | "unknown" {
  if (answerId === null) return "unknown";
  if (!answers.has(answerId)) return "unknown";
  return clampCondition(answers.get(answerId) ?? null);
}

/** Terminal and conflict states the waiting line may never overwrite (F4). */
function isSettledMissionState(kind: string): boolean {
  return kind === "completed" || kind === "blocked" || kind === "conflict";
}

/**
 * The decision queue and the waiting line, from the fold's own two fields.
 *
 * Nothing here re-decides anything: `decisions` and `waitingOnDecision` are
 * read verbatim, the question and its timestamp are joined from the fold's own
 * `decision.request` row, and the only local judgement is whether the lead has
 * an open turn — which is what turns a waiting fact into the state line.
 */
export function deriveDecisions(input: CodingSessionMissionInspectorInput): {
  decisions: CodingSessionMissionDecisionModel[];
  decisionsTruncated: number;
  decisionsTruncatedNotice: string | null;
  decisionsKnown: boolean;
  waiting: CodingSessionMissionWaitingModel | null;
} {
  const requests = new Map(
    (input.transactions ?? [])
      .filter((row) => row.type === "decision.request")
      .map((row) => [row.sourceEventId, row] as const),
  );
  // Only the answers this surface actually holds. `transactions` is bounded
  // at CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT with older rows collapsed,
  // so an id absent from this map means "not read", never "named no class".
  const answers = new Map(
    (input.transactions ?? [])
      .filter((row) => row.type === "decision.answer")
      .map((row) => [row.sourceEventId, row.condition ?? null] as const),
  );
  const source = input.decisions;
  const rows = (source ?? []).map((decision) => {
    const request = requests.get(decision.requestId) ?? null;
    const answered = decision.answeredBy !== null;
    const blocks = decision.blocks.map((eventId) => eventId.slice(0, 8));
    return {
      requestId: decision.requestId,
      shortId: decision.requestId.slice(0, 8),
      question:
        request && request.summary.trim().length > 0
          ? clampQuestion(request.summary)
          : null,
      state: (answered ? "answered" : "open") as "open" | "answered",
      stateWord: answered
        ? `Answered by ${heldOnLabel(
            decision.answeredBy === null ? "founder" : decision.answeredBy,
            input,
          )}`
        : `Open · held on ${heldOnLabel(decision.heldOn, input)}`,
      blocksWord:
        blocks.length === 0
          ? "holds up no assignment yet"
          : `holds up ${blocks.length} assignment${
              blocks.length === 1 ? "" : "s"
            }: ${blocks.join(", ")}`,
      blocks,
      askedAtMs: request ? request.createdAt * 1000 : null,
      // Read off the answer's own signed row, never off the request: a
      // condition is something the *ruling* said. Null while the row is open;
      // `unknown` when the answer's row aged out of the bounded window, which
      // is a different fact from "named no class" (REVIEW-L7 F10).
      condition: answered ? answerCondition(answers, decision.answerId) : null,
    };
  });
  rows.sort((left, right) => {
    if (left.state !== right.state) return left.state === "open" ? -1 : 1;
    return (right.askedAtMs ?? 0) - (left.askedAtMs ?? 0);
  });
  const shown = rows.slice(0, MAX_CODING_SESSION_MISSION_DECISION_ROWS);
  const dropped = rows.slice(MAX_CODING_SESSION_MISSION_DECISION_ROWS);
  // F13: the sort is open-first then newest-first, so what the bound drops is
  // the *oldest answered* rows — saying "earlier decisions" invited a reader to
  // think an open ruling might be hidden down there. When that is not true, the
  // notice says so; when it is, it claims nothing.
  const decisionsTruncatedNotice =
    dropped.length === 0
      ? null
      : dropped.every((row) => row.state === "answered")
        ? `${dropped.length} older answered decisions not shown`
        : `${dropped.length} decisions not shown`;
  const waitingInput = input.waitingOnDecision ?? null;
  const waiting: CodingSessionMissionWaitingModel | null =
    waitingInput === null
      ? null
      : {
          requestId: waitingInput.requestId,
          shortId: waitingInput.requestId.slice(0, 8),
          line: `Waiting on ${heldOnLabel(waitingInput.heldOn, input)}`,
          askedAtMs:
            (requests.get(waitingInput.requestId)?.createdAt ?? 0) * 1000 ||
            null,
          // F4: a mission that ended does not stop having ended because
          // somebody owes a ruling. `appended` puts the waiting fact beside the
          // terminal word; only a running mission with no open lead turn lets
          // waiting *be* the state.
          placement: isSettledMissionState(input.missionState.kind)
            ? ("appended" as const)
            : input.leadHasOpenTurn === true
              ? ("beside" as const)
              : ("state-line" as const),
        };
  return {
    decisions: shown,
    decisionsTruncated: dropped.length,
    decisionsTruncatedNotice,
    decisionsKnown: source !== undefined,
    waiting,
  };
}
