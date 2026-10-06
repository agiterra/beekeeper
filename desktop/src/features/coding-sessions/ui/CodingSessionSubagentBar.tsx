import { ArrowUpLeft, Bot } from "lucide-react";

import {
  type CodingSessionSubagentBarModel,
  codingSessionSubagentBarElapsedMs,
} from "@/features/coding-sessions/lib/codingSessionSubagentPageModel";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModelFormat";
import { cn } from "@/shared/lib/cn";
import { useNow } from "@/shared/lib/useNow";
import { CodingSessionSubagentStatusIcon } from "./CodingSessionSubagentEntry";

/**
 * The subagent page's facts bar (SV-79, SV-98; T3 `ProviderSubagentBar`).
 *
 * It sits where the composer sits — docked at the bottom of the page, in the
 * composer's measure — because that is the place a person looks to talk to
 * the agent on screen, and a subagent takes no prompts: the bar says so
 * ("Runs on its own") in the spot the composer would have been. It carries
 * what the subagent is, its settled status with a live timer only while it
 * runs, what it reported spending, and the way back to its parent.
 */
export function CodingSessionSubagentBar({
  bar,
  onOpenParent,
  parentStatusLabel = null,
}: {
  /** `null` when the owning call is not in this view. */
  bar: CodingSessionSubagentBarModel | null;
  onOpenParent: () => void;
  /** The parent's status as the workspace shows it, for the button's hint. */
  parentStatusLabel?: string | null;
}) {
  const meta = bar
    ? [bar.modelName, bar.tokens, bar.tools].filter(Boolean).join(" · ")
    : "";
  return (
    <div
      className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 rounded-2xl border border-border/70 bg-background px-4 py-2.5 text-sm shadow-sm"
      data-status={bar?.status ?? "missing"}
      data-testid="coding-session-subagent-bar"
    >
      <span className="flex min-w-0 items-center gap-1.5">
        {bar ? (
          <CodingSessionSubagentStatusIcon status={bar.status} />
        ) : (
          <Bot
            aria-hidden
            className="size-3.5 shrink-0 text-muted-foreground"
          />
        )}
        <span
          className="min-w-0 truncate font-medium"
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
        <span className="flex shrink-0 items-center gap-1 text-muted-foreground">
          <span data-testid="coding-session-subagent-bar-status">
            {bar.statusLabel}
          </span>
          {bar.live ? (
            <LiveElapsed bar={bar} />
          ) : bar.durationMs !== null ? (
            <>
              {/* Spaced text, so it reads "Completed in 9.0s" as one phrase. */}
              {" in "}
              <span
                className="tabular-nums"
                data-testid="coding-session-subagent-bar-elapsed"
              >
                {formatCodingSessionDuration(bar.durationMs)}
              </span>
            </>
          ) : null}
        </span>
      ) : null}
      {meta ? (
        <span
          className="min-w-0 truncate text-xs text-muted-foreground"
          data-testid="coding-session-subagent-bar-meta"
          title={bar?.model ?? undefined}
        >
          {meta}
        </span>
      ) : null}
      <span className="ms-auto flex shrink-0 items-center gap-3">
        <span
          className={cn("text-xs text-muted-foreground", !bar && "hidden")}
          title="A subagent works on its own task and reports back to the agent that started it; it takes no prompts from here."
        >
          Runs on its own
        </span>
        <button
          className="inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 font-medium text-foreground transition-colors hover:bg-accent/60"
          data-testid="coding-session-subagent-open-parent"
          onClick={onOpenParent}
          title={
            parentStatusLabel
              ? `Back to the parent conversation — ${parentStatusLabel} (Esc)`
              : "Back to the parent conversation (Esc)"
          }
          type="button"
        >
          <ArrowUpLeft aria-hidden className="size-3.5" />
          Open parent
        </button>
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
      className="tabular-nums"
      data-live="true"
      data-testid="coding-session-subagent-bar-elapsed"
    >
      {formatCodingSessionDuration(Math.floor(elapsed / 1_000) * 1_000)}
    </span>
  );
}
