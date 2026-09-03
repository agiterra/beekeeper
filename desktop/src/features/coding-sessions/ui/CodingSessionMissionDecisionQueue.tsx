import {
  codingSessionMissionAskedRelative,
  type CodingSessionMissionDecisionModel,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { useCodingSessionDecisionAnswer } from "@/features/coding-sessions/hooks/useCodingSessionDecisionAnswer";
import type { CodingSessionDecisionAnswerDeps } from "@/features/coding-sessions/lib/codingSessionTeamTransactionPublish";
import { CodingSessionDecisionAnswerForm } from "./CodingSessionDecisionAnswerForm";
import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
  missionRowTitleClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";

/**
 * The rulings this mission is holding, and the ones it has had.
 *
 * Item 105 put `decisions` and `waitingOnDecision` on the wire and nothing
 * rendered either for a whole batch: live run 2 had two requests, two answers,
 * one of them held on the founder — and the only place a person could see any
 * of it was `bee sessions operation list`. This is that queue.
 *
 * Three rules it keeps:
 *
 * - **Every state is a word.** `Open · held on the founder` and `Answered by
 *   {Who}` carry the state; the amber card is a second carrier, never the only
 *   one (I9).
 * - **`blocks: []` is an answer, not a gap.** The fold saying a request holds
 *   up no assignment is exactly the shape live run 2's founder-held request
 *   had, and a row that rendered nothing there would read as "we don't know".
 * - **Unknown is not empty.** A surface with no fold says so, rather than
 *   showing an empty list that reads as "nobody asked anything".
 *
 * L8 adds the fourth: **the screen that shows a ruling can take it.** Live run
 * 2 had the founder answering three requests from a terminal while this queue
 * displayed every one of them and could do nothing about it. Each open row now
 * carries {@link CodingSessionDecisionAnswerForm}, which publishes a real
 * `decision.answer` down the same build → sign → publish path the launch uses
 * for a 44245 — and the row still only reads `Answered` when a fold carrying
 * that answer comes back.
 */
export function CodingSessionMissionDecisionQueue({
  decisions,
  decisionsKnown,
  decisionsTruncated,
  decisionsTruncatedNotice = null,
  nowMs = Date.now(),
  supportsCondition,
  answerDeps,
  publishedForTest,
}: {
  decisions: readonly CodingSessionMissionDecisionModel[];
  /** False when no fold supplied `decisions`. Unknown, not empty. */
  decisionsKnown: boolean;
  /** Rows the 50-row bound dropped, disclosed rather than hidden (I10). */
  decisionsTruncated: number;
  /**
   * The sentence for those rows.
   *
   * F13: the sort is open-first then newest-first, so what the bound drops is
   * normally the oldest *answered* rows — `N earlier decisions not shown`
   * invited a reader to wonder whether an open ruling was hidden below. The
   * model says which it is.
   */
  decisionsTruncatedNotice?: string | null;
  /**
   * The clock, read once by the caller at render. A prop rather than a call
   * inside so a test pins it and so nothing here ever wants a timer (I1).
   */
  nowMs?: number;
  /**
   * Whether this build's core accepts §1k's `condition`.
   *
   * Normally measured by the hook's own probe; a caller may pin it (a test, or
   * a surface that already asked). `null` offers no condition field at all.
   */
  supportsCondition?: boolean | null;
  /** Injected publish path. The real Tauri/keyring/relay wiring by default. */
  answerDeps?: Partial<CodingSessionDecisionAnswerDeps>;
  /**
   * Publish receipts pinned for a server-rendered test.
   *
   * The hook's own receipts are the live source; this exists because
   * `renderToStaticMarkup` runs no effects and no click handlers, so the
   * post-publish state has no other way to be rendered under test.
   */
  publishedForTest?: Readonly<Record<string, string>>;
}) {
  const answering = useCodingSessionDecisionAnswer(answerDeps);
  const conditionSupported =
    supportsCondition ??
    answering.capabilities?.supportsDecisionAnswerCondition ??
    null;
  return (
    <div data-testid="mission-decision-queue">
      {!decisionsKnown ? (
        <p
          className={missionRowBodyClass()}
          data-testid="mission-decisions-unknown"
        >
          Decisions unknown — no fold has run for this session yet.
        </p>
      ) : decisions.length === 0 ? (
        <p
          className={missionRowBodyClass()}
          data-testid="mission-decisions-empty"
        >
          No decisions on the wire
        </p>
      ) : (
        <ul aria-label="Decisions" className="space-y-1.5">
          {decisions.map((decision) => {
            const asked = codingSessionMissionAskedRelative(
              decision.askedAtMs,
              nowMs,
            );
            return (
              <li key={decision.requestId}>
                <div
                  className={
                    decision.state === "open"
                      ? missionRowClass("attention", { tone: "caution" })
                      : missionRowClass("standard")
                  }
                  data-decision-state={decision.state}
                  data-testid="mission-decision-row"
                >
                  <p className={missionRowTitleClass()}>
                    {decision.question ??
                      "Question not in this session's records"}
                  </p>
                  <p className={cn(missionRowBodyClass(), "mt-1 font-medium")}>
                    {decision.stateWord}
                    {decision.state === "answered" && decision.answerChoiceWord
                      ? ` · ${decision.answerChoiceWord}`
                      : null}
                    {decision.state === "answered" &&
                    decision.answerCondition !== null
                      ? decision.answerCondition === "unknown"
                        ? // Not "named no class": this surface has not read the
                          // answer, and saying nothing here would read as the
                          // founder having named none (I9).
                          " · condition not read"
                        : ` · condition: ${decision.answerCondition.text}`
                      : null}
                    {asked === null ? null : ` · asked ${asked}`}
                  </p>
                  {decision.answerCondition !== null &&
                  decision.answerCondition !== "unknown" &&
                  decision.answerCondition.truncated > 0 ? (
                    <p
                      className={cn(missionRowMetaClass(), "mt-0.5")}
                      data-testid="mission-decision-condition-truncation"
                    >
                      {decision.answerCondition.truncated} more characters of
                      this condition are not shown
                    </p>
                  ) : null}
                  <p className={cn(missionRowMetaClass(), "mt-0.5")}>
                    {decision.shortId} · {decision.blocksWord}
                  </p>
                  {decision.state === "open" &&
                  (decision.channelRef === null ||
                    decision.sessionRef === null ||
                    decision.genesisRef === null) ? (
                    // REVIEW-L8 F10: hiding the control here reads as "this
                    // cannot be answered from here", which is the exact
                    // reading the disabled-never-hidden rule exists to
                    // prevent. Unreachable from the native projection, which
                    // always sets all three — a latent hole, said out loud.
                    <p
                      className={cn(missionRowBodyClass(), "mt-2")}
                      data-testid="decision-answer-unanchored"
                    >
                      This view does not know which session this ruling belongs
                      to, so it cannot be answered from here.
                    </p>
                  ) : null}
                  {decision.state === "open" &&
                  decision.channelRef !== null &&
                  decision.sessionRef !== null &&
                  decision.genesisRef !== null ? (
                    <CodingSessionDecisionAnswerForm
                      conditionMaxBytes={
                        answering.capabilities?.conditionMaxBytes ?? 512
                      }
                      decision={decision}
                      errorMessage={
                        answering.errors[decision.requestId] ?? null
                      }
                      onAnswer={(draft) => {
                        void answering.answer({
                          declaredOptions: decision.options.length,
                          channelRef: decision.channelRef as string,
                          sessionRef: decision.sessionRef as string,
                          genesisRef: decision.genesisRef as string,
                          draft,
                        });
                      }}
                      pending={
                        answering.pendingRequestId === decision.requestId
                      }
                      publishedEventId={
                        publishedForTest?.[decision.requestId] ??
                        answering.published[decision.requestId] ??
                        null
                      }
                      supportsCondition={conditionSupported}
                    />
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {decisionsTruncated > 0 ? (
        <p
          className={cn(missionRowMetaClass(), "mt-1.5")}
          data-testid="mission-decisions-truncation"
          role="status"
        >
          {decisionsTruncatedNotice ??
            `${decisionsTruncated} decisions not shown`}
        </p>
      ) : null}
    </div>
  );
}
