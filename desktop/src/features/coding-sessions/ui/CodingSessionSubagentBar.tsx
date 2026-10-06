import { ArrowLeft, Bot } from "lucide-react";

import {
  type CodingSessionSubagentBarModel,
  codingSessionSubagentBarElapsedMs,
} from "@/features/coding-sessions/lib/codingSessionSubagentPageModel";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModelFormat";
import { cn } from "@/shared/lib/cn";
import { useNow } from "@/shared/lib/useNow";
import { CodingSessionSubagentStatusIcon } from "./CodingSessionSubagentEntry";

/**
 * The subagent page's header (SV-79; T3 `ProviderSubagentBar`): what the
 * subagent is, its settled status with a live timer while it runs, what it
 * reported spending, that it runs on its own, and the way back to its parent.
 */
export function CodingSessionSubagentBar({
  bar,
  onOpenParent,
}: {
  /** `null` when the owning call is not in this view. */
  bar: CodingSessionSubagentBarModel | null;
  onOpenParent: () => void;
}) {
  return (
    <div
      className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-border/60 bg-background px-4 py-2 text-xs"
      data-status={bar?.status ?? "missing"}
      data-testid="coding-session-subagent-bar"
    >
      <button
        className="inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
        data-testid="coding-session-subagent-open-parent"
        onClick={onOpenParent}
        title="Back to the parent conversation (Esc)"
        type="button"
      >
        <ArrowLeft aria-hidden className="size-3.5" />
        Open parent
      </button>
      <span className="flex min-w-0 items-center gap-1.5">
        <Bot aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        <span
          className="min-w-0 truncate font-semibold"
          data-testid="coding-session-subagent-bar-title"
        >
          {bar?.title ?? "Subagent"}
        </span>
        {bar?.type ? (
          <span className="max-w-32 shrink-0 truncate rounded-sm border border-border/60 px-1 font-mono text-2xs text-muted-foreground">
            {bar.type}
          </span>
        ) : null}
      </span>
      {bar ? (
        <span className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
          <CodingSessionSubagentStatusIcon status={bar.status} />
          <span data-testid="coding-session-subagent-bar-status">
            {bar.statusLabel}
          </span>
          {bar.live ? (
            <LiveElapsed bar={bar} />
          ) : bar.durationMs !== null ? (
            <span
              className="font-mono text-2xs tabular-nums"
              data-testid="coding-session-subagent-bar-elapsed"
            >
              {formatCodingSessionDuration(bar.durationMs)}
            </span>
          ) : null}
        </span>
      ) : null}
      {bar ? (
        <span
          className="flex min-w-0 items-center gap-1.5 truncate font-mono text-2xs text-muted-foreground"
          data-testid="coding-session-subagent-bar-meta"
        >
          {[bar.model, bar.tokens, bar.tools].filter(Boolean).join(" · ")}
        </span>
      ) : null}
      <span
        className={cn(
          "ms-auto shrink-0 text-2xs text-muted-foreground/80",
          !bar && "hidden",
        )}
        title="A subagent works on its own task and reports back to the agent that started it; it takes no prompts from here."
      >
        Runs on its own
      </span>
    </div>
  );
}

/** Mounted only while running, so a settled bar never ticks. */
function LiveElapsed({ bar }: { bar: CodingSessionSubagentBarModel }) {
  const now = useNow(1_000);
  const elapsed = codingSessionSubagentBarElapsedMs(bar, now);
  if (elapsed === null) return null;
  return (
    <span
      className="font-mono text-2xs tabular-nums"
      data-live="true"
      data-testid="coding-session-subagent-bar-elapsed"
    >
      {formatCodingSessionDuration(Math.floor(elapsed / 1_000) * 1_000)}
    </span>
  );
}
