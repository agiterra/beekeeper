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

/**
 * The signed `decision.request` body behind one fold row.
 *
 * The queue reads the fold's `decisions[]` for *state*, and this for the
 * *question and its declared options*. The model never invents an option the
 * request did not declare: a request with `options: []` offers the free-text
 * field alone.
 */
export type CodingSessionMissionDecisionRequestInput = {
  requestId: string;
  question: string;
  options: readonly string[];
  recommendation: string | null;
  createdAt: number;
};

/**
 * The signed `decision.answer` body behind one answered fold row.
 *
 * `condition` is §1k's seventh key. It is `null` on every build whose
 * `buzz-core` predates lane L7 — absent from the wire is read as null here,
 * never as an empty string, because "no condition" and "a blank condition" are
 * different claims and only the first is real.
 */
export type CodingSessionMissionDecisionAnswerInput = {
  answerId: string;
  requestRef: string;
  choice: number | string;
  note: string | null;
  condition: string | null;
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
   * The class this ruling covers, from the answer's signed `condition`,
   * clamped for the row with what was clamped disclosed.
   *
   * Three values, and they are three different facts (I9 + I10, REVIEW-L7 F10):
   *
   * - an **object** — the ruling named this class; `truncated` is how many
   *   characters the row did not print, 0 when the whole text is shown;
   * - `null` — nothing named a class: the row is open, or the answer named
   *   none;
   * - `"unknown"` — the answer exists but neither its signed body nor its
   *   stream row is in the bounded window this model was given, so this
   *   surface has not read it. It is **not** "named no class", and a renderer
   *   must not print it as one.
   *
   * Live run 2, 11:33 (finding 21): the same question was asked twice because
   * the first answer had been given about one commit. A queue row that shows
   * the class is where the second asker finds out they need not ask.
   *
   * Quoted, never parsed: no state on this surface is derived from it.
   */
  answerCondition: { text: string; truncated: number } | null | "unknown";
  /**
   * The request's own declared options, in signed order.
   *
   * Empty is a real answer from the wire — a question with no options — and
   * the form then offers free text alone rather than inventing buttons.
   */
  options: readonly string[];
  /** The asker's recommendation, when the signed request carried one. */
  recommendation: string | null;
  /** Exactly `founder` or a 64-hex actor pubkey, verbatim from the fold. */
  heldOn: string;
  /**
   * Whether the viewer is the party this ruling is held on.
   *
   * `false` disables the control — §1l disables, never hides — and `null` is
   * the honest answer when this surface does not know the viewer's own key.
   */
  viewerIsHolder: boolean | null;
  /** §1l's sentence when the viewer may not answer, else null. */
  heldElsewhereSentence: string | null;
  /**
   * The chosen option's words, or the free text — from the signed answer.
   *
   * A numeric choice is resolved through the *request's* own options; when the
   * fold did not include the request, the row names the index rather than
   * inventing the words that went with it.
   */
  answerChoiceWord: string | null;
  /** The umbrella this row belongs to, so the form can publish an answer. */
  channelRef: string | null;
  sessionRef: string | null;
  genesisRef: string | null;
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
export function heldOnLabel(
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
 * How much of a signed condition one answered row prints.
 *
 * 200 characters, the same bound the question uses: a condition states the
 * *class* a ruling covers and can be a paragraph, and a rail row is scanned.
 * What is clamped is disclosed, never silently dropped (I10). The whole text
 * is one row away in the Mission stream, which renders the signed record.
 */
export const MAX_CODING_SESSION_DECISION_CONDITION_CHARS = 200;

function clampCondition(
  value: string,
): { text: string; truncated: number } | null {
  const collapsed = value.trim().replace(/\s+/g, " ");
  if (collapsed.length === 0) return null;
  if (collapsed.length <= MAX_CODING_SESSION_DECISION_CONDITION_CHARS) {
    return { text: collapsed, truncated: 0 };
  }
  return {
    text: `${collapsed.slice(0, MAX_CODING_SESSION_DECISION_CONDITION_CHARS - 1)}\u2026`,
    truncated:
      collapsed.length - (MAX_CODING_SESSION_DECISION_CONDITION_CHARS - 1),
  };
}

/**
 * The class an answered row's ruling covered, or why this surface cannot say.
 *
 * Two inputs carry the same signed fact and either will do: the answer's own
 * decoded body (`decisionAnswers`) and its bounded stream row
 * (`transactions`). `"unknown"` is returned exactly when *neither* holds the
 * answer id — the answer is on the wire and this surface has not read it.
 * Collapsing that to `null` would tell a reader the founder named no class
 * when the truth is that nobody here looked: the unknown-≠-empty rule (I9),
 * in the one place this model could break it.
 */
function answerConditionFor(
  bodies: ReadonlyMap<string, { condition: string | null }>,
  rows: ReadonlyMap<string, string | null>,
  answerId: string | null,
): { text: string; truncated: number } | null | "unknown" {
  if (answerId === null) return "unknown";
  const body = bodies.get(answerId);
  if (body !== undefined) {
    return body.condition === null ? null : clampCondition(body.condition);
  }
  if (!rows.has(answerId)) return "unknown";
  const row = rows.get(answerId) ?? null;
  return row === null ? null : clampCondition(row);
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
  // The signed bodies behind those rows. Separate from `transactions` because
  // the stream row carries a *summary*, and an Answer control needs the
  // request's own declared options, which a summary has never held.
  const requestBodies = new Map(
    (input.decisionRequests ?? []).map((row) => [row.requestId, row] as const),
  );
  const answerBodies = new Map(
    (input.decisionAnswers ?? []).map((row) => [row.answerId, row] as const),
  );
  const viewer = input.currentUserPubkey?.trim().toLowerCase() ?? null;
  const founder = input.founderPubkey?.trim().toLowerCase() ?? null;
  const source = input.decisions;
  const rows = (source ?? []).map((decision) => {
    const request = requests.get(decision.requestId) ?? null;
    const requestBody = requestBodies.get(decision.requestId) ?? null;
    const answered = decision.answeredBy !== null;
    const answerBody =
      decision.answerId === null
        ? null
        : (answerBodies.get(decision.answerId) ?? null);
    const blocks = decision.blocks.map((eventId) => eventId.slice(0, 8));
    // The party a ruling is held on, as a key: `founder` resolves through the
    // umbrella's own founder pubkey, which is the same substitution §1g's
    // label makes. Unknown stays `null` — a control disabled because we do not
    // know who is reading is a different fact from one held elsewhere.
    const holder =
      decision.heldOn === "founder"
        ? founder
        : decision.heldOn.trim().toLowerCase();
    const viewerIsHolder =
      viewer === null || holder === null ? null : viewer === holder;
    const answerChoiceWord =
      answerBody === null
        ? null
        : typeof answerBody.choice === "string"
          ? answerBody.choice
          : (requestBody?.options[answerBody.choice] ??
            `option ${answerBody.choice + 1}`);
    return {
      requestId: decision.requestId,
      shortId: decision.requestId.slice(0, 8),
      question:
        requestBody && requestBody.question.trim().length > 0
          ? clampQuestion(requestBody.question)
          : request && request.summary.trim().length > 0
            ? clampQuestion(request.summary)
            : null,
      options: requestBody ? [...requestBody.options] : [],
      recommendation: requestBody?.recommendation ?? null,
      heldOn: decision.heldOn,
      viewerIsHolder,
      // REVIEW-L8 F9: the sentence prints whenever this surface cannot show
      // the ruling is the viewer's — `false` **and** `null`. Unknown rendered
      // as permitted is a guess in the direction §8 I9 forbids, and §1l's
      // sentence is true either way: it names the party the wire says holds
      // it, and says only they can answer.
      heldElsewhereSentence:
        answered || viewerIsHolder === true
          ? null
          : `This ruling is held on ${heldOnLabel(decision.heldOn, input)}, so only they can answer it. You can read it here.`,
      answerChoiceWord,
      // Read off the answer's own signed record, never off the request: a
      // condition is something the *ruling* said. Null while the row is open.
      answerCondition: answered
        ? answerConditionFor(answerBodies, answers, decision.answerId)
        : null,
      channelRef: input.channelRef ?? null,
      sessionRef: input.sessionRef ?? null,
      genesisRef: input.genesisRef ?? null,
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
      askedAtMs: requestBody
        ? requestBody.createdAt * 1000
        : request
          ? request.createdAt * 1000
          : null,
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
