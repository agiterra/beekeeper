import type { ReactNode } from "react";

import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import { cn } from "@/shared/lib/cn";

/** Singularity's always-readable roster: identity first, provider details second. */
export function CodingSessionParticipantBar({
  focusedExecutionKey,
  items,
  leading,
  onFocus,
}: {
  focusedExecutionKey: string | null;
  items: readonly CodingSessionParticipantPresence[];
  /** Workflow controls that belong before the roster in the same strip. */
  leading?: ReactNode;
  onFocus: (executionKey: string | null) => void;
}) {
  if (items.length === 0) return null;
  return (
    <nav
      aria-label="Session participants"
      className="flex min-h-14 shrink-0 items-center gap-2 overflow-x-auto border-b border-border/60 bg-background/80 px-4 py-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      data-testid="coding-session-participant-bar"
    >
      {leading ? (
        <div className="mr-1 flex shrink-0 items-center border-r border-border/60 pr-3">
          {leading}
        </div>
      ) : null}
      {items.map((item) => {
        const selected = item.executionKey === focusedExecutionKey;
        const accent = codingSessionParticipantAccent(item.executionKey);
        const live = item.status.kind === "working";
        const attention =
          item.status.kind === "unknown" && item.status.attention !== undefined;
        const noProviderAnswering =
          item.status.kind === "unknown" &&
          item.status.label === "No provider answering";
        return (
          <button
            aria-label={`${selected ? "Show all participants" : `Focus ${item.label}`} — ${item.disposition}`}
            aria-pressed={selected}
            className={cn(
              "group relative flex min-w-44 shrink-0 items-center gap-2.5 rounded-xl border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              selected
                ? cn(accent.border, accent.soft)
                : "border-border/60 bg-muted/15 hover:bg-muted/35",
              live && "coding-session-agent-breathe",
              noProviderAnswering && "border-destructive/45",
              attention && !noProviderAnswering && "border-amber-500/45",
            )}
            data-state={item.status.kind}
            data-testid="coding-session-participant-chip"
            key={item.executionKey}
            onClick={() => onFocus(selected ? null : item.executionKey)}
            title={
              item.secondaryLabel
                ? `${item.secondaryLabel} · ${item.lastTurnLabel}`
                : item.lastTurnLabel
            }
            type="button"
          >
            <span
              aria-hidden
              className={cn(
                "size-2.5 shrink-0 rounded-full",
                participantDot(item),
              )}
            />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-xs font-semibold text-foreground">
                {item.label}
              </span>
              <span
                className={cn(
                  "mt-0.5 block truncate text-2xs",
                  noProviderAnswering
                    ? "text-destructive"
                    : attention
                      ? "text-amber-700 dark:text-amber-300"
                      : "text-muted-foreground",
                )}
              >
                {item.disposition}
              </span>
              {live && item.activity ? (
                <span className="mt-0.5 block max-w-64 truncate text-2xs text-muted-foreground/75">
                  {item.activity}
                </span>
              ) : null}
            </span>
          </button>
        );
      })}
    </nav>
  );
}

function participantDot(item: CodingSessionParticipantPresence): string {
  if (item.status.kind === "working") return "bg-emerald-500";
  if (item.status.kind === "waiting") return "bg-amber-500";
  if (
    item.status.kind === "unknown" &&
    item.status.label === "No provider answering"
  ) {
    return "bg-destructive";
  }
  if (item.status.kind === "unknown" && item.status.attention) {
    return "bg-amber-500";
  }
  if (item.status.kind === "ended") return "bg-muted-foreground/30";
  return "bg-muted-foreground/50";
}
