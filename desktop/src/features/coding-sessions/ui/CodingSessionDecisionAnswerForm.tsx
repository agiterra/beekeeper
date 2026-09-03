import * as React from "react";

import type { CodingSessionMissionDecisionModel } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { missionRowBodyClass } from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import type { CodingSessionDecisionAnswerDraft } from "@/features/coding-sessions/lib/codingSessionTeamTransactionPublish";
import { cn } from "@/shared/lib/cn";

/** §1l's hint under the condition field, frozen. */
export const CODING_SESSION_CONDITION_HINT =
  "Name the class this ruling covers, so it does not have to be asked again for the next commit.";

/**
 * The sentence a founder sees when this build's wire cannot carry a condition.
 *
 * Disclosed rather than hidden: a field that silently is not there reads as a
 * feature nobody built, and the honest fact is that the key exists in the
 * design and not yet in the bytes this build signs.
 *
 * The second clause was wrong in fix round 1 — it said the ruling "covers the
 * commit it names", and a `decision.answer` names no commit at all: it names a
 * `requestRef`. §1k's motivating example is a commit; the mechanism is a
 * question (REVIEW-L8 §8, unfrozen sentence 2).
 */
export const CODING_SESSION_CONDITION_UNSUPPORTED =
  "This build's wire has no condition on an answer yet, so this ruling covers " +
  "only the question it answers, and the same question will have to be asked again.";

const inputClass =
  "w-full rounded-md border border-border/60 bg-background px-2 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60";

/**
 * The founder's answer to one open ruling, on the screen that showed it.
 *
 * Three rules it keeps:
 *
 * - **The options are the request's own.** Buttons come from the signed
 *   `options[]`, index for index; a request that declared none offers free
 *   text alone. Nothing here invents an option the asker did not write.
 * - **Disabled, never hidden.** A ruling held on somebody else keeps its
 *   control and says whose it is (§1l) — a missing button would read as "this
 *   cannot be answered from here" rather than "not by you".
 * - **A publish is not an answer.** Submitting hands the draft up; the row
 *   only reads `Answered` when a fold carrying the answer arrives. A queue
 *   that flipped on a resolved promise would be the same lie as a badge with
 *   no event behind it.
 */
