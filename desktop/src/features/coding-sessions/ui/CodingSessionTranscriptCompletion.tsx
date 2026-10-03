import { ChevronRight, CircleAlert, CircleStop, Copy, X } from "lucide-react";

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

/**
 * The two rows a settled turn adds around its answer: the "Worked for …"
 * fold toggle and the quiet completion line. Split out of
 * `CodingSessionTranscript.tsx` for the 1000-line ceiling.
 */

/**
 * The row standing in for a settled turn's folded work.
 *
 * A button rather than a `<details>`: opening it does not nest the work
 * under it, it puts every hidden entry back in its own place in the turn.
 */
export function CodingSessionWorkedFold({
  fold,
  onToggle,
  open,
}: {
  fold: CodingSessionTurnFold;
  onToggle: () => void;
  open: boolean;
}) {
  const label =
    fold.durationMs !== null
      ? `Worked for ${formatCodingSessionDuration(fold.durationMs)}`
      : "Worked";
  return (
    <button
      aria-expanded={open}
      className="flex min-h-6 w-fit max-w-full items-center gap-1.5 rounded-md px-0.5 text-left text-sm text-muted-foreground transition-colors hover:text-foreground"
      data-hidden-count={fold.hiddenIndexes.length}
      data-testid="coding-session-worked-fold"
      onClick={onToggle}
      type="button"
    >
      <ChevronRight
        className={cn(
          "size-3.5 shrink-0 transition-transform",
          open && "rotate-90",
        )}
      />
      <span className="shrink-0 font-medium">{label}</span>
      {fold.summary ? (
        <span className="min-w-0 truncate text-muted-foreground/70">
          · {fold.summary}
        </span>
      ) : null}
    </button>
  );
}

/**
 * One quiet line closing a settled turn.
 *
 * A stop, a failure, or any outcome other than a normal end (`max tokens`,
 * `refusal`, …) is the turn's outcome, so it is always on screen in its own
 * colour. Everything else — when it finished, how long it took (when the
 * fold row does not already say), the cost estimate, copy,
 * and the turn's diagnostics — waits for the pointer or keyboard focus. Those
 * words stay in the document, so assistive technology and find-in-page still
 * reach them. The caller withholds the line while the turn is live.
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
  const meta = [
    completion?.state === "completed" && duration
      ? `Worked for ${duration}`
      : duration,
    completion?.costUsd !== null && completion?.costUsd !== undefined
      ? `${formatCodingSessionCost(completion.costUsd)} ${formatCodingSessionCostBasis(completion.costBasis)}`
      : null,
    completion ? formatCompletionTime(completion.timestamp) : null,
  ].filter((value): value is string => Boolean(value));

  return (
    <div data-testid="coding-session-turn-completion-block">
      <div
        className={cn(
          "flex min-h-6 flex-wrap items-center gap-x-1.5 px-0.5 text-xs text-muted-foreground",
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
          className="coding-session-turn-meta flex flex-wrap items-center gap-x-1.5 text-muted-foreground"
          data-pinned={detailsOpen ? "true" : undefined}
          data-testid="coding-session-turn-meta"
        >
          {meta.length > 0 ? <span>{meta.join(" · ")}</span> : null}
          {answerText ? (
            <button
              aria-label="Copy response"
              className="inline-flex items-center gap-1 rounded-sm px-1 transition-colors hover:text-foreground"
              data-testid="coding-session-turn-copy"
              onClick={() => copyTextToClipboard(answerText, "Response copied")}
              type="button"
            >
              <Copy className="size-3" />
              Copy
            </button>
          ) : null}
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

function formatCompletionTime(timestamp: string): string | null {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return null;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
