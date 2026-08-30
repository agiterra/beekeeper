import * as React from "react";
import { ArrowDown } from "lucide-react";

import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import type { CodingSessionLiveActivity } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";

/** A 44px presence row for working seats; state already lives in the roster. */
export function CodingSessionLiveActivityBar({
  followState = "following",
  items,
  onFocus,
  onFollow,
}: {
  followState?: "following" | "paused";
  items: readonly CodingSessionLiveActivity[];
  onFocus: (executionKey: string) => void;
  onFollow?: () => void;
}) {
  const nowMs = useLiveActivityClock(items);
  if (items.length === 0) return null;
  return (
    <aside
      aria-label="Live session activity"
      className="flex h-11 min-w-0 shrink-0 items-center gap-2 overflow-hidden rounded-xl border border-border/70 bg-background/95 px-2 shadow-sm"
      data-testid="coding-session-live-activity-bar"
    >
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {items.map((item) => {
          const accent = codingSessionParticipantAccent(item.executionKey);
          const elapsedLabel = formatLiveActivityElapsed(
            item.startedAtMs,
            nowMs,
          );
          return (
            <button
              aria-label={`Focus live activity for ${item.label}`}
              className={cn(
                "inline-flex h-8 min-w-0 shrink-0 items-center gap-2 rounded-lg px-2.5 text-xs transition-colors hover:bg-muted/45 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                accent.text,
              )}
              data-testid="coding-session-live-activity-segment"
              key={item.executionKey}
              onClick={() => onFocus(item.executionKey)}
              type="button"
            >
              <span
                aria-hidden
                className={cn(
                  "coding-session-agent-breathe size-2 rounded-full",
                  accent.dot,
                )}
              />
              <span className="max-w-80 truncate font-medium text-foreground">
                {item.label} · {item.activity ?? "working"}
              </span>
              {elapsedLabel ? (
                <span className="shrink-0 tabular-nums text-muted-foreground">
                  · {elapsedLabel}
                </span>
              ) : null}
              {item.openToolCount ? (
                <span className="shrink-0 text-muted-foreground">
                  · {item.openToolCount}{" "}
                  {item.openToolCount === 1
                    ? "tool this turn"
                    : "tools this turn"}
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
      {onFollow ? (
        <button
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-border/70 bg-muted/25 px-2.5 text-xs text-muted-foreground transition-colors hover:bg-muted/45 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="coding-session-follow-live"
          onClick={onFollow}
          type="button"
        >
          <ArrowDown aria-hidden className="size-3.5" />
          {followState === "paused" ? "New activity ↓" : "Follow live"}
        </button>
      ) : null}
    </aside>
  );
}

export function formatLiveActivityElapsed(
  startedAtMs: number | null,
  nowMs: number,
): string | null {
  if (startedAtMs === null || !Number.isFinite(startedAtMs)) return null;
  if (nowMs <= startedAtMs) return null;
  return formatCodingSessionDuration(nowMs - startedAtMs);
}

function useLiveActivityClock(
  items: readonly CodingSessionLiveActivity[],
): number {
  const shouldTick = items.some(
    (item) => item.startedAtMs !== null && Number.isFinite(item.startedAtMs),
  );
  const [nowMs, setNowMs] = React.useState(() => Date.now());

  React.useEffect(() => {
    if (!shouldTick) return;
    setNowMs(Date.now());
    const interval = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [shouldTick]);

  return nowMs;
}
