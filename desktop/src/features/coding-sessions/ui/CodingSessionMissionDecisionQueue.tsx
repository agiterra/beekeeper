import {
  codingSessionMissionAskedRelative,
  type CodingSessionMissionDecisionModel,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
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
 */
export function CodingSessionMissionDecisionQueue({
  decisions,
  decisionsKnown,
  decisionsTruncated,
  decisionsTruncatedNotice = null,
  nowMs = Date.now(),
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
}) {
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
                    {asked === null ? null : ` · asked ${asked}`}
                  </p>
                  <p className={cn(missionRowMetaClass(), "mt-0.5")}>
                    {decision.shortId} · {decision.blocksWord}
                  </p>
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