export function CodingSessionDecisionAnswerForm({
  decision,
  errorMessage = null,
  onAnswer,
  pending = false,
  publishedEventId = null,
  supportsCondition = null,
  conditionMaxBytes = 512,
}: {
  decision: CodingSessionMissionDecisionModel;
  /** The relay's own words after a rejected publish, or null. */
  errorMessage?: string | null;
  onAnswer?: (draft: CodingSessionDecisionAnswerDraft) => void;
  pending?: boolean;
  /**
   * The event id a successful publish left on the wire, or null.
   *
   * Renders the weaker, true claim — an answer went out — and disables the
   * form until a fold carries it. Never `Answered`: that word belongs to the
   * fold (REVIEW-L8 F8).
   */
  publishedEventId?: string | null;
  /**
   * Whether this build's `buzz-core` accepts §1k's `condition`.
   *
   * `null` is "not measured yet" and offers nothing — an input whose value the
   * relay would refuse is worse than no input, because the ruling then does
   * not reach the wire at all.
   */
  supportsCondition?: boolean | null;
  /** The bound the native adapter reported, for the counter. */
  conditionMaxBytes?: number;
}) {
  const [choiceText, setChoiceText] = React.useState("");
  const [note, setNote] = React.useState("");
  const [condition, setCondition] = React.useState("");
  // REVIEW-L8 F9: unknown is not permitted. A control this surface cannot
  // show belongs to the viewer is disabled and says whose it is — the relay
  // would refuse the publish anyway, and learning that from an error is the
  // guess §8 I9 forbids. Only a *known* holder gets a live control.
  // F8: and a published answer disables it until a fold arrives, so the same
  // ruling cannot be signed twice while the wire catches up.
  const disabled =
    decision.viewerIsHolder !== true || publishedEventId !== null;
  const conditionBytes = new TextEncoder().encode(condition).length;
  const overCondition = conditionBytes > conditionMaxBytes;

  const submit = (choice: number | string) => {
    if (disabled || pending || overCondition) return;
    onAnswer?.({
      requestRef: decision.requestId,
      choice,
      note: note.trim().length === 0 ? null : note,
      condition:
        supportsCondition !== true || condition.trim().length === 0
          ? null
          : condition,
    });
  };

  return (
    <div className="mt-2" data-testid="decision-answer-form">
      {decision.heldElsewhereSentence ? (
        <p
          className={cn(missionRowBodyClass(), "mb-1.5")}
          data-testid="decision-answer-held-elsewhere"
        >
          {decision.heldElsewhereSentence}
        </p>
      ) : null}
      {decision.recommendation ? (
        <p
          className={cn(missionRowBodyClass(), "mb-1.5")}
          data-testid="decision-answer-recommendation"
        >
          Asker recommends: {decision.recommendation}
        </p>
      ) : null}
      {decision.options.length > 0 ? (
        <div className="flex flex-wrap gap-1.5">
          {decision.options.map((option, index) => (
            <button
              className="rounded-md border border-border/60 bg-muted/30 px-2 py-1 text-xs font-medium text-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
              data-decision-option-index={index}
              data-testid="decision-answer-option"
              disabled={disabled || pending}
              // The index IS the identity: `choiceIndex` is what the answer
              // signs, the order is the signed request's own and never
              // reorders, and the option text alone collides if a producer
              // bypasses core's `validate_unique("options")` (REVIEW-L8 F11).
              // biome-ignore lint/suspicious/noArrayIndexKey: the signed option index is this row's identity
              key={`${decision.requestId}-${index}-${option}`}
              onClick={() => submit(index)}
              type="button"
            >
              {option}
            </button>
          ))}
        </div>
      ) : (
        <p
          className={cn(missionRowBodyClass(), "mb-1")}
          data-testid="decision-answer-no-options"
        >
          This request declared no options, so answer it in your own words.
        </p>
      )}

      <label className="mt-1.5 block">
        <span className="sr-only">Answer in your own words</span>
        <input
          className={inputClass}
          data-testid="decision-answer-choice"
          disabled={disabled || pending}
          onChange={(event) => setChoiceText(event.target.value)}
          placeholder="Answer in your own words"
          type="text"
          value={choiceText}
        />
      </label>

      <label className="mt-1.5 block">
        <span className="sr-only">Note (optional)</span>
        <input
          className={inputClass}
          data-testid="decision-answer-note"
          disabled={disabled || pending}
          onChange={(event) => setNote(event.target.value)}
          placeholder="Note (optional)"
          type="text"
          value={note}
        />
      </label>

      {supportsCondition === true ? (
        <div className="mt-1.5">
          <label className="block">
            <span className="text-2xs font-medium text-muted-foreground">
              Condition (optional)
            </span>
            <textarea
              className={cn(inputClass, "mt-0.5")}
              data-testid="decision-answer-condition"
              disabled={disabled || pending}
              onChange={(event) => setCondition(event.target.value)}
              rows={2}
              value={condition}
            />
          </label>
          <p
            className={cn(
              "mt-0.5 text-2xs",
              overCondition ? "text-destructive" : "text-muted-foreground",
            )}
            data-testid="decision-answer-condition-counter"
          >
            {conditionBytes}/{conditionMaxBytes} bytes
          </p>
          <p
            className="mt-0.5 text-2xs text-muted-foreground"
            data-testid="decision-answer-condition-hint"
          >
            {CODING_SESSION_CONDITION_HINT}
          </p>
        </div>
      ) : supportsCondition === false ? (
        <p
          className="mt-1.5 text-2xs text-muted-foreground"
          data-testid="decision-answer-condition-unsupported"
        >
          {CODING_SESSION_CONDITION_UNSUPPORTED}
        </p>
      ) : null}

      <button
        className="mt-2 rounded-md border border-border/60 bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary hover:bg-primary/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
        data-testid="decision-answer-submit"
        disabled={
          disabled || pending || choiceText.trim().length === 0 || overCondition
        }
        onClick={() => submit(choiceText)}
        type="button"
      >
        {pending ? "Answering…" : "Answer"}
      </button>

      {publishedEventId ? (
        <p
          className="mt-1.5 text-2xs text-muted-foreground"
          data-testid="decision-answer-published"
          role="status"
        >
          Answer published · {publishedEventId.slice(0, 8)} · the fold will show
          it
        </p>
      ) : null}
      {errorMessage ? (
        <p
          className="mt-1.5 text-2xs text-destructive"
          data-testid="decision-answer-error"
          role="alert"
        >
          {errorMessage}
        </p>
      ) : null}
    </div>
  );
}
