import { ChevronRight, CircleAlert, CircleStop, Copy, X } from "lucide-react";

import { ACTIVITY_ROW_LINE_CLASS } from "@/features/agents/ui/AgentSessionToolItem/ToolItemRowClasses";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  formatCodingSessionCompletionOutcome,
  formatCodingSessionCost,
  formatCodingSessionCostBasis,
  formatCodingSessionDuration,
  type CodingSessionTurnCompletion as CodingSessionTurnCompletionModel,
  type CodingSessionTurnFold,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { cn } from "@/shared/lib/cn";
import { useCodingSessionDisclosure } from "./CodingSessionTranscriptDisclosure";
import { CodingSessionDiagnosticRows } from "./CodingSessionTranscriptParts";
import { formatCodingSessionBlockTime } from "./CodingSessionTranscriptRhythm";

/**
 * The two rows a settled turn adds around its answer: the "Worked for …"
 * fold toggle and the quiet line under the answer. Split out of
 * `CodingSessionTranscript.tsx` for the 1000-line ceiling.
 */

/**
 * The row standing in for a settled turn's folded work.
 *
 * A button rather than a `<details>`: opening it does not nest the work
 * under it, it puts every hidden entry back in its own place in the turn.
 *
 * SV-07/SV-08: the whole row is the target and fills on hover, the time the
 * work began waits at its right for the pointer, and a hairline under it is
 * the turn's only rule. The model's summary — which may name a failed step —
 * is always on the row, muted; only the time waits for hover. A fold exists
 * only once the turn has settled, so nothing here shows while it is live.
 */
