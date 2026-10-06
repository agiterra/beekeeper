import { Bot } from "lucide-react";

import type { CodingSessionSubagentLineage } from "@/features/coding-sessions/lib/codingSessionSubagentPageModel";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { cn } from "@/shared/lib/cn";
import { codingSessionHeaderStatusDotClass } from "./CodingSessionHeaderParts";

/**
 * The divider that heads a subagent's transcript (SV-98; T3's "Subagent of ·
 * <parent>" rule): whose subagent this is, and — as T3's Lineage row does —
 * the parent's live status, drawn with the header's own dot and word so the
 * two never disagree. The pill is the way back to the parent too.
 *
 * The status is the workspace's (signed status corrected by reachability); a
 * parent no execution in view can name shows no status rather than a guess.
 */
export function CodingSessionSubagentPageLineage({
  lineage,
  onOpenParent,
  sessionClosed,
}: {
  lineage: CodingSessionSubagentLineage;
  onOpenParent: () => void;
  sessionClosed: boolean;
}) {
  const { status } = lineage;
  const detail = status ? codingSessionWorkspaceStatusDetail(status) : null;
  const statusWord = status
    ? sessionClosed
      ? "Closed"
      : detail
        ? `${status.label} (${detail})`
        : status.label
    : null;
  return (
    <div
      className="flex min-w-0 items-center gap-3"
      data-testid="coding-session-subagent-page-lineage"
    >
      <span aria-hidden className="h-px min-w-6 flex-1 bg-border/70" />
      <button
        aria-label={`Subagent of ${lineage.title}${statusWord ? ` — ${statusWord}` : ""}. Open parent`}
        className="flex min-w-0 max-w-[80%] items-center gap-1.5 rounded-full border border-border/70 px-3 py-1 text-xs transition-colors hover:bg-accent/40"
        onClick={onOpenParent}
        title="Open the parent conversation"
        type="button"
      >
        <Bot aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="shrink-0 font-medium text-foreground/85">
          Subagent of
        </span>
        <span className="shrink-0 text-muted-foreground">·</span>
        <span
          className="min-w-0 truncate text-muted-foreground"
          data-testid="coding-session-subagent-page-parent-title"
        >
          {lineage.title}
        </span>
        {status && statusWord ? (
          <span
            className="flex shrink-0 items-center gap-1 text-muted-foreground"
            data-parent-status={status.kind}
            data-testid="coding-session-subagent-page-parent-status"
            title={detail ?? undefined}
          >
            <span
              aria-hidden
              className={cn(
                "size-1.5 rounded-full",
                codingSessionHeaderStatusDotClass(status, sessionClosed),
              )}
            />
            {sessionClosed ? "Closed" : status.label}
          </span>
        ) : null}
      </button>
      <span aria-hidden className="h-px min-w-6 flex-1 bg-border/70" />
    </div>
  );
}
