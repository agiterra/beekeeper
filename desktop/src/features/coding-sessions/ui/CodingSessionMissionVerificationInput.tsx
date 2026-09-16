import { CheckCircle2, CircleDashed, OctagonAlert } from "lucide-react";

import {
  codingSessionAssignmentInputCopy,
  type CodingSessionAssignmentInputState,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInputCopy";
import {
  missionRowBodyClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";

const BADGE_CLASS =
  "inline-flex max-w-full shrink-0 items-center gap-1 rounded-md border px-1.5 py-0.5 text-2xs";

/**
 * What this computer did about the revision a verifier or runner was asked to
 * start from — on the assignment row that asked for it.
 *
 * The sentence is read, not hovered. A seat that is on the trunk instead of on
 * the commit under test produces green tests about the wrong code, and a
 * `title` attribute is invisible to the person scanning the mission for why a
 * verdict disagrees with the evidence.
 *
 * The established sentence always carries the ordering disclosure: the move is
 * not sequenced against the lead's wake, so a turn may already have run.
 */
export function CodingSessionMissionVerificationInput({
  onRetry,
  state,
}: {
  /** A person's own click. Omitted where there is nobody to click. */
  onRetry?: () => void;
  state: CodingSessionAssignmentInputState;
}) {
  const copy = codingSessionAssignmentInputCopy(state);
  // A record is read for the tone it records: a recorded establishment is as
  // quiet as a live one, a recorded refusal as loud.
  const reading = state.kind === "recorded" ? state.inner.kind : state.kind;
  // An assignment that named no commit is a fact about the signed body, not a
  // failure of this computer, so it is quiet — as is a move still in flight.
  // Only a refusal, an unanswerable build, or a commit with no attempt behind
  // it wears the amber.
  const quiet = reading === "pending" || reading === "unnamed";
  const settled = reading === "established";
  const Icon = settled ? CheckCircle2 : quiet ? CircleDashed : OctagonAlert;
  const tone =
    settled || quiet
      ? "border-border/60 text-muted-foreground"
      : "border-amber-500/45 text-amber-700 dark:text-amber-300";
  return (
    <div
      className="mt-1.5 min-w-0"
      data-kind={state.kind}
      data-testid="coding-session-verification-input"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
        <span
          className={cn(BADGE_CLASS, tone)}
          data-testid="coding-session-verification-input-badge"
        >
          <Icon aria-hidden className="size-3 shrink-0" />
          <span className="truncate">{copy.badge}</span>
        </span>
        {copy.retryable && onRetry ? (
          <button
            className={cn(
              missionRowMetaClass(),
              "rounded-sm underline underline-offset-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            )}
            data-testid="coding-session-verification-input-retry"
            onClick={onRetry}
            type="button"
          >
            Try again
          </button>
        ) : null}
      </div>
      <p
        className={cn(
          missionRowBodyClass(),
          "mt-1 wrap-break-word",
          settled || quiet ? null : "text-amber-700 dark:text-amber-300",
        )}
        data-testid="coding-session-verification-input-sentence"
      >
        {copy.sentence}
      </p>
      {copy.detail ? (
        <details className={cn(missionRowMetaClass(), "mt-1")}>
          <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            What this computer reported
          </summary>
          <code className="mt-1 block break-all">{copy.detail}</code>
        </details>
      ) : null}
    </div>
  );
}