export function CodingSessionWorkedFold({
  fold,
  onToggle,
  open,
  startedAt,
}: {
  fold: CodingSessionTurnFold;
  onToggle: () => void;
  open: boolean;
  /** When the turn's work began; the hover time. `null` says no time. */
  startedAt: string | null;
}) {
  const label =
    fold.durationMs !== null
      ? `Worked for ${formatCodingSessionDuration(fold.durationMs)}`
      : "Worked";
  const time = formatCodingSessionBlockTime(startedAt);
  return (
    <div
      className="border-b border-border/60 pb-1.5"
      data-testid="coding-session-worked-fold-row"
    >
      <button
        aria-expanded={open}
        className={cn(
          "group/fold-row cursor-pointer text-muted-foreground hover:text-foreground",
          ACTIVITY_ROW_LINE_CLASS,
          // T3's fold button is `px-1 gap-1`: the label sits where the
          // prose does, not where an icon would.
          "gap-1 px-1",
        )}
        data-hidden-count={fold.hiddenIndexes.length}
        data-testid="coding-session-worked-fold"
        onClick={onToggle}
        type="button"
      >
        <span className="shrink-0">{label}</span>
        {/* The work summary may ellipsize; the failure clause never does —
            a folded failure must stay on screen at any width (D2). */}
        {fold.workSummary ? (
          <span
            className="min-w-0 truncate text-muted-foreground/70"
            data-testid="coding-session-worked-fold-summary"
          >
            · {fold.workSummary}
          </span>
        ) : null}
        {fold.failureSummary ? (
          <span
            className="flex shrink-0 items-center gap-1 whitespace-nowrap text-muted-foreground/70"
            data-testid="coding-session-worked-fold-failures"
          >
            ·
            <X aria-hidden className="size-3 text-destructive/60" />
            {fold.failureSummary}
          </span>
        ) : null}
        <ChevronRight
          aria-hidden
          className={cn(
            "size-3.5 shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
        {time ? (
          <time
            className="coding-session-row-time ms-auto shrink-0 ps-3 text-xs tabular-nums text-muted-foreground"
            data-testid="coding-session-worked-fold-time"
            dateTime={startedAt ?? undefined}
            title={time.title}
          >
            {time.label}
          </time>
        ) : null}
      </button>
    </div>
  );
}

/**
 * One quiet line under a settled turn's answer.
 *
 * A stop, a failure, or any outcome other than a normal end (`max tokens`,
 * `refusal`, …) is the turn's outcome, so it is always on screen in its own
 * colour. Everything else — copy, when it finished, how long it took (when
 * the fold row does not already say), the cost estimate, and the turn's
 * diagnostics — waits for the pointer over the answer block or keyboard
 * focus (SV-07). Those words stay in the document, so assistive technology
 * and find-in-page still reach them. The caller withholds the line while the
 * turn is live.
 */
export function CodingSessionTurnCompletion({
  answerText,
  completion,
  diagnostics,
  durationShownInWorkFold,
  turnId,
}: {
  answerText: string | null;
  completion: CodingSessionTurnCompletionModel | null;
  diagnostics: TranscriptItem[];
  durationShownInWorkFold: boolean;
  turnId: string;
}) {
  const [detailsOpen, setDetailsOpen] = useCodingSessionDisclosure(
    `diagnostics:${turnId}`,
  );
  if (!completion && diagnostics.length === 0) return null;

  const duration =
    completion?.durationMs !== null &&
    completion?.durationMs !== undefined &&
    !durationShownInWorkFold
      ? formatCodingSessionDuration(completion.durationMs)
      : null;
  // A non-ceremonial outcome (`max tokens`, `refusal`, …) says the turn did
  // not end the ordinary way; like a stop or a failure it is always shown.
  const outcome = completion
    ? formatCodingSessionCompletionOutcome(completion)
    : null;
  const time = completion
    ? formatCodingSessionBlockTime(completion.timestamp)
    : null;
  const meta = [
    completion?.state === "completed" && duration
      ? `Worked for ${duration}`
      : duration,
    completion?.costUsd !== null && completion?.costUsd !== undefined
      ? `${formatCodingSessionCost(completion.costUsd)} ${formatCodingSessionCostBasis(completion.costBasis)}`
      : null,
  ].filter((value): value is string => Boolean(value));

  return (
    <div data-testid="coding-session-turn-completion-block">
      <div
        className={cn(
          "flex min-h-6 flex-wrap items-center gap-x-1.5 px-1 text-xs text-muted-foreground",
          completion?.state === "interrupted" &&
            "text-amber-600 dark:text-amber-400",
          completion?.state === "failed" && "text-destructive",
        )}
        data-outcome={outcome ?? undefined}
        data-testid="coding-session-turn-completion"
        data-turn-state={completion?.state ?? "unknown"}
      >
        {completion?.state === "interrupted" ? (
          <span className="flex items-center gap-1 font-medium">
            <CircleStop className="size-3.5" />
            Stopped
          </span>
        ) : completion?.state === "failed" ? (
          <span className="flex items-center gap-1 font-medium">
            <X className="size-3.5" />
            Failed
          </span>
        ) : null}
        {outcome ? (
          <span
            className={cn(
              "flex items-center gap-1 font-medium",
              completion?.state === "completed" &&
                "text-amber-600 dark:text-amber-400",
            )}
            data-testid="coding-session-turn-outcome"
          >
            {completion?.state === "completed" ? (
              <CircleAlert className="size-3.5" />
            ) : null}
            {completion?.state === "completed" ? `Ended: ${outcome}` : outcome}
          </span>
        ) : null}
        <span
          className="coding-session-turn-meta flex flex-wrap items-center gap-x-2 text-muted-foreground tabular-nums"
          data-pinned={detailsOpen ? "true" : undefined}
          data-testid="coding-session-turn-meta"
        >
          {answerText ? (
            <button
              aria-label="Copy response"
              className="-ms-1 inline-flex size-6 items-center justify-center rounded-md transition-colors hover:bg-accent/30 hover:text-foreground"
              data-testid="coding-session-turn-copy"
              onClick={() => copyTextToClipboard(answerText, "Response copied")}
              title="Copy response"
              type="button"
            >
              <Copy aria-hidden className="size-3" />
            </button>
          ) : null}
          {time ? (
            <time
              data-testid="coding-session-turn-time"
              dateTime={completion?.timestamp}
              title={time.title}
            >
              {time.label}
            </time>
          ) : null}
          {meta.length > 0 ? <span>{meta.join(" · ")}</span> : null}
          {diagnostics.length > 0 ? (
            <button
              aria-expanded={detailsOpen}
              className="rounded-sm px-1 transition-colors hover:text-foreground"
              data-testid="coding-session-diagnostics"
              onClick={() => setDetailsOpen(!detailsOpen)}
              type="button"
            >
              Turn details · {diagnostics.length}{" "}
              {diagnostics.length === 1 ? "event" : "events"}
            </button>
          ) : null}
        </span>
      </div>
      {detailsOpen && diagnostics.length > 0 ? (
        <div className="text-xs text-muted-foreground">
          <CodingSessionDiagnosticRows diagnostics={diagnostics} />
        </div>
      ) : null}
    </div>
  );
}
